use std::collections::HashMap;

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{
        maintenance_schedule_item, model_template, service_record, service_schedule_link, vehicle,
        work_item,
    },
    error::{DomainError, DomainResult},
    inputs::schedule::{NewScheduleItem, ScheduleFilter, UpdateScheduleItem},
};

#[derive(Serialize)]
pub struct ResolvedScheduleItem {
    pub effective_item: maintenance_schedule_item::Model,
    pub inherited_from: Option<String>,
}

pub async fn list(
    db: &impl ConnectionTrait,
    filter: ScheduleFilter,
) -> DomainResult<Vec<maintenance_schedule_item::Model>> {
    let mut select = maintenance_schedule_item::Entity::find();

    if let Some(pid) = filter.platform_id {
        select = select.filter(maintenance_schedule_item::Column::PlatformId.eq(pid));
    }
    if let Some(mtid) = filter.model_template_id {
        select = select.filter(maintenance_schedule_item::Column::ModelTemplateId.eq(mtid));
    }
    if let Some(vid) = filter.vehicle_id {
        select = select.filter(maintenance_schedule_item::Column::VehicleId.eq(vid));
    }

    Ok(select.all(db).await?)
}

pub async fn get(
    db: &impl ConnectionTrait,
    id: i32,
) -> DomainResult<maintenance_schedule_item::Model> {
    maintenance_schedule_item::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Schedule item {id} not found")))
}

pub async fn create(
    db: &impl ConnectionTrait,
    input: NewScheduleItem,
) -> DomainResult<maintenance_schedule_item::Model> {
    // Validate exactly one owner is set
    let owner_count = [
        input.platform_id.is_some(),
        input.model_template_id.is_some(),
        input.vehicle_id.is_some(),
    ]
    .iter()
    .filter(|&&b| b)
    .count();

    if owner_count != 1 {
        return Err(DomainError::invalid(
            "owner",
            "Exactly one of platform_id, model_template_id, or vehicle_id must be set",
        ));
    }

    let model = maintenance_schedule_item::ActiveModel {
        platform_id: Set(input.platform_id),
        model_template_id: Set(input.model_template_id),
        vehicle_id: Set(input.vehicle_id),
        overrides_item_id: Set(input.overrides_item_id),
        name: Set(input.name),
        description: Set(input.description),
        interval_miles: Set(input.interval_miles),
        interval_months: Set(input.interval_months),
        warning_miles: Set(input.warning_miles),
        warning_days: Set(input.warning_days),
        enabled: Set(input.enabled.unwrap_or(true)),
        source: Set(input.source),
        notes: Set(input.notes),
        is_factory_recommended: Set(input.is_factory_recommended),
        labor_categories: Set(input.labor_categories),
        est_cost_cents: Set(input.est_cost_cents),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

pub async fn update(
    db: &impl ConnectionTrait,
    id: i32,
    input: UpdateScheduleItem,
) -> DomainResult<maintenance_schedule_item::Model> {
    let existing = get(db, id).await?;
    let mut active: maintenance_schedule_item::ActiveModel = existing.into();

    if let Some(v) = input.name {
        active.name = Set(v);
    }
    if let Some(v) = input.description {
        active.description = Set(v);
    }
    if let Some(v) = input.interval_miles {
        active.interval_miles = Set(v);
    }
    if let Some(v) = input.interval_months {
        active.interval_months = Set(v);
    }
    if let Some(v) = input.warning_miles {
        active.warning_miles = Set(v);
    }
    if let Some(v) = input.warning_days {
        active.warning_days = Set(v);
    }
    if let Some(v) = input.enabled {
        active.enabled = Set(v);
    }
    if let Some(v) = input.source {
        active.source = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }
    if let Some(v) = input.is_factory_recommended {
        active.is_factory_recommended = Set(v);
    }
    if let Some(v) = input.labor_categories {
        active.labor_categories = Set(v);
    }
    if let Some(v) = input.est_cost_cents {
        active.est_cost_cents = Set(v);
    }

    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
    Ok(active.update(db).await?)
}

/// Creates a schedule item owned by the vehicle. `input.vehicle_id` is
/// overwritten with `vehicle_id` and the other owner fields are cleared, so
/// the item can only ever belong to this vehicle.
///
/// An item named like one the vehicle inherits replaces it for this vehicle,
/// and the vehicle's services and planned work linked to the inherited item
/// move to the new one. A name the vehicle already has its own item for is a
/// `BadRequest`.
pub async fn create_for_vehicle<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    mut input: NewScheduleItem,
) -> DomainResult<maintenance_schedule_item::Model> {
    let v = crate::services::vehicle::require(db, vehicle_id).await?;
    if input.name.trim().is_empty() {
        return Err(DomainError::invalid("name", "must not be blank"));
    }
    let txn = db.begin().await?;
    if let Some(own) = own_item_named(&txn, vehicle_id, &input.name).await? {
        return Err(DomainError::BadRequest(format!(
            "This vehicle already has a schedule item named '{}' (id {}). Change it with \
             update_schedule_item, or bring it back with restore_schedule_item if it was \
             dismissed.",
            own.name, own.id
        )));
    }
    let shadowed = inherited_named(&txn, &v, &input.name).await?;
    input.vehicle_id = Some(vehicle_id);
    input.platform_id = None;
    input.model_template_id = None;
    input.overrides_item_id = effective_inherited(&shadowed).map(|i| i.id);
    let created = create(&txn, input).await?;
    repoint_links(&txn, vehicle_id, &shadowed, created.id).await?;
    txn.commit().await?;
    Ok(created)
}

/// Updates a schedule item as this vehicle sees it.
///
/// A vehicle-owned item is edited in place. An item inherited from the
/// platform or model template is never edited, since other cars share it:
/// the change lands on this vehicle's own copy of it, which is created on
/// first use and reused afterwards, whether the call names the inherited id
/// or the copy's id. The vehicle's services and planned work linked to the
/// inherited item move to the copy, so its history and plans still count.
/// The copy keeps the inherited item's name; a rename is a `BadRequest`.
///
/// An id that is not visible to this vehicle reads as nonexistent.
pub async fn update_for_vehicle<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    schedule_item_id: i32,
    input: UpdateScheduleItem,
) -> DomainResult<maintenance_schedule_item::Model> {
    if input.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
        return Err(DomainError::invalid("name", "must not be blank"));
    }
    let txn = db.begin().await?;
    let item = require_in_vehicle_scope(&txn, vehicle_id, schedule_item_id).await?;
    let v = crate::services::vehicle::require(&txn, vehicle_id).await?;

    let owned = item.vehicle_id == Some(vehicle_id);
    let is_override = !owned || item.overrides_item_id.is_some();
    if is_override && input.name.as_deref().is_some_and(|n| n != item.name) {
        return Err(DomainError::BadRequest(format!(
            "Schedule item {schedule_item_id} follows the '{}' item from the platform or model \
             template, so it keeps that name. Add a differently named job with add_schedule_item.",
            item.name
        )));
    }
    if let Some(name) = input.name.as_deref().filter(|n| *n != item.name)
        && let Some(other) = own_item_named(&txn, vehicle_id, name).await?
    {
        return Err(DomainError::BadRequest(format!(
            "This vehicle already has a schedule item named '{name}' (id {}).",
            other.id
        )));
    }

    let target = if owned {
        item
    } else if let Some(existing) = own_item_named(&txn, vehicle_id, &item.name).await? {
        existing
    } else {
        copy_for_vehicle(&item, vehicle_id).insert(&txn).await?
    };
    let updated = update(&txn, target.id, input).await?;
    let shadowed = inherited_named(&txn, &v, &updated.name).await?;
    repoint_links(&txn, vehicle_id, &shadowed, updated.id).await?;
    txn.commit().await?;
    Ok(updated)
}

