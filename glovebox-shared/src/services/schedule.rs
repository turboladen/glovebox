use std::collections::HashMap;

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{
        maintenance_schedule_item, model_template, service_record, service_schedule_link, vehicle,
        work_item,
    },
    error::{DomainError, DomainResult},
    inputs::schedule::{NewScheduleItem, UpdateScheduleItem},
};

/// Miles before an item's due point at which it reads as due soon, when the
/// item sets no warning of its own.
pub const DEFAULT_WARNING_MILES: i32 = 1000;
/// Days before an item's due date at which it reads as due soon, when the item
/// sets no warning of its own.
pub const DEFAULT_WARNING_DAYS: i32 = 30;

/// The form of a schedule item name that every name comparison uses: ASCII
/// case and surrounding whitespace do not matter. Two names with the same key
/// are the same job, so a vehicle's own item shadows an inherited one of that
/// name whatever its spelling.
pub(crate) fn name_key(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

#[derive(Serialize)]
pub struct ResolvedScheduleItem {
    pub effective_item: maintenance_schedule_item::Model,
    pub inherited_from: Option<String>,
    /// For a vehicle-owned item that replaces an inherited one by name, the
    /// replaced item; `None` otherwise.
    pub overrides: Option<OverriddenItem>,
}

/// The inherited item a vehicle-owned item replaces.
#[derive(Serialize)]
pub struct OverriddenItem {
    pub id: i32,
    /// `platform` or `model_template`.
    pub inherited_from: String,
}

async fn get(db: &impl ConnectionTrait, id: i32) -> DomainResult<maintenance_schedule_item::Model> {
    maintenance_schedule_item::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Schedule item {id} not found")))
}

/// Refuses an interval of zero or less. A reminder adds the interval to the
/// last service, so such an item would read as due from the day it was
/// serviced.
fn validate_intervals(miles: Option<i32>, months: Option<i32>) -> DomainResult<()> {
    for (field, value) in [("interval_miles", miles), ("interval_months", months)] {
        if value.is_some_and(|v| v <= 0) {
            return Err(DomainError::invalid(
                field,
                "must be greater than zero; leave it out for no interval",
            ));
        }
    }
    Ok(())
}

/// Refuses a warning below zero. Zero warns only once the item is due.
fn validate_warnings(miles: Option<i32>, days: Option<i32>) -> DomainResult<()> {
    for (field, value) in [("warning_miles", miles), ("warning_days", days)] {
        if value.is_some_and(|v| v < 0) {
            return Err(DomainError::invalid(field, "must be zero or more"));
        }
    }
    Ok(())
}

pub async fn create(
    db: &impl ConnectionTrait,
    input: NewScheduleItem,
) -> DomainResult<maintenance_schedule_item::Model> {
    validate_intervals(input.interval_miles, input.interval_months)?;
    validate_warnings(input.warning_miles, input.warning_days)?;
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
    if input.overrides_item_id.is_some() && input.vehicle_id.is_none() {
        return Err(DomainError::invalid(
            "overrides_item_id",
            "only a vehicle's own item overrides an inherited one",
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
        warning_miles: Set(Some(input.warning_miles.unwrap_or(DEFAULT_WARNING_MILES))),
        warning_days: Set(Some(input.warning_days.unwrap_or(DEFAULT_WARNING_DAYS))),
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

pub(crate) async fn update(
    db: &impl ConnectionTrait,
    id: i32,
    input: UpdateScheduleItem,
) -> DomainResult<maintenance_schedule_item::Model> {
    validate_intervals(
        input.interval_miles.flatten(),
        input.interval_months.flatten(),
    )?;
    validate_warnings(input.warning_miles.flatten(), input.warning_days.flatten())?;
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
        active.warning_miles = Set(Some(v.unwrap_or(DEFAULT_WARNING_MILES)));
    }
    if let Some(v) = input.warning_days {
        active.warning_days = Set(Some(v.unwrap_or(DEFAULT_WARNING_DAYS)));
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

    active.updated_at = Set(super::now());
    Ok(active.update(db).await?)
}

/// Creates a schedule item owned by the vehicle. `input.vehicle_id` is
/// overwritten with `vehicle_id` and the other owner fields are cleared, so
/// the item can only ever belong to this vehicle.
///
/// An item named like one the vehicle inherits replaces it for this vehicle:
/// it takes the inherited item's spelling of the name and, for every field
/// the caller leaves out, the inherited item's value. The vehicle's services
/// and planned work linked to the inherited item count toward the new one.
/// A name the vehicle already has its own item for is a `BadRequest`, and an
/// interval of zero or less or a warning below zero is `Invalid`.
pub async fn create_for_vehicle<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    mut input: NewScheduleItem,
) -> DomainResult<maintenance_schedule_item::Model> {
    let v = crate::services::vehicle::require(db, vehicle_id).await?;
    if input.name.trim().is_empty() {
        return Err(DomainError::invalid("name", "must not be blank"));
    }
    let txn = super::begin_write(db).await?;
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
    if let Some(inherited) = effective_inherited(&shadowed) {
        fill_from_inherited(&mut input, inherited);
    }
    let created = create(&txn, input).await?;
    drop_links_the_vehicles_jobs_bring(&txn, vehicle_id).await?;
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
/// inherited item count toward the copy, so its history and plans stay.
/// The copy keeps the inherited item's name; a rename is a `BadRequest`. A
/// vehicle-owned item renamed onto an inherited item's name replaces that
/// item for this vehicle, as [`create_for_vehicle`] does, and then keeps the
/// name too.
///
/// An interval of zero or less or a warning below zero is `Invalid`. An id that is not visible to
/// this vehicle reads as nonexistent.
pub async fn update_for_vehicle<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    schedule_item_id: i32,
    mut input: UpdateScheduleItem,
) -> DomainResult<maintenance_schedule_item::Model> {
    if input.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
        return Err(DomainError::invalid("name", "must not be blank"));
    }
    let txn = super::begin_write(db).await?;
    let item = require_in_vehicle_scope(&txn, vehicle_id, schedule_item_id).await?;
    let v = crate::services::vehicle::require(&txn, vehicle_id).await?;

    let owned = item.vehicle_id == Some(vehicle_id);
    let is_override = !owned || item.overrides_item_id.is_some();
    // A name with the same key as the item's own is not a rename.
    if input
        .name
        .as_deref()
        .is_some_and(|n| name_key(n) == name_key(&item.name))
    {
        if is_override {
            input.name = None;
        }
    } else if let Some(name) = input.name.as_deref() {
        if is_override {
            return Err(DomainError::BadRequest(format!(
                "Schedule item {schedule_item_id} follows the '{}' item from the platform or \
                 model template, so it keeps that name. Add a differently named job with \
                 add_schedule_item.",
                item.name
            )));
        }
        if let Some(other) = own_item_named(&txn, vehicle_id, name).await? {
            return Err(DomainError::BadRequest(format!(
                "This vehicle already has a schedule item named '{}' (id {}).",
                other.name, other.id
            )));
        }
    }

    let target = if owned {
        item
    } else if let Some(existing) = own_item_named(&txn, vehicle_id, &item.name).await? {
        existing
    } else {
        copy_for_vehicle(&txn, &item, vehicle_id).await?
    };
    let updated = update(&txn, target.id, input).await?;
    let shadowed = inherited_named(&txn, &v, &updated.name).await?;
    // An item renamed onto an inherited item's name stands in for it, as one
    // created under that name does, so it is tied to it and keeps the name.
    let updated = match effective_inherited(&shadowed) {
        Some(inherited) if updated.overrides_item_id.is_none() => {
            let mut active: maintenance_schedule_item::ActiveModel = updated.into();
            active.overrides_item_id = Set(Some(inherited.id));
            active.update(&txn).await?
        }
        _ => updated,
    };
    drop_links_the_vehicles_jobs_bring(&txn, vehicle_id).await?;
    txn.commit().await?;
    Ok(updated)
}

/// The vehicle-owned item called `name` (see [`name_key`]), if the vehicle
/// has one. The lowest id wins when several match.
async fn own_item_named(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    name: &str,
) -> DomainResult<Option<maintenance_schedule_item::Model>> {
    let key = name_key(name);
    Ok(maintenance_schedule_item::Entity::find()
        .filter(maintenance_schedule_item::Column::VehicleId.eq(vehicle_id))
        .order_by_asc(maintenance_schedule_item::Column::Id)
        .all(db)
        .await?
        .into_iter()
        .find(|i| name_key(&i.name) == key))
}

/// The platform and model-template items called `name` (see [`name_key`])
/// that the vehicle inherits, whether or not one of them shadows another.
async fn inherited_named(
    db: &impl ConnectionTrait,
    v: &vehicle::Model,
    name: &str,
) -> DomainResult<Vec<maintenance_schedule_item::Model>> {
    let Some(owner) = inherited_owner(db, v).await? else {
        return Ok(Vec::new());
    };
    let key = name_key(name);
    Ok(maintenance_schedule_item::Entity::find()
        .filter(owner)
        .all(db)
        .await?
        .into_iter()
        .filter(|i| name_key(&i.name) == key)
        .collect())
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

/// Fills every field of `input` that the caller left empty from the inherited
/// item, and takes the inherited item's spelling of the name. The copy covers
/// the description, intervals, warning window, source, notes, factory flag,
/// labor categories and estimated cost. `enabled` is not copied.
fn fill_from_inherited(input: &mut NewScheduleItem, item: &maintenance_schedule_item::Model) {
    item.name.clone_into(&mut input.name);
    input.description = input
        .description
        .take()
        .or_else(|| item.description.clone());
    input.interval_miles = input.interval_miles.or(item.interval_miles);
    input.interval_months = input.interval_months.or(item.interval_months);
    input.warning_miles = input.warning_miles.or(item.warning_miles);
    input.warning_days = input.warning_days.or(item.warning_days);
    input.source = input.source.take().or_else(|| item.source.clone());
    input.notes = input.notes.take().or_else(|| item.notes.clone());
    input.is_factory_recommended = input.is_factory_recommended.or(item.is_factory_recommended);
    input.labor_categories = input
        .labor_categories
        .take()
        .or_else(|| item.labor_categories.clone());
    input.est_cost_cents = input.est_cost_cents.or(item.est_cost_cents);
}

/// Stores a vehicle-owned copy of an inherited item's full definition,
/// pointing back at it through `overrides_item_id`.
async fn copy_for_vehicle(
    db: &impl ConnectionTrait,
    item: &maintenance_schedule_item::Model,
    vehicle_id: i32,
) -> DomainResult<maintenance_schedule_item::Model> {
    let mut copy = NewScheduleItem {
        vehicle_id: Some(vehicle_id),
        overrides_item_id: Some(item.id),
        enabled: Some(item.enabled),
        ..Default::default()
    };
    fill_from_inherited(&mut copy, item);
    create(db, copy).await
}

crate::vocab::vocabulary! {
    /// How a service's schedule links combine with the items given.
    // `add` unions them with the links already there, so a reconciliation
    // loop clobbers nothing; `replace` makes them the record's only links.
    pub enum LinkMode {
        Add => "add",
        Replace => "replace",
    }
}

/// The schedule item each of `item_ids` counts toward for the vehicle: the
/// item [`resolve_for_listing`] shows under the same name (see [`name_key`]),
/// dismissed ones included, or the id itself when the vehicle has no item of
/// that name. An id of no row maps to itself.
// A stored link or a job names a row, and the row a vehicle tracks under a
// name changes when the vehicle gets its own copy of an inherited item or
// moves to another model template. Reading through the name keeps the
// history and the plans on whichever row the vehicle tracks.
pub(crate) async fn effective_ids(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    item_ids: &[i32],
) -> DomainResult<HashMap<i32, i32>> {
    let mut out: HashMap<i32, i32> = item_ids.iter().map(|id| (*id, *id)).collect();
    if item_ids.is_empty() {
        return Ok(out);
    }
    let named: Vec<(i32, String, Option<i32>)> = maintenance_schedule_item::Entity::find()
        .select_only()
        .column(maintenance_schedule_item::Column::Id)
        .column(maintenance_schedule_item::Column::Name)
        .column(maintenance_schedule_item::Column::VehicleId)
        .filter(maintenance_schedule_item::Column::Id.is_in(item_ids.to_vec()))
        .into_tuple()
        .all(db)
        .await?;
    // The vehicle's own row is the one it tracks, so the schedule is
    // resolved only when an id names some other row.
    let others: Vec<(i32, String)> = named
        .into_iter()
        .filter(|(_, _, owner)| *owner != Some(vehicle_id))
        .map(|(id, name, _)| (id, name))
        .collect();
    if others.is_empty() {
        return Ok(out);
    }
    let tracked: HashMap<String, i32> = resolve_for_listing(db, vehicle_id, true)
        .await?
        .into_iter()
        .map(|r| (name_key(&r.effective_item.name), r.effective_item.id))
        .collect();
    for (id, name) in others {
        if let Some(effective) = tracked.get(&name_key(&name)) {
            out.insert(id, *effective);
        }
    }
    Ok(out)
}

// The item each `(service, item)` pair counts toward for the service's
// vehicle, in the order of `pairs`.
async fn effective_items(
    db: &impl ConnectionTrait,
    pairs: &[(i32, i32)],
) -> DomainResult<Vec<i32>> {
    if pairs.is_empty() {
        return Ok(Vec::new());
    }
    let service_ids: Vec<i32> = pairs.iter().map(|(service_id, _)| *service_id).collect();
    let vehicle_of: HashMap<i32, i32> = service_record::Entity::find()
        .select_only()
        .column(service_record::Column::Id)
        .column(service_record::Column::VehicleId)
        .filter(service_record::Column::Id.is_in(service_ids))
        .into_tuple::<(i32, i32)>()
        .all(db)
        .await?
        .into_iter()
        .collect();
    let mut by_vehicle: HashMap<i32, Vec<i32>> = HashMap::new();
    for (service_id, item_id) in pairs {
        if let Some(vehicle_id) = vehicle_of.get(service_id) {
            by_vehicle.entry(*vehicle_id).or_default().push(*item_id);
        }
    }
    let mut maps: HashMap<i32, HashMap<i32, i32>> = HashMap::new();
    for (vehicle_id, item_ids) in by_vehicle {
        maps.insert(vehicle_id, effective_ids(db, vehicle_id, &item_ids).await?);
    }
    Ok(pairs
        .iter()
        .map(|(service_id, item_id)| {
            vehicle_of
                .get(service_id)
                .and_then(|vehicle_id| maps.get(vehicle_id))
                .and_then(|map| map.get(item_id))
                .copied()
                .unwrap_or(*item_id)
        })
        .collect())
}

// Rewrites the item of each `(service, item)` pair to the item it counts
// toward, then sorts the pairs and drops repeats.
async fn effective_pairs(
    db: &impl ConnectionTrait,
    pairs: Vec<(i32, i32)>,
) -> DomainResult<Vec<(i32, i32)>> {
    let items = effective_items(db, &pairs).await?;
    let mut out: Vec<(i32, i32)> = pairs
        .into_iter()
        .zip(items)
        .map(|((service_id, _), item_id)| (service_id, item_id))
        .collect();
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

/// Each pair of a service in `service_ids` and a schedule item it satisfies,
/// ordered by service and then by item. Each item is the one the service's
/// vehicle tracks under that name (see [`effective_ids`]).
pub(crate) async fn links_of_services(
    db: &impl ConnectionTrait,
    service_ids: &[i32],
) -> DomainResult<Vec<(i32, i32)>> {
    if service_ids.is_empty() {
        return Ok(Vec::new());
    }
    let stored = service_schedule_link::Entity::find()
        .filter(service_schedule_link::Column::ServiceRecordId.is_in(service_ids.to_vec()));
    let jobs = work_item::Entity::find()
        .filter(work_item::Column::CompletedServiceId.is_in(service_ids.to_vec()));
    effective_pairs(db, satisfied(db, stored, jobs).await?).await
}

/// Each pair of one of the vehicle's services and a schedule item in
/// `item_ids` that the service satisfies, ordered by service and then by
/// item. A link or a job that names another row of the same name counts
/// toward the item the vehicle tracks (see [`effective_ids`]).
pub(crate) async fn links_to_items(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    item_ids: &[i32],
) -> DomainResult<Vec<(i32, i32)>> {
    if item_ids.is_empty() {
        return Ok(Vec::new());
    }
    // A link may name a row the vehicle tracked before it got its own copy
    // or changed its template, so every link of the vehicle is read and
    // judged by the item it counts toward.
    let own_services = service_record::Entity::find()
        .select_only()
        .column(service_record::Column::Id)
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .into_query();
    let stored = service_schedule_link::Entity::find()
        .filter(service_schedule_link::Column::ServiceRecordId.in_subquery(own_services));
    let jobs = work_item::Entity::find().filter(work_item::Column::VehicleId.eq(vehicle_id));
    let pairs = satisfied(db, stored, jobs).await?;
    let seen: Vec<i32> = pairs.iter().map(|(_, item_id)| *item_id).collect();
    let effective = effective_ids(db, vehicle_id, &seen).await?;
    let mut out: Vec<(i32, i32)> = pairs
        .into_iter()
        .map(|(service_id, item_id)| (service_id, effective[&item_id]))
        .filter(|(_, item_id)| item_ids.contains(item_id))
        .collect();
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

// A service satisfies the schedule items its stored links name and the
// schedule item of each job it completed. A job's item is read from the job
// and never stored as a link, so taking the job off the service takes the
// item's baseline with it and leaves the other items the invoice named. The
// pairs name rows as stored.
async fn satisfied(
    db: &impl ConnectionTrait,
    stored: Select<service_schedule_link::Entity>,
    jobs: Select<work_item::Entity>,
) -> DomainResult<Vec<(i32, i32)>> {
    let mut pairs: Vec<(i32, i32)> = stored
        .all(db)
        .await?
        .into_iter()
        .map(|l| (l.service_record_id, l.schedule_item_id))
        .collect();
    let completed: Vec<(i32, i32)> = jobs
        .select_only()
        .column(work_item::Column::CompletedServiceId)
        .column(work_item::Column::ScheduleItemId)
        .filter(work_item::Column::CompletedServiceId.is_not_null())
        .filter(work_item::Column::ScheduleItemId.is_not_null())
        .into_tuple()
        .all(db)
        .await?;
    pairs.extend(completed);
    pairs.sort_unstable();
    pairs.dedup();
    Ok(pairs)
}

/// Links a service to the schedule items the vehicle tracks for `item_ids`,
/// resolving them with [`tracked_ids`] first. Runs on the caller's
/// connection, so inside its transaction when there is one.
pub(crate) async fn link_service(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    service_record_id: i32,
    item_ids: &[i32],
    mode: LinkMode,
) -> DomainResult<()> {
    let tracked = tracked_ids(db, vehicle_id, item_ids).await?;
    insert_service_links(db, service_record_id, &tracked, mode).await
}

/// Writes a service's schedule links for ids that are already resolved:
/// `Replace` drops the existing links first, `Add` skips the ones present.
/// A caller that resolved the ids through [`tracked_ids`] earlier, or that
/// holds an id a job stores, uses this instead of [`link_service`].
pub(crate) async fn insert_service_links(
    db: &impl ConnectionTrait,
    service_record_id: i32,
    item_ids: &[i32],
    mode: LinkMode,
) -> DomainResult<()> {
    let already: std::collections::HashSet<i32> = match mode {
        LinkMode::Replace => {
            service_schedule_link::Entity::delete_many()
                .filter(service_schedule_link::Column::ServiceRecordId.eq(service_record_id))
                .exec(db)
                .await?;
            std::collections::HashSet::new()
        }
        // A link to the inherited row already counts toward the vehicle's
        // copy, so the links are compared by the items they count toward.
        LinkMode::Add => links_of_services(db, &[service_record_id])
            .await?
            .into_iter()
            .map(|(_, item_id)| item_id)
            .collect(),
    };
    let new_links = super::dedup_ids(item_ids)
        .into_iter()
        .filter(|id| !already.contains(id))
        .map(|schedule_item_id| service_schedule_link::ActiveModel {
            service_record_id: Set(service_record_id),
            schedule_item_id: Set(schedule_item_id),
        });
    service_schedule_link::Entity::insert_many(new_links)
        .exec_without_returning(db)
        .await?;
    drop_links_jobs_bring(db, &[service_record_id]).await
}

/// Removes the stored links of the named services that name the schedule item
/// of a job the same service completes. Runs on the caller's connection.
// A job's schedule item is read from the job. A stored link for the same item
// would outlive the job when a correction takes the job off, and a service
// read lists the job's item, so a caller that sends the list back would store
// one. A write that stores a link, completes a job, or changes the schedule
// item of a completed job ends here.
pub(crate) async fn drop_links_jobs_bring(
    db: &impl ConnectionTrait,
    service_ids: &[i32],
) -> DomainResult<()> {
    if service_ids.is_empty() {
        return Ok(());
    }
    let from_jobs: Vec<(i32, i32)> = work_item::Entity::find()
        .select_only()
        .column(work_item::Column::CompletedServiceId)
        .column(work_item::Column::ScheduleItemId)
        .filter(work_item::Column::CompletedServiceId.is_in(service_ids.to_vec()))
        .filter(work_item::Column::ScheduleItemId.is_not_null())
        .into_tuple()
        .all(db)
        .await?;
    if from_jobs.is_empty() {
        return Ok(());
    }
    let stored: Vec<(i32, i32)> = service_schedule_link::Entity::find()
        .filter(service_schedule_link::Column::ServiceRecordId.is_in(service_ids.to_vec()))
        .all(db)
        .await?
        .into_iter()
        .map(|l| (l.service_record_id, l.schedule_item_id))
        .collect();
    // A link repeats a job's item when both count toward the same item, even
    // when one names the inherited row and the other the vehicle's copy.
    let both: Vec<(i32, i32)> = stored.iter().chain(&from_jobs).copied().collect();
    let mut counted = effective_items(db, &both).await?;
    let from_jobs: Vec<(i32, i32)> = from_jobs
        .iter()
        .zip(counted.split_off(stored.len()))
        .map(|((service_id, _), item_id)| (*service_id, item_id))
        .collect();
    for ((service_id, item_id), counted) in stored.into_iter().zip(counted) {
        if from_jobs.contains(&(service_id, counted)) {
            service_schedule_link::Entity::delete_by_id((service_id, item_id))
                .exec(db)
                .await?;
        }
    }
    Ok(())
}

/// Runs [`drop_links_jobs_bring`] for every service that completed one of
/// the vehicle's jobs.
// A change to which row the vehicle tracks under a name can make a stored
// link and a completed job of one service count toward the same item, so the
// writes that make one end here: a new or renamed item, and a change of the
// vehicle's model template.
pub(crate) async fn drop_links_the_vehicles_jobs_bring(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<()> {
    let completing: Vec<i32> = work_item::Entity::find()
        .select_only()
        .column(work_item::Column::CompletedServiceId)
        .filter(work_item::Column::VehicleId.eq(vehicle_id))
        .filter(work_item::Column::ScheduleItemId.is_not_null())
        .filter(work_item::Column::CompletedServiceId.is_not_null())
        .into_tuple()
        .all(db)
        .await?;
    drop_links_jobs_bring(db, &super::dedup_ids(&completing)).await
}

/// Checks that every id is in the vehicle's schedule scope and maps each one
/// to the item the vehicle tracks under that name (see [`effective_ids`]):
/// the vehicle's own item, else the model template's, else the platform's.
/// Duplicates are dropped, first occurrence kept. An id outside the scope
/// reads as nonexistent.
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

    let tracked = effective_ids(db, vehicle_id, ids).await?;
    let mapped: Vec<i32> = ids.iter().map(|id| tracked[id]).collect();
    Ok(super::dedup_ids(&mapped))
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
// Each layer is read in descending id order, so when a name repeats the
// lowest id is the one kept, as `own_item_named` picks.
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
                    .order_by_desc(maintenance_schedule_item::Column::Id)
                    .all(db)
                    .await?;

                for item in platform_items {
                    schedule.insert(
                        name_key(&item.name),
                        ResolvedScheduleItem {
                            effective_item: item,
                            inherited_from: Some("platform".to_string()),
                            overrides: None,
                        },
                    );
                }
            }

            // Layer 2: Model template items override platform items by name
            let template_items = maintenance_schedule_item::Entity::find()
                .filter(maintenance_schedule_item::Column::ModelTemplateId.eq(mt_id))
                .order_by_desc(maintenance_schedule_item::Column::Id)
                .all(db)
                .await?;

            for item in template_items {
                schedule.insert(
                    name_key(&item.name),
                    ResolvedScheduleItem {
                        effective_item: item,
                        inherited_from: Some("model_template".to_string()),
                        overrides: None,
                    },
                );
            }
        }
    }

    // Layer 3: Vehicle-level items override everything
    let vehicle_items = maintenance_schedule_item::Entity::find()
        .filter(maintenance_schedule_item::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(maintenance_schedule_item::Column::Id)
        .all(db)
        .await?;

    for item in vehicle_items {
        let key = name_key(&item.name);
        let replaced = schedule.insert(
            key.clone(),
            ResolvedScheduleItem {
                overrides: None,
                effective_item: item,
                inherited_from: None,
            },
        );
        // A vehicle item shadowing an earlier vehicle item keeps what that one
        // replaced.
        let overrides = replaced.and_then(|r| match r.inherited_from {
            Some(inherited_from) => Some(OverriddenItem {
                id: r.effective_item.id,
                inherited_from,
            }),
            None => r.overrides,
        });
        if let Some(entry) = schedule.get_mut(&key) {
            entry.overrides = overrides;
        }
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
/// planned work linked to the inherited item count toward the copy. Returns the
/// disabled row; its id is what [`undismiss_for_vehicle`] takes. A `reason`
/// is appended to the row's notes.
pub async fn dismiss_for_vehicle<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    schedule_item_id: i32,
    reason: Option<String>,
) -> DomainResult<maintenance_schedule_item::Model> {
    let txn = super::begin_write(db).await?;
    let item = require_in_vehicle_scope(&txn, vehicle_id, schedule_item_id).await?;

    // Re-dismissing an inherited item reuses the vehicle's existing copy
    // rather than stacking a second one.
    let target = if item.vehicle_id == Some(vehicle_id) {
        item
    } else if let Some(existing) = own_item_named(&txn, vehicle_id, &item.name).await? {
        existing
    } else {
        copy_for_vehicle(&txn, &item, vehicle_id).await?
    };

    let notes = append_dismiss_reason(target.notes.clone(), reason.as_deref());
    let mut active: maintenance_schedule_item::ActiveModel = target.into();
    active.enabled = Set(false);
    active.notes = Set(notes);
    active.updated_at = Set(super::now());
    let dismissed = active.update(&txn).await?;

    txn.commit().await?;
    Ok(dismissed)
}

/// Reverses a dismissal by re-enabling the vehicle-owned row that
/// [`dismiss_for_vehicle`] returned. A restored copy of an inherited item
/// keeps the definition it had when it was copied and does not follow
/// later changes to the template.
///
/// Rows not owned by this vehicle are indistinguishable from nonexistent.
pub async fn undismiss_for_vehicle<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    schedule_item_id: i32,
) -> DomainResult<maintenance_schedule_item::Model> {
    let txn = super::begin_write(db).await?;
    let db = &txn;
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
    active.updated_at = Set(super::now());
    let out = active.update(db).await?;
    txn.commit().await?;
    Ok(out)
}

#[cfg(test)]
mod tests;
