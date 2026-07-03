use std::collections::HashMap;

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{maintenance_schedule_item, model_template, vehicle},
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

    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
    Ok(active.update(db).await?)
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
pub async fn resolve(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
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

    // Filter out disabled items and sort by name for stable output
    let mut result: Vec<ResolvedScheduleItem> = schedule
        .into_values()
        .filter(|r| r.effective_item.enabled)
        .collect();
    result.sort_by(|a, b| a.effective_item.name.cmp(&b.effective_item.name));

    Ok(result)
}

/// Load a schedule item only if it's within the vehicle's schedule scope:
/// owned by the vehicle itself, by its model template, or by that template's
/// platform. Anything else — including another vehicle's items — must be
/// indistinguishable from a nonexistent item (paxy discipline).
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

/// Dismiss a schedule item for one vehicle via the existing vehicle-level
/// `enabled = false` override — no new state (design decision ⑤).
///
/// Vehicle-owned items (including an existing same-name override) are
/// disabled in place. Inherited platform/template items get a vehicle-owned
/// shadow row (same `name`, full definition copied, `enabled = false`,
/// `overrides_item_id` pointing at the source) which `resolve()`'s
/// name-shadowing already hides. Returns the override row — its id is what
/// `undismiss_for_vehicle` takes.
pub async fn dismiss_for_vehicle(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    schedule_item_id: i32,
    reason: Option<String>,
) -> DomainResult<maintenance_schedule_item::Model> {
    let item = require_in_vehicle_scope(db, vehicle_id, schedule_item_id).await?;

    // The row to disable: the item itself when vehicle-owned, else any
    // existing vehicle-owned override with the same name (re-dismissing an
    // inherited item must not stack duplicate shadows).
    let in_place = if item.vehicle_id == Some(vehicle_id) {
        Some(item.clone())
    } else {
        maintenance_schedule_item::Entity::find()
            .filter(maintenance_schedule_item::Column::VehicleId.eq(vehicle_id))
            .filter(maintenance_schedule_item::Column::Name.eq(item.name.clone()))
            .one(db)
            .await?
    };

    if let Some(existing) = in_place {
        let notes = append_dismiss_reason(existing.notes.clone(), reason.as_deref());
        let mut active: maintenance_schedule_item::ActiveModel = existing.into();
        active.enabled = Set(false);
        active.notes = Set(notes);
        active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
        return Ok(active.update(db).await?);
    }

    let shadow = maintenance_schedule_item::ActiveModel {
        vehicle_id: Set(Some(vehicle_id)),
        overrides_item_id: Set(Some(item.id)),
        name: Set(item.name.clone()),
        description: Set(item.description.clone()),
        interval_miles: Set(item.interval_miles),
        interval_months: Set(item.interval_months),
        warning_miles: Set(item.warning_miles),
        warning_days: Set(item.warning_days),
        enabled: Set(false),
        source: Set(item.source.clone()),
        notes: Set(append_dismiss_reason(item.notes.clone(), reason.as_deref())),
        is_factory_recommended: Set(item.is_factory_recommended),
        labor_categories: Set(item.labor_categories.clone()),
        ..Default::default()
    };
    Ok(shadow.insert(db).await?)
}

/// Reverse a dismissal: re-enable the vehicle-owned override row (the model
/// `dismiss_for_vehicle` returned). Re-enabling is the single code path for
/// both dismissal shapes — a shadow copies its source's full definition, so
/// re-enabling it behaves identically to the inherited item it replaced
/// (future template edits stop flowing through; the item simply reads as
/// vehicle-owned from then on, which the schedule UI already renders).
/// Deleting shadows instead would need a second path for in-place-disabled
/// items and would drop the recorded dismissal reason.
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
    use crate::test_support::test_db;

    async fn seed_vehicle(db: &impl ConnectionTrait, model_template_id: Option<i32>) -> i32 {
        vehicle::ActiveModel {
            name: Set("Car".into()),
            model_template_id: Set(model_template_id),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
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
}