/// The vehicle-owned item called `name`, if the vehicle has one.
async fn own_item_named(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    name: &str,
) -> DomainResult<Option<maintenance_schedule_item::Model>> {
    Ok(maintenance_schedule_item::Entity::find()
        .filter(maintenance_schedule_item::Column::VehicleId.eq(vehicle_id))
        .filter(maintenance_schedule_item::Column::Name.eq(name))
        .order_by_asc(maintenance_schedule_item::Column::Id)
        .one(db)
        .await?)
}

/// The platform and model-template items called `name` that the vehicle
/// inherits, whether or not one of them shadows another.
async fn inherited_named(
    db: &impl ConnectionTrait,
    v: &vehicle::Model,
    name: &str,
) -> DomainResult<Vec<maintenance_schedule_item::Model>> {
    let Some(owner) = inherited_owner(db, v).await? else {
        return Ok(Vec::new());
    };
    Ok(maintenance_schedule_item::Entity::find()
        .filter(maintenance_schedule_item::Column::Name.eq(name))
        .filter(owner)
        .all(db)
        .await?)
}

/// Matches the items the vehicle inherits: its model template's and that
/// template's platform's. `None` when the vehicle has no template.
async fn inherited_owner(
    db: &impl ConnectionTrait,
    v: &vehicle::Model,
) -> DomainResult<Option<Condition>> {
    let Some(mt_id) = v.model_template_id else {
        return Ok(None);
    };
    let mut owner =
        Condition::any().add(maintenance_schedule_item::Column::ModelTemplateId.eq(mt_id));
    let platform_id = model_template::Entity::find_by_id(mt_id)
        .one(db)
        .await?
        .and_then(|mt| mt.platform_id);
    if let Some(platform_id) = platform_id {
        owner = owner.add(maintenance_schedule_item::Column::PlatformId.eq(platform_id));
    }
    Ok(Some(owner))
}

/// The one of same-named inherited items that [`resolve`] would show: the
/// model template's over the platform's.
fn effective_inherited(
    items: &[maintenance_schedule_item::Model],
) -> Option<&maintenance_schedule_item::Model> {
    items
        .iter()
        .find(|i| i.model_template_id.is_some())
        .or_else(|| items.first())
}

/// A vehicle-owned copy of an inherited item's full definition, pointing back
/// at it through `overrides_item_id`.
fn copy_for_vehicle(
    item: &maintenance_schedule_item::Model,
    vehicle_id: i32,
) -> maintenance_schedule_item::ActiveModel {
    maintenance_schedule_item::ActiveModel {
        vehicle_id: Set(Some(vehicle_id)),
        overrides_item_id: Set(Some(item.id)),
        name: Set(item.name.clone()),
        description: Set(item.description.clone()),
        interval_miles: Set(item.interval_miles),
        interval_months: Set(item.interval_months),
        warning_miles: Set(item.warning_miles),
        warning_days: Set(item.warning_days),
        enabled: Set(item.enabled),
        source: Set(item.source.clone()),
        notes: Set(item.notes.clone()),
        is_factory_recommended: Set(item.is_factory_recommended),
        labor_categories: Set(item.labor_categories.clone()),
        est_cost_cents: Set(item.est_cost_cents),
        ..Default::default()
    }
}

/// Moves this vehicle's service links and planned work from the `shadowed`
/// inherited items to the vehicle's own item `target_id`. Other vehicles'
/// rows are untouched: a service belongs to one vehicle, and so does a work
/// item.
async fn repoint_links(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    shadowed: &[maintenance_schedule_item::Model],
    target_id: i32,
) -> DomainResult<()> {
    let from: Vec<i32> = shadowed.iter().map(|i| i.id).collect();
    if from.is_empty() {
        return Ok(());
    }
    let own_services = service_record::Entity::find()
        .select_only()
        .column(service_record::Column::Id)
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .into_query();
    let links = service_schedule_link::Entity::find()
        .filter(service_schedule_link::Column::ScheduleItemId.is_in(from.clone()))
        .filter(service_schedule_link::Column::ServiceRecordId.in_subquery(own_services))
        .all(db)
        .await?;
    for link in links {
        let service_record_id = link.service_record_id;
        link.delete(db).await?;
        let already = service_schedule_link::Entity::find_by_id((service_record_id, target_id))
            .one(db)
            .await?
            .is_some();
        if !already {
            service_schedule_link::ActiveModel {
                service_record_id: Set(service_record_id),
                schedule_item_id: Set(target_id),
            }
            .insert(db)
            .await?;
        }
    }

    work_item::Entity::update_many()
        .set(work_item::ActiveModel {
            schedule_item_id: Set(Some(target_id)),
            updated_at: Set(super::now_stamp()),
            ..Default::default()
        })
        .filter(work_item::Column::VehicleId.eq(vehicle_id))
        .filter(work_item::Column::ScheduleItemId.is_in(from))
        .exec(db)
        .await?;
    Ok(())
}

/// Checks that every id is in the vehicle's schedule scope and maps each one
/// to the item the vehicle actually tracks: an inherited id the vehicle has
/// its own same-named item for becomes that item's id. Duplicates are
/// dropped, first occurrence kept. An id outside the scope reads as
/// nonexistent.
pub(crate) async fn tracked_ids(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    ids: &[i32],
) -> DomainResult<Vec<i32>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let v = crate::services::vehicle::require(db, vehicle_id).await?;
    let mut owner =
        Condition::any().add(maintenance_schedule_item::Column::VehicleId.eq(vehicle_id));
    if let Some(inherited) = inherited_owner(db, &v).await? {
        owner = owner.add(inherited);
    }
    let found: HashMap<i32, maintenance_schedule_item::Model> =
        maintenance_schedule_item::Entity::find()
            .filter(maintenance_schedule_item::Column::Id.is_in(ids.to_vec()))
            .filter(owner)
            .all(db)
            .await?
            .into_iter()
            .map(|i| (i.id, i))
            .collect();
    if let Some(missing) = ids.iter().find(|id| !found.contains_key(id)) {
        return Err(DomainError::NotFound(format!(
            "Schedule item {missing} not found"
        )));
    }

    let inherited_names: Vec<String> = found
        .values()
        .filter(|i| i.vehicle_id != Some(vehicle_id))
        .map(|i| i.name.clone())
        .collect();
    let mut own_by_name: HashMap<String, i32> = HashMap::new();
    if !inherited_names.is_empty() {
        let own = maintenance_schedule_item::Entity::find()
            .filter(maintenance_schedule_item::Column::VehicleId.eq(vehicle_id))
            .filter(maintenance_schedule_item::Column::Name.is_in(inherited_names))
            .order_by_desc(maintenance_schedule_item::Column::Id)
            .all(db)
            .await?;
        // Descending ids, so the lowest id wins when a name repeats.
        for item in own {
            own_by_name.insert(item.name, item.id);
        }
    }

    let mut seen = std::collections::HashSet::new();
    Ok(ids
        .iter()
        .map(|id| {
            let item = &found[id];
            if item.vehicle_id == Some(vehicle_id) {
                *id
            } else {
                own_by_name.get(&item.name).copied().unwrap_or(*id)
            }
        })
        .filter(|id| seen.insert(*id))
        .collect())
}

pub async fn delete(db: &impl ConnectionTrait, id: i32) -> DomainResult<()> {
    let result = maintenance_schedule_item::Entity::delete_by_id(id)
        .exec(db)
        .await?;
    if result.rows_affected == 0 {
        return Err(DomainError::NotFound(format!(
            "Schedule item {id} not found"
        )));
    }
    Ok(())
}

/// Resolve the effective maintenance schedule for a vehicle.
/// Implements the 3-level inheritance: Platform → Model Template → Vehicle.
/// Dismissed and otherwise disabled items are left out.
pub async fn resolve(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<Vec<ResolvedScheduleItem>> {
    resolve_for_listing(db, vehicle_id, false).await
}

/// [`resolve`], keeping disabled items when `include_dismissed` is true. A
/// dismissed item then appears as the vehicle's disabled row, whose id is
/// what [`undismiss_for_vehicle`] takes.
pub async fn resolve_for_listing(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    include_dismissed: bool,
) -> DomainResult<Vec<ResolvedScheduleItem>> {
    let v = vehicle::Entity::find_by_id(vehicle_id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Vehicle {vehicle_id} not found")))?;

    let mut schedule: HashMap<String, ResolvedScheduleItem> = HashMap::new();

    if let Some(mt_id) = v.model_template_id {
        let mt = model_template::Entity::find_by_id(mt_id).one(db).await?;

        if let Some(mt) = &mt {
            // Layer 1: Platform items (via model template → platform)
            if let Some(platform_id) = mt.platform_id {
                let platform_items = maintenance_schedule_item::Entity::find()
                    .filter(maintenance_schedule_item::Column::PlatformId.eq(platform_id))
                    .all(db)
                    .await?;

                for item in platform_items {
                    let name = item.name.clone();
                    schedule.insert(
                        name,
                        ResolvedScheduleItem {
                            effective_item: item,
                            inherited_from: Some("platform".to_string()),
                        },
                    );
                }
            }

            // Layer 2: Model template items override platform items by name
            let template_items = maintenance_schedule_item::Entity::find()
                .filter(maintenance_schedule_item::Column::ModelTemplateId.eq(mt_id))
                .all(db)
                .await?;

            for item in template_items {
                let name = item.name.clone();
                schedule.insert(
                    name,
                    ResolvedScheduleItem {
                        effective_item: item,
                        inherited_from: Some("model_template".to_string()),
                    },
                );
            }
        }
    }

    // Layer 3: Vehicle-level items override everything
    let vehicle_items = maintenance_schedule_item::Entity::find()
        .filter(maintenance_schedule_item::Column::VehicleId.eq(vehicle_id))
        .all(db)
        .await?;

    for item in vehicle_items {
        let name = item.name.clone();
        schedule.insert(
            name,
            ResolvedScheduleItem {
                effective_item: item,
                inherited_from: None,
            },
        );
    }

    // Leave out disabled items unless asked, and sort by name for stable output
    let mut result: Vec<ResolvedScheduleItem> = schedule
        .into_values()
        .filter(|r| include_dismissed || r.effective_item.enabled)
        .collect();
    result.sort_by(|a, b| a.effective_item.name.cmp(&b.effective_item.name));

    Ok(result)
}

/// Load a schedule item only if it's within the vehicle's schedule scope:
/// owned by the vehicle itself, by its model template, or by that template's
/// platform. Anything else — including another vehicle's items — must be
/// indistinguishable from a nonexistent item.
/// `pub(crate)`: shared single-id guard (`work_item` links reuse it).
pub(crate) async fn require_in_vehicle_scope(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    schedule_item_id: i32,
) -> DomainResult<maintenance_schedule_item::Model> {
    let v = crate::services::vehicle::require(db, vehicle_id).await?;
    let not_found = || DomainError::NotFound(format!("Schedule item {schedule_item_id} not found"));

    let item = maintenance_schedule_item::Entity::find_by_id(schedule_item_id)
        .one(db)
        .await?
        .ok_or_else(not_found)?;

    if item.vehicle_id == Some(vehicle_id) {
        return Ok(item);
    }
    if let Some(mt_id) = v.model_template_id {
        if item.model_template_id == Some(mt_id) {
            return Ok(item);
        }
        if item.platform_id.is_some() {
            let mt = model_template::Entity::find_by_id(mt_id).one(db).await?;
            if mt.is_some_and(|mt| mt.platform_id == item.platform_id) {
                return Ok(item);
            }
        }
    }
    Err(not_found())
}

/// Append a dismissal reason to a notes blob, preserving what's there.
fn append_dismiss_reason(notes: Option<String>, reason: Option<&str>) -> Option<String> {
    let Some(reason) = reason else { return notes };
    let line = format!("Dismissed: {reason}");
    Some(match notes {
        Some(existing) if !existing.is_empty() => format!("{existing}\n{line}"),
        _ => line,
    })
}

/// Dismisses a schedule item for one vehicle by disabling the vehicle's own
/// row for it.
///
/// A vehicle-owned item is disabled in place. An inherited item gets the
/// vehicle's own copy of it (created on first use, as
/// [`update_for_vehicle`] does), disabled, and the vehicle's services and
/// planned work linked to the inherited item move to the copy. Returns the
/// disabled row; its id is what [`undismiss_for_vehicle`] takes. A `reason`
/// is appended to the row's notes.
pub async fn dismiss_for_vehicle<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    schedule_item_id: i32,
    reason: Option<String>,
) -> DomainResult<maintenance_schedule_item::Model> {
    let txn = db.begin().await?;
    let item = require_in_vehicle_scope(&txn, vehicle_id, schedule_item_id).await?;
    let v = crate::services::vehicle::require(&txn, vehicle_id).await?;

    // Re-dismissing an inherited item reuses the vehicle's existing copy
    // rather than stacking a second one.
    let target = if item.vehicle_id == Some(vehicle_id) {
        item
    } else if let Some(existing) = own_item_named(&txn, vehicle_id, &item.name).await? {
        existing
    } else {
        copy_for_vehicle(&item, vehicle_id).insert(&txn).await?
    };

    let notes = append_dismiss_reason(target.notes.clone(), reason.as_deref());
    let mut active: maintenance_schedule_item::ActiveModel = target.into();
    active.enabled = Set(false);
    active.notes = Set(notes);
    active.updated_at = Set(super::now_stamp());
    let dismissed = active.update(&txn).await?;

    let shadowed = inherited_named(&txn, &v, &dismissed.name).await?;
    repoint_links(&txn, vehicle_id, &shadowed, dismissed.id).await?;
    txn.commit().await?;
    Ok(dismissed)
}

/// Reverses a dismissal by re-enabling the vehicle-owned row that
/// [`dismiss_for_vehicle`] returned. A restored copy of an inherited item
/// keeps the definition it had when it was copied and does not follow
/// later changes to the template.
///
/// Rows not owned by this vehicle are indistinguishable from nonexistent.
pub async fn undismiss_for_vehicle(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    schedule_item_id: i32,
) -> DomainResult<maintenance_schedule_item::Model> {
    crate::services::vehicle::require(db, vehicle_id).await?;
    let item = maintenance_schedule_item::Entity::find_by_id(schedule_item_id)
        .filter(maintenance_schedule_item::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| {
            DomainError::NotFound(format!("Schedule item {schedule_item_id} not found"))
        })?;

    let mut active: maintenance_schedule_item::ActiveModel = item.into();
    active.enabled = Set(true);
    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
    Ok(active.update(db).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{VehicleFixture, test_db};

    async fn seed_vehicle(db: &impl ConnectionTrait, model_template_id: Option<i32>) -> i32 {
        let mut fixture = VehicleFixture::new();
        if let Some(id) = model_template_id {
            fixture = fixture.model_template_id(id);
        }
        fixture.insert_id(db).await
    }

    #[tokio::test]
    async fn create_requires_exactly_one_owner() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let err = create(
            &db,
            NewScheduleItem {
                platform_id: Some(1),
                model_template_id: None,
                vehicle_id: Some(vid),
                overrides_item_id: None,
                name: "Oil change".into(),
                description: None,
                interval_miles: Some(5000),
                interval_months: None,
                warning_miles: None,
                warning_days: None,
                enabled: None,
                source: None,
                notes: None,
                is_factory_recommended: None,
                labor_categories: None,
                est_cost_cents: None,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { .. }));
    }

    #[tokio::test]
    async fn create_then_get_and_update() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let created = create(
            &db,
            NewScheduleItem {
                platform_id: None,
                model_template_id: None,
                vehicle_id: Some(vid),
                overrides_item_id: None,
                name: "Oil change".into(),
                description: None,
                interval_miles: Some(5000),
                interval_months: None,
                warning_miles: None,
                warning_days: None,
                enabled: None,
                source: None,
                notes: None,
                is_factory_recommended: None,
                labor_categories: None,
                est_cost_cents: None,
            },
        )
        .await
        .unwrap();
        // enabled defaults to true
        assert!(created.enabled);
        assert_eq!(get(&db, created.id).await.unwrap().name, "Oil change");

        let updated = update(
            &db,
            created.id,
            UpdateScheduleItem {
                interval_miles: Some(Some(7500)),
                enabled: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.interval_miles, Some(7500));
        assert!(!updated.enabled);
    }

    #[tokio::test]
    async fn resolve_applies_vehicle_layer_and_hides_disabled() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        create(
            &db,
            NewScheduleItem {
                platform_id: None,
                model_template_id: None,
                vehicle_id: Some(vid),
                overrides_item_id: None,
                name: "Tire rotation".into(),
                description: None,
                interval_miles: Some(6000),
                interval_months: None,
                warning_miles: None,
                warning_days: None,
                enabled: Some(true),
                source: None,
                notes: None,
                is_factory_recommended: None,
                labor_categories: None,
                est_cost_cents: None,
            },
        )
        .await
        .unwrap();
        create(
            &db,
            NewScheduleItem {
                platform_id: None,
                model_template_id: None,
                vehicle_id: Some(vid),
                overrides_item_id: None,
                name: "Disabled item".into(),
                description: None,
                interval_miles: None,
                interval_months: None,
                warning_miles: None,
                warning_days: None,
                enabled: Some(false),
                source: None,
                notes: None,
                is_factory_recommended: None,
                labor_categories: None,
                est_cost_cents: None,
            },
        )
        .await
        .unwrap();

        let resolved = resolve(&db, vid).await.unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].effective_item.name, "Tire rotation");
        assert_eq!(resolved[0].inherited_from, None);
    }

    #[tokio::test]
    async fn delete_missing_is_not_found() {
        let db = test_db().await;
        assert!(matches!(
            delete(&db, 999).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    fn item_named(name: &str) -> NewScheduleItem {
        NewScheduleItem {
            platform_id: None,
            model_template_id: None,
            vehicle_id: None,
            overrides_item_id: None,
            name: name.into(),
            description: None,
            interval_miles: Some(5000),
            interval_months: None,
            warning_miles: None,
            warning_days: None,
            enabled: None,
            source: None,
            notes: None,
            is_factory_recommended: None,
            labor_categories: None,
            est_cost_cents: None,
        }
    }

    /// Platform → template → vehicle chain; returns (`platform_id`,
    /// `template_id`, `vehicle_id`).
    async fn seed_chain(db: &impl ConnectionTrait) -> (i32, i32, i32) {
        use crate::entities::platform;
        let platform_id = platform::ActiveModel {
            name: Set("MQB".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id;
        let template_id = model_template::ActiveModel {
            platform_id: Set(Some(platform_id)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id;
        let vid = seed_vehicle(db, Some(template_id)).await;
        (platform_id, template_id, vid)
    }

    #[tokio::test]
    async fn dismiss_vehicle_owned_disables_in_place() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let item = create(
            &db,
            NewScheduleItem {
                vehicle_id: Some(vid),
                notes: Some("factory says 5k".into()),
                ..item_named("Oil change")
            },
        )
        .await
        .unwrap();

        let dismissed =
            dismiss_for_vehicle(&db, vid, item.id, Some("switched to synthetic".into()))
                .await
                .unwrap();
        assert_eq!(
            dismissed.id, item.id,
            "no shadow row for vehicle-owned items"
        );
        assert!(!dismissed.enabled);
        let notes = dismissed.notes.as_deref().unwrap();
        assert!(
            notes.contains("factory says 5k"),
            "existing notes kept: {notes}"
        );
        assert!(
            notes.contains("switched to synthetic"),
            "reason appended: {notes}"
        );

        assert!(resolve(&db, vid).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn dismiss_inherited_creates_hidden_shadow() {
        let db = test_db().await;
        let (_pid, template_id, vid) = seed_chain(&db).await;
        let inherited = create(
            &db,
            NewScheduleItem {
                model_template_id: Some(template_id),
                ..item_named("Haldex service")
            },
        )
        .await
        .unwrap();
        assert_eq!(resolve(&db, vid).await.unwrap().len(), 1);

        let shadow = dismiss_for_vehicle(&db, vid, inherited.id, None)
            .await
            .unwrap();
        assert_ne!(shadow.id, inherited.id);
        assert_eq!(shadow.vehicle_id, Some(vid));
        assert_eq!(shadow.overrides_item_id, Some(inherited.id));
        assert_eq!(shadow.name, "Haldex service");
        assert!(!shadow.enabled);
        // The shadow copies the source definition so a later re-enable is
        // behaviorally identical to the inherited item.
        assert_eq!(shadow.interval_miles, inherited.interval_miles);

        // Name-shadowing + disabled filter hide it from the resolved schedule.
        assert!(resolve(&db, vid).await.unwrap().is_empty());
        // The inherited item itself is untouched.
        assert!(get(&db, inherited.id).await.unwrap().enabled);
    }

    #[tokio::test]
    async fn dismiss_inherited_twice_reuses_the_existing_override() {
        let db = test_db().await;
        let (_pid, template_id, vid) = seed_chain(&db).await;
        let inherited = create(
            &db,
            NewScheduleItem {
                model_template_id: Some(template_id),
                ..item_named("Haldex service")
            },
        )
        .await
        .unwrap();

        let first = dismiss_for_vehicle(&db, vid, inherited.id, None)
            .await
            .unwrap();
        let second = dismiss_for_vehicle(&db, vid, inherited.id, None)
            .await
            .unwrap();
        assert_eq!(first.id, second.id, "no duplicate shadow rows");

        let vehicle_rows = list(
            &db,
            ScheduleFilter {
                vehicle_id: Some(vid),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(vehicle_rows.len(), 1);
    }

    #[tokio::test]
    async fn dismiss_wrong_vehicle_is_indistinguishable_not_found() {
        let db = test_db().await;
        let owner = seed_vehicle(&db, None).await;
        let intruder = seed_vehicle(&db, None).await;
        let item = create(
            &db,
            NewScheduleItem {
                vehicle_id: Some(owner),
                ..item_named("Oil change")
            },
        )
        .await
        .unwrap();

        // Another vehicle's item and a nonexistent item must be the same error.
        let foreign = dismiss_for_vehicle(&db, intruder, item.id, None)
            .await
            .unwrap_err();
        let missing = dismiss_for_vehicle(&db, intruder, 9999, None)
            .await
            .unwrap_err();
        assert!(matches!(foreign, DomainError::NotFound(_)));
        assert!(matches!(missing, DomainError::NotFound(_)));
        assert_eq!(
            foreign.to_string().replace(&item.id.to_string(), "N"),
            missing.to_string().replace("9999", "N"),
            "wrong-vehicle must be indistinguishable from nonexistent"
        );
        // Nothing was disabled or shadowed.
        assert!(get(&db, item.id).await.unwrap().enabled);
        assert_eq!(resolve(&db, owner).await.unwrap().len(), 1);

        // A template item outside the vehicle's chain is also invisible.
        let (_pid, template_id, _chain_vid) = seed_chain(&db).await;
        let unrelated = create(
            &db,
            NewScheduleItem {
                model_template_id: Some(template_id),
                ..item_named("Unrelated template item")
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            dismiss_for_vehicle(&db, owner, unrelated.id, None)
                .await
                .unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn undismiss_restores_vehicle_owned_and_shadowed_items() {
        let db = test_db().await;
        let (_pid, template_id, vid) = seed_chain(&db).await;
        let own = create(
            &db,
            NewScheduleItem {
                vehicle_id: Some(vid),
                ..item_named("Oil change")
            },
        )
        .await
        .unwrap();
        let inherited = create(
            &db,
            NewScheduleItem {
                model_template_id: Some(template_id),
                ..item_named("Haldex service")
            },
        )
        .await
        .unwrap();

        dismiss_for_vehicle(&db, vid, own.id, None).await.unwrap();
        let shadow = dismiss_for_vehicle(&db, vid, inherited.id, None)
            .await
            .unwrap();
        assert!(resolve(&db, vid).await.unwrap().is_empty());

        let restored = undismiss_for_vehicle(&db, vid, own.id).await.unwrap();
        assert!(restored.enabled);
        let restored_shadow = undismiss_for_vehicle(&db, vid, shadow.id).await.unwrap();
        assert!(restored_shadow.enabled);

        let resolved = resolve(&db, vid).await.unwrap();
        let names: Vec<&str> = resolved
            .iter()
            .map(|r| r.effective_item.name.as_str())
            .collect();
        assert_eq!(names, vec!["Haldex service", "Oil change"]);
    }

    #[tokio::test]
    async fn undismiss_wrong_vehicle_is_indistinguishable_not_found() {
        let db = test_db().await;
        let owner = seed_vehicle(&db, None).await;
        let intruder = seed_vehicle(&db, None).await;
        let item = create(
            &db,
            NewScheduleItem {
                vehicle_id: Some(owner),
                enabled: Some(false),
                ..item_named("Oil change")
            },
        )
        .await
        .unwrap();

        let foreign = undismiss_for_vehicle(&db, intruder, item.id)
            .await
            .unwrap_err();
        let missing = undismiss_for_vehicle(&db, intruder, 9999)
            .await
            .unwrap_err();
        assert!(matches!(foreign, DomainError::NotFound(_)));
        assert!(matches!(missing, DomainError::NotFound(_)));
        assert_eq!(
            foreign.to_string().replace(&item.id.to_string(), "N"),
            missing.to_string().replace("9999", "N"),
            "wrong-vehicle must be indistinguishable from nonexistent"
        );
        assert!(!get(&db, item.id).await.unwrap().enabled, "untouched");
    }

    fn days_ago(days: i64) -> String {
        (chrono::Utc::now().date_naive() - chrono::Duration::days(days))
            .format("%Y-%m-%d")
            .to_string()
    }

    async fn service_on(db: &DatabaseConnection, vehicle_id: i32, item_id: i32) -> i32 {
        crate::services::service_record::create(
            db,
            vehicle_id,
            crate::inputs::service_record::NewServiceRecord {
                service_date: days_ago(30),
                schedule_item_ids: Some(vec![item_id]),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .record
        .id
    }

    fn reminder_for<'a>(
        rems: &'a crate::services::reminders::RemindersResponse,
        name: &str,
    ) -> &'a crate::services::reminders::ReminderStatus {
        rems.reminders
            .iter()
            .find(|r| r.schedule_item.name == name)
            .unwrap_or_else(|| panic!("no reminder named {name}"))
    }

    #[tokio::test]
    async fn changing_an_inherited_item_keeps_its_history_and_planned_work() {
        use crate::services::{budget, reminders, work_item};
        let db = test_db().await;
        let (_pid, template_id, vid) = seed_chain(&db).await;
        let inherited = create(
            &db,
            NewScheduleItem {
                model_template_id: Some(template_id),
                interval_miles: None,
                interval_months: Some(12),
                ..item_named("Spark Plugs")
            },
        )
        .await
        .unwrap();
        let svc = service_on(&db, vid, inherited.id).await;
        let planned = work_item::create(
            &db,
            vid,
            crate::inputs::work_item::NewWorkItem {
                title: "Plugs".into(),
                notes: None,
                schedule_item_id: Some(inherited.id),
                research_finding_id: None,
                incident_id: None,
                est_cost_cents: Some(9_000),
                revisit_by_date: None,
                revisit_by_miles: None,
            },
        )
        .await
        .unwrap();

        let over = update_for_vehicle(
            &db,
            vid,
            inherited.id,
            UpdateScheduleItem {
                est_cost_cents: Some(Some(12_000)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_ne!(over.id, inherited.id, "the shared item is never edited");
        assert_eq!(over.vehicle_id, Some(vid));
        assert_eq!(over.overrides_item_id, Some(inherited.id));
        assert_eq!(over.interval_months, Some(12), "definition copied");
        assert_eq!(over.est_cost_cents, Some(12_000));
        assert_eq!(get(&db, inherited.id).await.unwrap().est_cost_cents, None);

        let rems = reminders::calculate_reminders(&db, vid).await.unwrap();
        let plugs = reminder_for(&rems, "Spark Plugs");
        assert_eq!(plugs.schedule_item.id, over.id);
        assert_eq!(plugs.status, "ok");
        assert_eq!(plugs.last_service.as_ref().map(|s| s.id), Some(svc));

        // The planned work now points at the override, so it still stands in
        // for the first projected occurrence.
        let moved = work_item::get(&db, vid, planned.id).await.unwrap();
        assert_eq!(moved.schedule_item_id, Some(over.id));
        let f = budget::forecast_from(&db, vid, &rems).await.unwrap();
        assert!(
            !f.lines.iter().any(|l| l.label == "Spark Plugs"),
            "first occurrence is already planned: {:?}",
            f.lines
        );

        // Calling again, on either id, edits the same override.
        for id in [inherited.id, over.id] {
            let again = update_for_vehicle(
                &db,
                vid,
                id,
                UpdateScheduleItem {
                    notes: Some(Some(format!("edit via {id}"))),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            assert_eq!(again.id, over.id);
        }
        let own_rows = list(
            &db,
            ScheduleFilter {
                vehicle_id: Some(vid),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(own_rows.len(), 1, "one override, never two");
    }

    #[tokio::test]
    async fn overriding_an_item_on_one_car_leaves_the_other_cars_links_alone() {
        use crate::services::reminders;
        let db = test_db().await;
        let (_pid, template_id, first) = seed_chain(&db).await;
        let second = seed_vehicle(&db, Some(template_id)).await;
        let inherited = create(
            &db,
            NewScheduleItem {
                model_template_id: Some(template_id),
                interval_miles: None,
                interval_months: Some(12),
                ..item_named("Spark Plugs")
            },
        )
        .await
        .unwrap();
        service_on(&db, first, inherited.id).await;
        let second_svc = service_on(&db, second, inherited.id).await;

        update_for_vehicle(
            &db,
            first,
            inherited.id,
            UpdateScheduleItem {
                est_cost_cents: Some(Some(12_000)),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let rems = reminders::calculate_reminders(&db, second).await.unwrap();
        let plugs = reminder_for(&rems, "Spark Plugs");
        assert_eq!(plugs.schedule_item.id, inherited.id);
        assert_eq!(plugs.last_service.as_ref().map(|s| s.id), Some(second_svc));
    }

    #[tokio::test]
    async fn adding_an_item_that_shadows_an_inherited_one_keeps_its_history() {
        use crate::services::reminders;
        let db = test_db().await;
        let (_pid, template_id, vid) = seed_chain(&db).await;
        let inherited = create(
            &db,
            NewScheduleItem {
                model_template_id: Some(template_id),
                interval_miles: None,
                interval_months: Some(12),
                ..item_named("Spark Plugs")
            },
        )
        .await
        .unwrap();
        let svc = service_on(&db, vid, inherited.id).await;

        let own = create_for_vehicle(
            &db,
            vid,
            NewScheduleItem {
                interval_months: Some(24),
                ..item_named("Spark Plugs")
            },
        )
        .await
        .unwrap();
        assert_eq!(own.overrides_item_id, Some(inherited.id));

        let rems = reminders::calculate_reminders(&db, vid).await.unwrap();
        let plugs = reminder_for(&rems, "Spark Plugs");
        assert_eq!(plugs.schedule_item.id, own.id);
        assert_eq!(plugs.last_service.as_ref().map(|s| s.id), Some(svc));

        // A second item under the same name would make the schedule ambiguous.
        let err = create_for_vehicle(&db, vid, item_named("Spark Plugs"))
            .await
            .unwrap_err();
        assert!(
            matches!(&err, DomainError::BadRequest(m) if m.contains("update_schedule_item")),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn an_inherited_item_keeps_its_name() {
        let db = test_db().await;
        let (_pid, template_id, vid) = seed_chain(&db).await;
        let inherited = create(
            &db,
            NewScheduleItem {
                model_template_id: Some(template_id),
                ..item_named("Spark Plugs")
            },
        )
        .await
        .unwrap();
        let err = update_for_vehicle(
            &db,
            vid,
            inherited.id,
            UpdateScheduleItem {
                name: Some("Plugs".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "got: {err}");
        assert!(
            list(
                &db,
                ScheduleFilter {
                    vehicle_id: Some(vid),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .is_empty(),
            "a refused change leaves no override behind"
        );
    }

    #[tokio::test]
    async fn dismissing_and_restoring_an_inherited_item_keeps_its_history() {
        use crate::services::reminders;
        let db = test_db().await;
        let (_pid, template_id, vid) = seed_chain(&db).await;
        let inherited = create(
            &db,
            NewScheduleItem {
                model_template_id: Some(template_id),
                interval_miles: None,
                interval_months: Some(12),
                ..item_named("Spark Plugs")
            },
        )
        .await
        .unwrap();
        let svc = service_on(&db, vid, inherited.id).await;
        let shadow = dismiss_for_vehicle(&db, vid, inherited.id, None)
            .await
            .unwrap();
        undismiss_for_vehicle(&db, vid, shadow.id).await.unwrap();

        let rems = reminders::calculate_reminders(&db, vid).await.unwrap();
        let plugs = reminder_for(&rems, "Spark Plugs");
        assert_eq!(plugs.last_service.as_ref().map(|s| s.id), Some(svc));
    }

    #[tokio::test]
    async fn links_sent_with_a_shadowed_id_land_on_the_cars_own_item() {
        let db = test_db().await;
        let (_pid, template_id, vid) = seed_chain(&db).await;
        let inherited = create(
            &db,
            NewScheduleItem {
                model_template_id: Some(template_id),
                ..item_named("Spark Plugs")
            },
        )
        .await
        .unwrap();
        let over = update_for_vehicle(
            &db,
            vid,
            inherited.id,
            UpdateScheduleItem {
                est_cost_cents: Some(Some(1)),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let record = crate::services::service_record::create(
            &db,
            vid,
            crate::inputs::service_record::NewServiceRecord {
                service_date: days_ago(1),
                schedule_item_ids: Some(vec![inherited.id, over.id]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(record.schedule_item_ids, vec![over.id]);
    }

    #[tokio::test]
    async fn update_for_vehicle_edits_own_items_only() {
        let db = test_db().await;
        let template_id = crate::services::model_template::list(&db).await.unwrap()[0].id;
        let vid = seed_vehicle(&db, Some(template_id)).await;
        let other = seed_vehicle(&db, None).await;

        let own = create_for_vehicle(
            &db,
            vid,
            NewScheduleItem {
                name: "Smog check".into(),
                interval_months: Some(24),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let updated = update_for_vehicle(
            &db,
            vid,
            own.id,
            UpdateScheduleItem {
                est_cost_cents: Some(Some(5800)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.est_cost_cents, Some(5800));

        let shared = create(
            &db,
            NewScheduleItem {
                model_template_id: Some(template_id),
                name: "Shared item".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let over = update_for_vehicle(&db, vid, shared.id, UpdateScheduleItem::default())
            .await
            .unwrap();
        assert_eq!(over.overrides_item_id, Some(shared.id));
        assert_eq!(over.vehicle_id, Some(vid));

        let foreign = update_for_vehicle(&db, other, own.id, UpdateScheduleItem::default())
            .await
            .unwrap_err();
        let missing = update_for_vehicle(&db, other, 99_999, UpdateScheduleItem::default())
            .await
            .unwrap_err();
        assert_eq!(
            foreign.to_string().replace(&own.id.to_string(), "N"),
            missing.to_string().replace("99999", "N")
        );
    }
}
