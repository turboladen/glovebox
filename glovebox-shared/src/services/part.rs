use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use sea_orm::{sea_query::Expr, *};
use serde::Serialize;

use crate::{
    entities::{
        job_need,
        part::{self, PartStatus, Supplier},
        part_content, service_record,
    },
    error::{DomainError, DomainResult},
    inputs::part::{
        Fitting, Install, InstallPatch, NewFitting, NewPart, NewPartContent, PartFilter, UpdatePart,
    },
    services::service_record::CompletedWork,
};

/// A part's manufacturer and number joined by a space, such as
/// `ATE 13.0460-2788.2`, or `None` without a number. Blank text counts as
/// absent.
pub fn label(manufacturer: Option<&str>, part_number: Option<&str>) -> Option<String> {
    fn nonblank(s: Option<&str>) -> Option<&str> {
        s.map(str::trim).filter(|s| !s.is_empty())
    }
    let number = nonblank(part_number)?;
    Some(match nonblank(manufacturer) {
        Some(m) => format!("{m} {number}"),
        None => number.to_owned(),
    })
}

/// Whether `p` was sent back from its need.
pub(crate) fn is_returned(p: &part::Model) -> bool {
    p.status == PartStatus::Returned
}

/// Whether `p` has been on the car: it is installed, or it was installed and
/// later replaced.
pub(crate) fn was_fitted(p: &part::Model) -> bool {
    matches!(p.status, PartStatus::Installed | PartStatus::Replaced)
}

/// Marks `p` returned today for `reason`, remembering the need it was
/// returned from.
pub(crate) async fn mark_returned(
    db: &impl ConnectionTrait,
    p: part::Model,
    reason: String,
    need_id: i32,
) -> DomainResult<part::Model> {
    let mut active: part::ActiveModel = p.into();
    PartState::Returned {
        on: super::today(),
        reason,
        need_id: Some(need_id),
    }
    .write(&mut active);
    active.updated_at = Set(super::now());
    Ok(active.update(db).await?)
}

/// Where a part stands, with the details its status takes.
// Every write of the status, install, replacement and return columns of
// `parts` goes through `PartState::write`, except two bulk writes that patch
// the install details of fitted parts (`follow_service` and the done-another-way
// release) and the `ON DELETE SET NULL` keys, which the CHECK constraints keep
// inside a state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PartState {
    Purchased,
    Installed(Install),
    Replaced {
        install: Install,
        on: Option<NaiveDate>,
        odometer: Option<i32>,
    },
    /// `need_id` is cleared when the need is deleted.
    Returned {
        on: NaiveDate,
        reason: String,
        need_id: Option<i32>,
    },
}

impl PartState {
    /// The state `p` holds.
    ///
    /// # Errors
    /// `Internal`, naming the part, for a row whose columns no state holds.
    pub(crate) fn of(p: &part::Model) -> DomainResult<Self> {
        let install = Install {
            service_id: p.installed_service_id,
            date: p.installed_date,
            odometer: p.installed_odometer,
        };
        let replaced = p.replaced_date.is_some() || p.replaced_odometer.is_some();
        let returned = p.returned_on.is_some()
            || p.return_reason.is_some()
            || p.returned_from_need_id.is_some();
        let state = match p.status {
            PartStatus::Purchased if install.is_empty() && !replaced && !returned => {
                Some(Self::Purchased)
            }
            PartStatus::Installed if !replaced && !returned => Some(Self::Installed(install)),
            PartStatus::Replaced if !returned => Some(Self::Replaced {
                install,
                on: p.replaced_date,
                odometer: p.replaced_odometer,
            }),
            PartStatus::Returned if install.is_empty() && !replaced => {
                match (p.returned_on, &p.return_reason) {
                    (Some(on), Some(reason)) => Some(Self::Returned {
                        on,
                        reason: reason.clone(),
                        need_id: p.returned_from_need_id,
                    }),
                    _ => None,
                }
            }
            _ => None,
        };
        state.ok_or_else(|| {
            DomainError::Internal(format!(
                "part {} is {} but its install, replacement or return details do not fit that \
                 status",
                p.id, p.status
            ))
        })
    }

    /// The state a new part starts in.
    fn new(fitting: NewFitting) -> Self {
        match fitting {
            NewFitting::Purchased => Self::Purchased,
            NewFitting::Installed(install) => Self::Installed(install),
            NewFitting::Replaced(install) => Self::Replaced {
                install,
                on: None,
                odometer: None,
            },
        }
    }

    /// The install details of a fitted part, and `None` for one off the car.
    fn install(&self) -> Option<Install> {
        match self {
            Self::Installed(install) | Self::Replaced { install, .. } => Some(*install),
            Self::Purchased | Self::Returned { .. } => None,
        }
    }

    fn status(&self) -> PartStatus {
        match self {
            Self::Purchased => PartStatus::Purchased,
            Self::Installed(_) => PartStatus::Installed,
            Self::Replaced { .. } => PartStatus::Replaced,
            Self::Returned { .. } => PartStatus::Returned,
        }
    }

    /// Sets the status and every install, replacement and return column of
    /// `active` from this state.
    pub(crate) fn write(self, active: &mut part::ActiveModel) {
        active.status = Set(self.status());
        let (install, (replaced_on, replaced_odometer), (returned_on, reason, need_id)) = match self
        {
            Self::Purchased => (Install::default(), (None, None), (None, None, None)),
            Self::Installed(install) => (install, (None, None), (None, None, None)),
            Self::Replaced {
                install,
                on,
                odometer,
            } => (install, (on, odometer), (None, None, None)),
            Self::Returned {
                on,
                reason,
                need_id,
            } => (
                Install::default(),
                (None, None),
                (Some(on), Some(reason), need_id),
            ),
        };
        active.installed_service_id = Set(install.service_id);
        active.installed_date = Set(install.date);
        active.installed_odometer = Set(install.odometer);
        active.replaced_date = Set(replaced_on);
        active.replaced_odometer = Set(replaced_odometer);
        active.returned_on = Set(returned_on);
        active.return_reason = Set(reason);
        active.returned_from_need_id = Set(need_id);
    }

    /// The state `change` moves `existing`, a part in this state, to. A part
    /// that comes off the car also returns the date it reads as received.
    ///
    /// `supplied_by` and `purchase_date` are the values the part has once
    /// the same correction is applied, and `received_on` is the arrival date
    /// it sends.
    fn apply(
        self,
        change: Fitting,
        existing: &part::Model,
        supplied_by: Supplier,
        purchase_date: Option<NaiveDate>,
        received_on: Option<Option<NaiveDate>>,
    ) -> DomainResult<(Self, Option<NaiveDate>)> {
        let id = existing.id;
        let refused = |message: String| Err(DomainError::BadRequest(message));
        // A fitted part's install details are the base a change patches.
        let base = self.install().unwrap_or_default();
        let next = match (self, change) {
            (Self::Returned { .. } | Self::Purchased, Fitting::Purchased) => Self::Purchased,
            (Self::Returned { .. }, Fitting::Installed(..) | Fitting::Replaced(..)) => {
                return refused(format!(
                    "Part {id} was returned; its status can only change back to purchased"
                ));
            }
            (returned @ Self::Returned { .. }, Fitting::Keep(install, replace)) => {
                if install.sets_any() || replace.sets_any() {
                    return refused(format!(
                        "Part {id} was returned and cannot be given install details"
                    ));
                }
                returned
            }
            (Self::Purchased, Fitting::Keep(install, replace)) => {
                if install.sets_any() || replace.sets_any() {
                    return refused(format!(
                        "Part {id} is purchased, so it takes no install or replacement details; \
                         set its status to installed or replaced with them"
                    ));
                }
                Self::Purchased
            }
            (Self::Installed(_), Fitting::Keep(_, replace)) if replace.sets_any() => {
                return refused(format!(
                    "Part {id} is installed, so it takes no replacement details; set its status \
                     to replaced with them"
                ));
            }
            (
                Self::Purchased | Self::Installed(_),
                Fitting::Installed(install, _) | Fitting::Keep(install, _),
            ) => Self::Installed(base.patched(install)),
            (Self::Purchased | Self::Installed(_), Fitting::Replaced(install, replace)) => {
                Self::Replaced {
                    install: base.patched(install),
                    on: replace.date.flatten(),
                    odometer: replace.odometer.flatten(),
                }
            }
            (
                Self::Replaced { on, odometer, .. },
                Fitting::Keep(install, replace) | Fitting::Replaced(install, replace),
            ) => Self::Replaced {
                install: base.patched(install),
                on: replace.date.unwrap_or(on),
                odometer: replace.odometer.unwrap_or(odometer),
            },
            (Self::Replaced { on, odometer, .. }, Fitting::Installed(install, cleared)) => {
                // Going back to installed says the part was never replaced, so
                // the details of the replacement must be cleared with it.
                // `cleared` holds no value but `Some(None)`.
                if cleared.date.unwrap_or(on).is_some()
                    || cleared.odometer.unwrap_or(odometer).is_some()
                {
                    return refused(format!(
                        "Part {id} was replaced; clear its replacement date and odometer in the \
                         same change to set it back to installed"
                    ));
                }
                Self::Installed(base.patched(install))
            }
            (Self::Installed(_) | Self::Replaced { .. }, Fitting::Purchased) => {
                let received = coming_off(existing, supplied_by, purchase_date, received_on)?;
                return Ok((Self::Purchased, Some(received)));
            }
        };
        Ok((next, None))
    }
}

// A part set back to `purchased` from the car is one the owner holds: a part
// the shop fitted never was, so saying it was not fitted is a correction to
// the service or a deleted part.
//
// Returns the date the part reads as received.
fn coming_off(
    existing: &part::Model,
    supplied_by: Supplier,
    purchase_date: Option<NaiveDate>,
    received_on: Option<Option<NaiveDate>>,
) -> DomainResult<NaiveDate> {
    if supplied_by == Supplier::Shop {
        // Dropping a replaced part from its service leaves it replaced, so
        // only deleting it applies.
        let instead = if existing.status == PartStatus::Installed {
            "correct the service that installed it or delete the part"
        } else {
            "delete the part"
        };
        return Err(DomainError::BadRequest(format!(
            "Part {} was supplied by the shop, so it cannot go back to purchased. To record that \
             it was not fitted, {instead}",
            existing.id
        )));
    }
    if received_on == Some(None) {
        return Err(DomainError::BadRequest(format!(
            "Part {} goes back to purchased in the owner's hands, so its received date cannot be \
             cleared",
            existing.id
        )));
    }
    // A part is never received before it was bought.
    let not_before_purchase = |d: NaiveDate| Ord::max(d, purchase_date.unwrap_or(d));
    Ok(existing
        .received_on
        .or(existing.installed_date.map(not_before_purchase))
        .unwrap_or_else(|| not_before_purchase(super::today())))
}

/// Sets the date the part arrived to `on`.
///
/// # Errors
/// `Invalid` for a date before `purchase_date`, the date the part has once
/// the caller's write is applied.
pub(crate) fn set_received(
    active: &mut part::ActiveModel,
    on: Option<NaiveDate>,
    purchase_date: Option<NaiveDate>,
) -> DomainResult<()> {
    if let (Some(received), Some(bought)) = (on, purchase_date)
        && received < bought
    {
        return Err(DomainError::invalid(
            "received_on",
            format!(
                "is before the purchase date {bought}; correct the purchase date first if it is \
                 wrong"
            ),
        ));
    }
    active.received_on = Set(on);
    Ok(())
}

/// A part with the items inside it, when it was bought as a kit.
#[derive(Debug, Serialize)]
pub struct PartView {
    #[serde(flatten)]
    pub part: part::Model,
    /// The kit's items in print order; empty for a part that is not a kit.
    pub contents: Vec<part_content::Model>,
}

/// The vehicle's parts, newest first, each with its kit contents.
pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    filter: PartFilter,
) -> DomainResult<Vec<PartView>> {
    super::vehicle::require(db, vehicle_id).await?;
    let mut query = part::Entity::find().filter(part::Column::VehicleId.eq(vehicle_id));

    if let Some(status) = filter.status {
        query = query.filter(part::Column::Status.eq(status));
    }

    let parts = query.order_by_desc(part::Column::CreatedAt).all(db).await?;
    views(db, parts).await
}

/// Each part with its kit contents, in the order given, from one query.
pub(crate) async fn views(
    db: &impl ConnectionTrait,
    parts: Vec<part::Model>,
) -> DomainResult<Vec<PartView>> {
    let mut contents = contents_by_part(db, parts.iter().map(|p| p.id)).await?;
    Ok(parts
        .into_iter()
        .map(|part| PartView {
            contents: contents.remove(&part.id).unwrap_or_default(),
            part,
        })
        .collect())
}

/// One part with its kit contents.
pub(crate) async fn view(db: &impl ConnectionTrait, part: part::Model) -> DomainResult<PartView> {
    let contents = contents_by_part(db, [part.id])
        .await?
        .remove(&part.id)
        .unwrap_or_default();
    Ok(PartView { part, contents })
}

/// The kit contents of each named part, in print order. A part with no
/// contents has no entry.
pub(crate) async fn contents_by_part(
    db: &impl ConnectionTrait,
    part_ids: impl IntoIterator<Item = i32>,
) -> DomainResult<HashMap<i32, Vec<part_content::Model>>> {
    let ids: Vec<i32> = part_ids.into_iter().collect();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = part_content::Entity::find()
        .filter(part_content::Column::PartId.is_in(ids))
        .order_by_asc(part_content::Column::PartId)
        .order_by_asc(part_content::Column::Position)
        .all(db)
        .await?;
    let mut out: HashMap<i32, Vec<part_content::Model>> = HashMap::new();
    for row in rows {
        out.entry(row.part_id).or_default().push(row);
    }
    Ok(out)
}

/// Records a part as [`create`] does and returns it with its kit contents.
pub async fn record<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    input: NewPart,
) -> DomainResult<PartView> {
    let txn = super::begin_write(db).await?;
    let (part, contents) = insert_part(&txn, vehicle_id, input).await?;
    txn.commit().await?;
    Ok(PartView { part, contents })
}

/// Corrects a part as [`update`] does and returns it with its kit contents.
pub async fn correct<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    input: UpdatePart,
) -> DomainResult<PartView> {
    let txn = super::begin_write(db).await?;
    let part = apply_update(&txn, vehicle_id, id, input).await?;
    let out = view(&txn, part).await?;
    txn.commit().await?;
    Ok(out)
}

// The messages count items from 1, as the owner would, and name no argument
// syntax, because they reach the web page as well as a tool result.
pub(crate) fn validate_contents(field: &str, contents: &[NewPartContent]) -> DomainResult<()> {
    for (n, item) in (1..).zip(contents) {
        if item.name.trim().is_empty() {
            return Err(DomainError::invalid(
                field,
                format!("item {n} needs a name"),
            ));
        }
        if item.quantity.is_some_and(|q| q < 1) {
            return Err(DomainError::invalid(
                field,
                format!("item {n}'s quantity must be at least 1"),
            ));
        }
    }
    Ok(())
}

// Kits hold a handful of items, so the rows go in one at a time, which lets
// each insert stamp itself. The items take positions from `first` on.
async fn insert_contents(
    db: &impl ConnectionTrait,
    part_id: i32,
    first: i32,
    contents: Vec<NewPartContent>,
) -> DomainResult<Vec<part_content::Model>> {
    let mut rows = Vec::with_capacity(contents.len());
    for (position, item) in (first..).zip(contents) {
        let row = part_content::ActiveModel {
            part_id: Set(part_id),
            position: Set(position),
            name: Set(item.name.trim().to_owned()),
            manufacturer: Set(item.manufacturer.as_deref().and_then(super::trim_to_none)),
            part_number: Set(item.part_number.as_deref().and_then(super::trim_to_none)),
            quantity: Set(item.quantity.unwrap_or(1)),
            ..Default::default()
        }
        .insert(db)
        .await?;
        rows.push(row);
    }
    Ok(rows)
}

// Removes the named items of the part, then appends the new ones after the
// last remaining item, so the kept items keep their order and the unique
// (part_id, position) index never sees two rows in one place.
async fn edit_contents(
    db: &impl ConnectionTrait,
    part_id: i32,
    remove: &[i32],
    add: Vec<NewPartContent>,
) -> DomainResult<()> {
    if !remove.is_empty() {
        let found: HashSet<i32> = part_content::Entity::find()
            .select_only()
            .column(part_content::Column::Id)
            .filter(part_content::Column::PartId.eq(part_id))
            .filter(part_content::Column::Id.is_in(remove.iter().copied()))
            .into_tuple::<i32>()
            .all(db)
            .await?
            .into_iter()
            .collect();
        // Another part's item reads as a missing one, so an id reveals nothing
        // about parts the caller did not name.
        if let Some(missing) = remove.iter().find(|id| !found.contains(id)) {
            return Err(DomainError::NotFound(format!(
                "Kit item {missing} not found"
            )));
        }
        part_content::Entity::delete_many()
            .filter(part_content::Column::Id.is_in(found))
            .exec(db)
            .await?;
    }
    if !add.is_empty() {
        let last: Option<i32> = part_content::Entity::find()
            .select_only()
            .column_as(part_content::Column::Position.max(), "last")
            .filter(part_content::Column::PartId.eq(part_id))
            .into_tuple::<Option<i32>>()
            .one(db)
            .await?
            .flatten();
        insert_contents(db, part_id, last.map_or(0, |p| p + 1), add).await?;
    }
    Ok(())
}

pub async fn get(db: &impl ConnectionTrait, vehicle_id: i32, id: i32) -> DomainResult<part::Model> {
    part::Entity::find_by_id(id)
        .filter(part::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Part {id} not found")))
}

/// Records a part, with its kit contents, in one transaction.
///
/// # Errors
/// `NotFound` for a vehicle or an installing service that is not the
/// vehicle's; `Invalid` for a kit item with a blank name or a quantity below
/// 1, or a `received_on` before the purchase date.
pub async fn create<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    input: NewPart,
) -> DomainResult<part::Model> {
    Ok(record(db, vehicle_id, input).await?.part)
}

async fn insert_part(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    input: NewPart,
) -> DomainResult<(part::Model, Vec<part_content::Model>)> {
    validate_contents("contents", &input.contents)?;
    super::vehicle::require(db, vehicle_id).await?;
    let supplied_by = input.supplied_by.unwrap_or(Supplier::Owner);
    let state = PartState::new(input.fitting);
    if let Some(Install {
        service_id: Some(service_id),
        ..
    }) = state.install()
    {
        super::service_record::require(db, vehicle_id, service_id).await?;
    }

    let mut model = part::ActiveModel {
        vehicle_id: Set(vehicle_id),
        name: Set(input.name),
        manufacturer: Set(input.manufacturer),
        part_number: Set(input.part_number),
        oe_part_number_replaced: Set(input.oe_part_number_replaced),
        seller: Set(input.seller),
        purchase_date: Set(input.purchase_date),
        cost_cents: Set(input.cost_cents),
        cost_currency: Set(input.cost_currency),
        manufacturer_url: Set(input.manufacturer_url),
        retailer_url: Set(input.retailer_url),
        notes: Set(input.notes),
        location: Set(input.location),
        warranty_expires_on: Set(input.warranty_expires_on),
        warranty_expires_miles: Set(input.warranty_expires_miles),
        supplied_by: Set(supplied_by),
        order_ref: Set(input.order_ref),
        ..Default::default()
    };
    state.write(&mut model);
    let out = model.insert(db).await?;
    let contents = insert_contents(db, out.id, 0, input.contents).await?;
    Ok((out, contents))
}

/// Corrects a part's fields, in one transaction.
///
/// # Errors
/// `NotFound` for a part or an installing service that is not the vehicle's,
/// and for a kit item to remove that is not the part's; `Invalid` for an
/// added kit item with a blank name or a quantity below 1, and for a
/// `received_on` before the purchase date. `BadRequest` when `fitting` does
/// not fit the part: a returned part only goes back to `purchased` and takes
/// no details; a purchased part takes no install or replacement details and
/// an installed one no replacement details while its status stays; a
/// replaced part goes back to `installed` only when the same change clears
/// its replacement date and odometer; and each refusal of the change to
/// `purchased` described below.
///
/// Removed kit items go first, and added ones follow the items that remain.
///
/// Changing an installed or replaced part to `purchased` clears its install
/// and replace details and, when the part has no received date, sets it to
/// the install date, or to today when there is no install date, and never to
/// a day before the purchase date. The change is refused for a part the shop
/// supplied and when it clears the received date.
pub async fn update<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    input: UpdatePart,
) -> DomainResult<part::Model> {
    let txn = super::begin_write(db).await?;
    let part = apply_update(&txn, vehicle_id, id, input).await?;
    txn.commit().await?;
    Ok(part)
}

async fn apply_update(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
    input: UpdatePart,
) -> DomainResult<part::Model> {
    validate_contents("add_contents", &input.add_contents)?;
    let existing = get(db, vehicle_id, id).await?;

    if let Some(service_id) = input.fitting.service_id() {
        super::service_record::require(db, vehicle_id, service_id).await?;
    }

    let purchase_date = input.purchase_date.unwrap_or(existing.purchase_date);
    let (state, shelved_on) = PartState::of(&existing)?.apply(
        input.fitting,
        &existing,
        input.supplied_by.unwrap_or(existing.supplied_by),
        purchase_date,
        input.received_on,
    )?;
    let received_on = input
        .received_on
        .unwrap_or(shelved_on.or(existing.received_on));

    let mut active: part::ActiveModel = existing.into();
    state.write(&mut active);
    set_received(&mut active, received_on, purchase_date)?;

    if let Some(v) = input.supplied_by {
        active.supplied_by = Set(v);
    }
    if let Some(v) = input.name {
        active.name = Set(v);
    }
    if let Some(v) = input.manufacturer {
        active.manufacturer = Set(v);
    }
    if let Some(v) = input.part_number {
        active.part_number = Set(v);
    }
    if let Some(v) = input.oe_part_number_replaced {
        active.oe_part_number_replaced = Set(v);
    }
    if let Some(v) = input.seller {
        active.seller = Set(v);
    }
    if let Some(v) = input.purchase_date {
        active.purchase_date = Set(v);
    }
    if let Some(v) = input.cost_cents {
        active.cost_cents = Set(v);
    }
    if let Some(v) = input.cost_currency {
        active.cost_currency = Set(v);
    }
    if let Some(v) = input.manufacturer_url {
        active.manufacturer_url = Set(v);
    }
    if let Some(v) = input.retailer_url {
        active.retailer_url = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }
    if let Some(v) = input.location {
        active.location = Set(v);
    }
    if let Some(v) = input.warranty_expires_on {
        active.warranty_expires_on = Set(v);
    }
    if let Some(v) = input.warranty_expires_miles {
        active.warranty_expires_miles = Set(v);
    }
    if let Some(v) = input.order_ref {
        active.order_ref = Set(v);
    }

    active.updated_at = Set(super::now());
    let updated = active.update(db).await?;
    edit_contents(db, id, &input.remove_contents, input.add_contents).await?;
    Ok(updated)
}

/// Marks a vehicle's part as installed by `record` on the record's date.
/// `odometer` patches `installed_odometer`, so `None` keeps the reading the
/// part already has. A part another service installed and a later one
/// replaced becomes installed again, with its replacement details cleared; a
/// replaced part this record already installed stays replaced. A part of another vehicle reads as
/// nonexistent, and a `returned` part is refused with `BadRequest`.
pub(crate) async fn install(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    part_id: i32,
    record: &service_record::Model,
    odometer: Option<Option<i32>>,
) -> DomainResult<()> {
    let existing = get(db, vehicle_id, part_id).await?;
    let state = PartState::of(&existing)?;
    if matches!(state, PartState::Returned { .. }) {
        return Err(DomainError::BadRequest(format!(
            "Part {part_id} was returned and cannot be installed"
        )));
    }
    let install = state.install().unwrap_or_default().patched(InstallPatch {
        service_id: Some(Some(record.id)),
        date: Some(Some(record.service_date)),
        odometer,
    });
    let fitted = match state {
        // A later service took off a part this record put on, so the
        // replacement stands.
        PartState::Replaced {
            install: before,
            on,
            odometer,
        } if before.service_id == Some(record.id) => PartState::Replaced {
            install,
            on,
            odometer,
        },
        _ => PartState::Installed(install),
    };
    let mut active: part::ActiveModel = existing.into();
    fitted.write(&mut active);
    active.updated_at = Set(super::now());
    active.update(db).await?;
    Ok(())
}

/// Moves the install date of every part `record` installed to the record's
/// date, and their install odometer to its mileage when it has one. A record
/// with no mileage leaves each part's own reading.
// A part that names its installing service is installed or replaced, which
// both take install details, so the write needs no `PartState`.
pub(crate) async fn follow_service(
    db: &impl ConnectionTrait,
    record: &service_record::Model,
) -> DomainResult<()> {
    let mut changes = part::ActiveModel {
        installed_date: Set(Some(record.service_date)),
        updated_at: Set(super::now()),
        ..Default::default()
    };
    if let Some(miles) = record.mileage {
        changes.installed_odometer = Set(Some(miles));
    }
    part::Entity::update_many()
        .set(changes)
        .filter(part::Column::InstalledServiceId.eq(record.id))
        .exec(db)
        .await?;
    Ok(())
}

/// Releases `parts` from the service that installed them, on the caller's
/// transaction. After `did_not_happen` an installed part goes back to
/// `purchased` with its install fields cleared and, when the shop supplied
/// it, leaves its need; a replaced or returned part keeps its status and its
/// need. After `done_another_way` every part keeps its status and install
/// date. Either way no part names the service any more.
pub(crate) async fn release_from_service(
    db: &impl ConnectionTrait,
    parts: Vec<part::Model>,
    completed: CompletedWork,
) -> DomainResult<()> {
    if parts.is_empty() {
        return Ok(());
    }
    if completed == CompletedWork::DoneAnotherWay {
        // Clearing one install detail keeps every part in its state.
        part::Entity::update_many()
            .col_expr(part::Column::InstalledServiceId, Expr::value(None::<i32>))
            .col_expr(part::Column::UpdatedAt, Expr::value(super::now()))
            .filter(part::Column::Id.is_in(parts.iter().map(|p| p.id)))
            .exec(db)
            .await?;
        return Ok(());
    }
    // Only an installed part goes back to purchased. A part a later service
    // replaced did come off the car, so it stays as it is.
    let reverts = |p: &part::Model| p.status == PartStatus::Installed;
    // The owner never held a shop-supplied part, so its need falls back to
    // `shop_supplies` and a re-filed invoice can link a new part to it.
    let shop_ids: Vec<i32> = parts
        .iter()
        .filter(|p| reverts(p) && p.supplied_by == Supplier::Shop)
        .map(|p| p.id)
        .collect();
    if !shop_ids.is_empty() {
        job_need::Entity::update_many()
            .col_expr(job_need::Column::PartId, Expr::value(None::<i32>))
            .col_expr(job_need::Column::UpdatedAt, Expr::value(super::now()))
            .filter(job_need::Column::PartId.is_in(shop_ids))
            .exec(db)
            .await?;
    }
    for p in parts {
        let next = match PartState::of(&p)? {
            PartState::Installed(_) => PartState::Purchased,
            PartState::Replaced { on, odometer, .. } => PartState::Replaced {
                install: Install::default(),
                on,
                odometer,
            },
            other => other,
        };
        let mut active: part::ActiveModel = p.into();
        next.write(&mut active);
        active.updated_at = Set(super::now());
        active.update(db).await?;
    }
    Ok(())
}

/// Delete a part.
pub async fn delete<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<()> {
    let txn = super::begin_write(db).await?;
    let db = &txn;
    get(db, vehicle_id, id).await?.delete(db).await?;
    txn.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn label_joins_manufacturer_and_number() {
        assert_eq!(
            super::label(Some("ATE"), Some("1")).as_deref(),
            Some("ATE 1")
        );
        assert_eq!(super::label(None, Some("1")).as_deref(), Some("1"));
        assert_eq!(super::label(Some("ATE"), None), None);
        assert_eq!(super::label(Some(""), Some(" ")), None);
        assert_eq!(super::label(Some(" "), Some("1")).as_deref(), Some("1"));
    }

    use super::*;
    use crate::{
        inputs::part::ReplacePatch,
        test_support::{VehicleFixture, assert_empty, date, row_count, test_db},
    };

    async fn seed_vehicle(db: &impl ConnectionTrait) -> i32 {
        VehicleFixture::new().insert_id(db).await
    }

    async fn seed_service(db: &impl ConnectionTrait, vehicle_id: i32) -> i32 {
        use crate::entities::service_record;
        service_record::ActiveModel {
            vehicle_id: Set(vehicle_id),
            service_date: Set(date("2024-01-01")),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    fn fitting(status: PartStatus) -> Fitting {
        Fitting::from_fields(
            Some(status),
            InstallPatch::default(),
            ReplacePatch::default(),
        )
        .unwrap()
    }

    fn install_patch(patch: InstallPatch) -> Fitting {
        Fitting::Keep(patch, ReplacePatch::default())
    }

    fn minimal_part(name: &str, installed_service_id: Option<i32>) -> NewPart {
        NewPart {
            name: name.into(),
            manufacturer: None,
            part_number: None,
            oe_part_number_replaced: None,
            seller: None,
            purchase_date: None,
            cost_cents: None,
            cost_currency: None,
            manufacturer_url: None,
            retailer_url: None,
            fitting: installed_service_id.map_or(NewFitting::Purchased, |id| {
                NewFitting::Installed(Install {
                    service_id: Some(id),
                    ..Install::default()
                })
            }),
            notes: None,
            location: None,
            warranty_expires_on: None,
            warranty_expires_miles: None,
            supplied_by: None,
            order_ref: None,
            need_id: None,
            contents: Vec::new(),
        }
    }

    #[tokio::test]
    async fn create_defaults_status_and_get_round_trips() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = create(
            &db,
            vid,
            NewPart {
                cost_cents: Some(1_599),
                ..minimal_part("Oil filter", None)
            },
        )
        .await
        .unwrap();
        assert_eq!(created.status, PartStatus::Purchased);
        let fetched = get(&db, vid, created.id).await.unwrap();
        assert_eq!(fetched.name, "Oil filter");
        assert_eq!(fetched.cost_cents, Some(1_599));
    }

    #[tokio::test]
    async fn location_round_trips_and_clears() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = create(
            &db,
            vid,
            NewPart {
                location: Some("Front brakes".into()),
                ..minimal_part("Brake pads", None)
            },
        )
        .await
        .unwrap();
        assert_eq!(created.location, Some("Front brakes".into()));

        // Omitted location survives an unrelated update.
        let updated = update(
            &db,
            vid,
            created.id,
            UpdatePart {
                fitting: fitting(PartStatus::Installed),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.location, Some("Front brakes".into()));

        // Explicit null clears it (double-option convention).
        let cleared = update(
            &db,
            vid,
            created.id,
            UpdatePart {
                location: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.location, None);
    }

    #[tokio::test]
    async fn update_preserves_installed_service_link() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc_id = seed_service(&db, vid).await;
        let p = create(&db, vid, minimal_part("Brake pads", Some(svc_id)))
            .await
            .unwrap();
        assert_eq!(p.installed_service_id, Some(svc_id));

        // Update an unrelated field; the installed_service_id link must survive.
        let updated = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                fitting: fitting(PartStatus::Installed),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.status, PartStatus::Installed);
        assert_eq!(updated.installed_service_id, Some(svc_id));

        // Explicitly clear the link.
        let cleared = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                fitting: install_patch(InstallPatch {
                    service_id: Some(None),
                    ..InstallPatch::default()
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.installed_service_id, None);
    }

    #[tokio::test]
    async fn create_rejects_other_vehicles_service() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign_svc = seed_service(&db, other).await;

        // Referencing another vehicle's service must 404 and create nothing.
        assert!(matches!(
            create(&db, owner, minimal_part("Part", Some(foreign_svc)))
                .await
                .unwrap_err(),
            DomainError::NotFound(_)
        ));
        crate::test_support::assert_empty(&list(&db, owner, PartFilter::default()).await.unwrap());
    }

    #[tokio::test]
    async fn update_rejects_other_vehicles_service() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign_svc = seed_service(&db, other).await;
        let p = create(&db, owner, minimal_part("Part", None))
            .await
            .unwrap();

        assert!(matches!(
            update(
                &db,
                owner,
                p.id,
                UpdatePart {
                    fitting: install_patch(InstallPatch {
                        service_id: Some(Some(foreign_svc)),
                        ..InstallPatch::default()
                    }),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        // The part is untouched by the rejected update.
        let fetched = get(&db, owner, p.id).await.unwrap();
        assert_eq!(fetched.installed_service_id, None);
    }

    #[tokio::test]
    async fn list_filters_by_status() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        create(
            &db,
            vid,
            NewPart {
                fitting: NewFitting::Installed(Install::default()),
                ..minimal_part("A", None)
            },
        )
        .await
        .unwrap();
        create(
            &db,
            vid,
            NewPart {
                fitting: NewFitting::Purchased,
                ..minimal_part("B", None)
            },
        )
        .await
        .unwrap();

        assert_eq!(
            list(&db, vid, PartFilter::default()).await.unwrap().len(),
            2
        );
        assert_eq!(
            list(
                &db,
                vid,
                PartFilter {
                    status: Some(PartStatus::Purchased)
                }
            )
            .await
            .unwrap()
            .len(),
            1
        );
    }

    // --- delete ----------------------------------------------------------

    fn item(name: &str, number: Option<&str>) -> NewPartContent {
        NewPartContent {
            name: name.into(),
            part_number: number.map(Into::into),
            ..Default::default()
        }
    }

    async fn record_kit(db: &DatabaseConnection, v: i32, contents: Vec<NewPartContent>) -> i32 {
        record(
            db,
            v,
            NewPart {
                contents,
                ..minimal_part("Oil change kit", None)
            },
        )
        .await
        .unwrap()
        .part
        .id
    }

    fn names(view: &PartView) -> Vec<&str> {
        view.contents.iter().map(|c| c.name.as_str()).collect()
    }

    #[tokio::test]
    async fn a_kit_is_recorded_with_its_contents_in_order() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;

        let kit = record(
            &db,
            v,
            NewPart {
                contents: vec![
                    item("Oil filter", Some("HU 7020 z")),
                    item(" Drain plug ", None),
                ],
                ..minimal_part("Oil change kit", None)
            },
        )
        .await
        .unwrap();

        assert_eq!(names(&kit), vec!["Oil filter", "Drain plug"]);
        assert_eq!(kit.contents[0].part_number.as_deref(), Some("HU 7020 z"));
        assert_eq!(kit.contents[1].quantity, 1);
        let listed = list(&db, v, PartFilter::default()).await.unwrap();
        assert_eq!(names(&listed[0]), vec!["Oil filter", "Drain plug"]);
    }

    // Adds and removes kit items on `kit` and returns the result.
    async fn edit(
        db: &DatabaseConnection,
        v: i32,
        kit: i32,
        add: Vec<NewPartContent>,
        remove: Vec<i32>,
    ) -> DomainResult<PartView> {
        correct(
            db,
            v,
            kit,
            UpdatePart {
                add_contents: add,
                remove_contents: remove,
                ..Default::default()
            },
        )
        .await
    }

    #[tokio::test]
    async fn adding_items_puts_them_after_the_ones_the_kit_holds() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let kit = record_kit(
            &db,
            v,
            vec![item("Oil filter", None), item("Drain plug", None)],
        )
        .await;

        let renamed = correct(
            &db,
            v,
            kit,
            UpdatePart {
                name: Some("Liqui Moly kit".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(names(&renamed), vec!["Oil filter", "Drain plug"]);

        let added = edit(&db, v, kit, vec![item("Crush washer", None)], vec![])
            .await
            .unwrap();
        assert_eq!(
            names(&added),
            vec!["Oil filter", "Drain plug", "Crush washer"]
        );
    }

    #[tokio::test]
    async fn removing_items_deletes_only_the_named_ones() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let kit = record_kit(
            &db,
            v,
            vec![
                item("Oil filter", None),
                item("Drain plug", None),
                item("Seal", None),
            ],
        )
        .await;
        let ids: Vec<i32> = list(&db, v, PartFilter::default()).await.unwrap()[0]
            .contents
            .iter()
            .map(|c| c.id)
            .collect();

        // Removing the middle item and adding one in the same call keeps the
        // other two in place and puts the new one last.
        let edited = edit(
            &db,
            v,
            kit,
            vec![item("Drain plug", Some("N 908 132 02"))],
            vec![ids[1], ids[1]],
        )
        .await
        .unwrap();
        assert_eq!(names(&edited), vec!["Oil filter", "Seal", "Drain plug"]);
        assert_eq!(
            edited.contents[2].part_number.as_deref(),
            Some("N 908 132 02")
        );

        let all: Vec<i32> = edited.contents.iter().map(|c| c.id).collect();
        let emptied = edit(&db, v, kit, vec![], all).await.unwrap();
        assert_empty(&emptied.contents);
        assert_eq!(row_count::<part_content::Entity>(&db).await, 0);
    }

    #[tokio::test]
    async fn removing_another_parts_item_is_byte_identical_to_a_missing_one() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let kit = record_kit(&db, v, vec![item("Oil filter", None)]).await;
        let other = record_kit(&db, v, vec![item("Rotor bolt", None)]).await;
        let theirs = list(&db, v, PartFilter::default())
            .await
            .unwrap()
            .into_iter()
            .find(|p| p.part.id == other)
            .unwrap()
            .contents[0]
            .id;
        let missing = theirs + 1000;

        let wrong = edit(&db, v, kit, vec![item("Seal", None)], vec![theirs])
            .await
            .unwrap_err();
        let absent = edit(&db, v, kit, vec![item("Seal", None)], vec![missing])
            .await
            .unwrap_err();

        let say = |e: DomainError, id: i32| e.to_string().replace(&id.to_string(), "N");
        assert!(matches!(wrong, DomainError::NotFound(_)), "{wrong:?}");
        assert_eq!(say(wrong, theirs), say(absent, missing));
        // Nothing was written: the kit kept its item and gained none, and the
        // other kit kept its own.
        assert_eq!(row_count::<part_content::Entity>(&db).await, 2);
    }

    #[tokio::test]
    async fn a_bad_added_item_keeps_the_items_named_for_removal() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let kit = record_kit(&db, v, vec![item("Oil filter", None)]).await;
        let id = list(&db, v, PartFilter::default()).await.unwrap()[0].contents[0].id;

        let err = edit(&db, v, kit, vec![item(" ", None)], vec![id])
            .await
            .unwrap_err();

        assert!(matches!(err, DomainError::Invalid { .. }), "{err:?}");
        assert_eq!(row_count::<part_content::Entity>(&db).await, 1);
    }

    #[tokio::test]
    async fn a_blank_item_or_a_zero_quantity_is_refused_and_writes_nothing() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let kit = record_kit(&db, v, vec![item("Oil filter", None)]).await;
        let zero = NewPartContent {
            quantity: Some(0),
            ..item("Oil filter", None)
        };

        for bad in [item("  ", None), zero] {
            let err = correct(
                &db,
                v,
                kit,
                UpdatePart {
                    name: Some("Renamed".into()),
                    add_contents: vec![bad.clone()],
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
            assert!(
                matches!(&err, DomainError::Invalid { field, .. } if field == "add_contents"),
                "{err:?}"
            );
            let err = record(
                &db,
                v,
                NewPart {
                    contents: vec![bad],
                    ..minimal_part("Another kit", None)
                },
            )
            .await
            .unwrap_err();
            assert!(matches!(err, DomainError::Invalid { .. }), "{err:?}");
        }

        let kept = list(&db, v, PartFilter::default()).await.unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].part.name, "Oil change kit");
        assert_eq!(names(&kept[0]), vec!["Oil filter"]);
    }

    #[tokio::test]
    async fn deleting_a_kit_deletes_its_contents() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let kit = record_kit(&db, v, vec![item("Oil filter", None)]).await;

        delete(&db, v, kit).await.unwrap();

        assert_eq!(row_count::<part_content::Entity>(&db).await, 0);
    }

    #[tokio::test]
    async fn delete_wrong_vehicle_is_byte_identical_to_nonexistent() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let other_vid = seed_vehicle(&db).await;
        let part = create(&db, vid, minimal_part("Mine", None)).await.unwrap();

        let wrong_parent = delete(&db, other_vid, part.id).await.unwrap_err();
        // Compare against the nonexistent-id message for the SAME id: no
        // ownership oracle distinguishing "exists elsewhere" from "not at all".
        let DomainError::NotFound(wrong_parent_msg) = wrong_parent else {
            panic!("expected NotFound");
        };
        assert_eq!(wrong_parent_msg, format!("Part {} not found", part.id));
        // The part survives a wrong-parent delete attempt.
        assert!(get(&db, vid, part.id).await.is_ok());
    }

    // --- supplied_by ------------------------------------------------------

    #[tokio::test]
    async fn supplied_by_defaults_to_self_and_can_be_changed() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let made = create(&db, vid, minimal_part("Pads", None)).await.unwrap();
        assert_eq!(made.supplied_by, Supplier::Owner);

        let shop_part = create(
            &db,
            vid,
            NewPart {
                supplied_by: Some(Supplier::Shop),
                ..minimal_part("Fluid", None)
            },
        )
        .await
        .unwrap();
        assert_eq!(shop_part.supplied_by, Supplier::Shop);

        let updated = update(
            &db,
            vid,
            made.id,
            UpdatePart {
                supplied_by: Some(Supplier::Shop),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.supplied_by, Supplier::Shop);
    }

    #[tokio::test]
    async fn a_returned_part_cannot_be_installed() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sid = seed_service(&db, vid).await;
        let record = service_record::Entity::find_by_id(sid)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let p = seed_returned(&db, vid).await;
        let err = install(&db, vid, p.id, &record, None).await.unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        let after = get(&db, vid, p.id).await.unwrap();
        assert_eq!(after.status, PartStatus::Returned);
        assert_eq!(after.installed_service_id, None);
    }

    #[tokio::test]
    async fn order_ref_and_received_on_round_trip() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let p = create(
            &db,
            vid,
            NewPart {
                order_ref: Some("A-1".into()),
                ..minimal_part("Rotors", None)
            },
        )
        .await
        .unwrap();
        assert_eq!(p.order_ref.as_deref(), Some("A-1"));
        let updated = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                received_on: Some(Some(date("2026-11-05"))),
                order_ref: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.received_on, Some(date("2026-11-05")));
        assert_eq!(updated.order_ref, None);
    }

    async fn seed_returned(db: &impl ConnectionTrait, vid: i32) -> part::Model {
        part::ActiveModel {
            vehicle_id: Set(vid),
            name: Set("Rotors".into()),
            status: Set(PartStatus::Returned),
            supplied_by: Set(Supplier::Owner),
            returned_on: Set(Some(date("2026-11-06"))),
            return_reason: Set(Some("wrong size".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[test]
    fn returned_cannot_be_set_through_create_or_update() {
        // The status is refused while the arguments are read, before any write.
        let err =
            NewFitting::from_fields(Some(PartStatus::Returned), Install::default()).unwrap_err();
        assert!(
            matches!(&err, DomainError::BadRequest(m) if m.contains("return_part")),
            "{err:?}"
        );
        let err = Fitting::from_fields(
            Some(PartStatus::Returned),
            InstallPatch::default(),
            ReplacePatch::default(),
        )
        .unwrap_err();
        assert!(
            matches!(&err, DomainError::BadRequest(m) if m.contains("return_part")),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn a_returned_part_can_only_go_back_to_purchased_and_loses_its_return_fields() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let p = seed_returned(&db, vid).await;
        for status in [PartStatus::Installed, PartStatus::Replaced] {
            let err = update(
                &db,
                vid,
                p.id,
                UpdatePart {
                    fitting: fitting(status),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
            assert!(
                matches!(err, DomainError::BadRequest(_)),
                "{status}: {err:?}"
            );
        }
        let noted = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                notes: Some(Some("still at the door".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(noted.status, PartStatus::Returned);
        assert_eq!(noted.return_reason.as_deref(), Some("wrong size"));
        let back = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                fitting: fitting(PartStatus::Purchased),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(back.status, PartStatus::Purchased);
        assert_eq!(back.returned_on, None);
        assert_eq!(back.return_reason, None);
        assert_eq!(back.returned_from_need_id, None);
    }

    #[tokio::test]
    async fn a_returned_part_cannot_be_given_install_details() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sid = seed_service(&db, vid).await;
        let p = seed_returned(&db, vid).await;
        let attempts = [
            UpdatePart {
                fitting: install_patch(InstallPatch {
                    service_id: Some(Some(sid)),
                    ..InstallPatch::default()
                }),
                ..Default::default()
            },
            UpdatePart {
                fitting: install_patch(InstallPatch {
                    date: Some(Some(date("2026-11-10"))),
                    ..InstallPatch::default()
                }),
                ..Default::default()
            },
            UpdatePart {
                fitting: install_patch(InstallPatch {
                    odometer: Some(Some(50_000)),
                    ..InstallPatch::default()
                }),
                ..Default::default()
            },
        ];
        for attempt in attempts {
            let err = update(&db, vid, p.id, attempt).await.unwrap_err();
            assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        }
        let cleared = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                fitting: install_patch(InstallPatch {
                    date: Some(None),
                    ..InstallPatch::default()
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.status, PartStatus::Returned);
    }

    async fn seed_installed(db: &DatabaseConnection, vid: i32, sid: i32) -> part::Model {
        create(
            db,
            vid,
            NewPart {
                fitting: NewFitting::Installed(Install {
                    service_id: Some(sid),
                    date: Some(date("2024-01-01")),
                    odometer: Some(40_000),
                }),
                ..minimal_part("Brake pads", None)
            },
        )
        .await
        .unwrap()
    }

    fn to_purchased() -> UpdatePart {
        UpdatePart {
            fitting: fitting(PartStatus::Purchased),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn correcting_an_installed_part_to_purchased_clears_its_install_details() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sid = seed_service(&db, vid).await;
        let p = seed_installed(&db, vid, sid).await;

        let updated = update(&db, vid, p.id, to_purchased()).await.unwrap();
        assert_eq!(updated.status, PartStatus::Purchased);
        assert_eq!(updated.installed_service_id, None);
        assert_eq!(updated.installed_date, None);
        assert_eq!(updated.installed_odometer, None);

        // The service no longer holds the part, so deleting it leaves the
        // part as the correction left it.
        super::super::service_record::delete(&db, vid, sid, super::super::service_record::UNDO)
            .await
            .unwrap();
        let after = get(&db, vid, p.id).await.unwrap();
        assert_eq!(after.status, PartStatus::Purchased);
        assert_eq!(after.installed_date, None);
    }

    #[tokio::test]
    async fn correcting_a_replaced_part_to_purchased_clears_its_install_and_replace_details() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sid = seed_service(&db, vid).await;
        let p = seed_installed(&db, vid, sid).await;
        update(
            &db,
            vid,
            p.id,
            UpdatePart {
                fitting: Fitting::Replaced(
                    InstallPatch::default(),
                    ReplacePatch {
                        date: Some(Some(date("2025-01-01"))),
                        odometer: Some(Some(50_000)),
                    },
                ),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let updated = update(&db, vid, p.id, to_purchased()).await.unwrap();
        assert_eq!(updated.status, PartStatus::Purchased);
        assert_eq!(updated.installed_service_id, None);
        assert_eq!(updated.installed_date, None);
        assert_eq!(updated.installed_odometer, None);
        assert_eq!(updated.replaced_date, None);
        assert_eq!(updated.replaced_odometer, None);
    }

    #[tokio::test]
    async fn a_purchased_part_takes_no_install_or_replacement_details() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let p = create(&db, vid, minimal_part("Wiper", None)).await.unwrap();
        let attempts = [
            install_patch(InstallPatch {
                date: Some(Some(date("2024-01-01"))),
                ..InstallPatch::default()
            }),
            Fitting::Keep(
                InstallPatch::default(),
                ReplacePatch {
                    odometer: Some(Some(9_000)),
                    ..ReplacePatch::default()
                },
            ),
        ];
        for attempt in attempts {
            let err = update(
                &db,
                vid,
                p.id,
                UpdatePart {
                    fitting: attempt,
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
            assert_eq!(
                err.to_string(),
                format!(
                    "Part {} is purchased, so it takes no install or replacement details; set its \
                     status to installed or replaced with them",
                    p.id
                )
            );
        }
        let after = get(&db, vid, p.id).await.unwrap();
        assert_eq!(PartState::of(&after).unwrap(), PartState::Purchased);

        // Clearing a detail the part does not hold changes nothing.
        let cleared = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                fitting: install_patch(InstallPatch {
                    date: Some(None),
                    ..InstallPatch::default()
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.status, PartStatus::Purchased);

        // A new part with install details names its status.
        let err = NewFitting::from_fields(
            None,
            Install {
                date: Some(date("2024-01-01")),
                ..Install::default()
            },
        )
        .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
    }

    #[tokio::test]
    async fn going_back_to_purchased_refuses_contradicting_details() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sid = seed_service(&db, vid).await;
        let p = seed_installed(&db, vid, sid).await;

        for (install, replace) in [
            (
                InstallPatch {
                    odometer: Some(Some(41_000)),
                    ..InstallPatch::default()
                },
                ReplacePatch::default(),
            ),
            (
                InstallPatch::default(),
                ReplacePatch {
                    date: Some(Some(date("2025-01-01"))),
                    ..ReplacePatch::default()
                },
            ),
        ] {
            let err =
                Fitting::from_fields(Some(PartStatus::Purchased), install, replace).unwrap_err();
            assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        }
        let err = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                received_on: Some(None),
                ..to_purchased()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        let after = get(&db, vid, p.id).await.unwrap();
        assert_eq!(after.status, PartStatus::Installed);
        assert_eq!(after.installed_odometer, Some(40_000));
    }

    #[tokio::test]
    async fn a_part_off_the_car_reads_received() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sid = seed_service(&db, vid).await;
        let p = seed_installed(&db, vid, sid).await;
        assert_eq!(p.received_on, None);
        let updated = update(&db, vid, p.id, to_purchased()).await.unwrap();
        assert_eq!(
            updated.received_on,
            Some(date("2024-01-01")),
            "the install date"
        );

        let held = create(
            &db,
            vid,
            NewPart {
                ..minimal_part("Wiper", Some(sid))
            },
        )
        .await
        .unwrap();
        let updated = update(
            &db,
            vid,
            held.id,
            UpdatePart {
                received_on: Some(Some(date("2023-12-20"))),
                ..to_purchased()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            updated.received_on,
            Some(date("2023-12-20")),
            "the date sent"
        );

        let bulb = create(
            &db,
            vid,
            NewPart {
                ..minimal_part("Bulb", Some(sid))
            },
        )
        .await
        .unwrap();
        let updated = update(&db, vid, bulb.id, to_purchased()).await.unwrap();
        assert_eq!(updated.received_on, Some(crate::services::today()), "today");

        // A part is never received before it was bought.
        let late = create(
            &db,
            vid,
            NewPart {
                fitting: NewFitting::Installed(Install {
                    service_id: Some(sid),
                    date: Some(date("2024-01-01")),
                    odometer: None,
                }),
                purchase_date: Some(date("2024-03-01")),
                ..minimal_part("Fuse", None)
            },
        )
        .await
        .unwrap();
        let updated = update(&db, vid, late.id, to_purchased()).await.unwrap();
        assert_eq!(updated.received_on, Some(date("2024-03-01")));
    }

    #[tokio::test]
    async fn a_shop_part_cannot_go_back_to_purchased() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sid = seed_service(&db, vid).await;
        for status in [PartStatus::Installed, PartStatus::Replaced] {
            let shop = create(
                &db,
                vid,
                NewPart {
                    supplied_by: Some(Supplier::Shop),
                    fitting: NewFitting::from_fields(
                        Some(status),
                        Install {
                            service_id: Some(sid),
                            ..Install::default()
                        },
                    )
                    .unwrap(),
                    ..minimal_part("Wiper blades", None)
                },
            )
            .await
            .unwrap();
            let err = update(&db, vid, shop.id, to_purchased()).await.unwrap_err();
            assert!(
                matches!(err, DomainError::BadRequest(_)),
                "{status}: {err:?}"
            );
            assert_eq!(get(&db, vid, shop.id).await.unwrap().status, status);

            // Saying the owner supplied it makes the part the owner's to hold.
            let owned = update(
                &db,
                vid,
                shop.id,
                UpdatePart {
                    supplied_by: Some(Supplier::Owner),
                    ..to_purchased()
                },
            )
            .await
            .unwrap();
            assert_eq!(owned.status, PartStatus::Purchased);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn racing_records_and_deletes_all_succeed() {
        let (_dir, pools) = crate::test_support::race_pools().await;
        let first = &pools[0];
        let v = seed_vehicle(first).await;
        let mut ids = Vec::new();
        for i in 0..pools.len() * 2 {
            ids.push(
                create(first, v, minimal_part(&format!("Part {i}"), None))
                    .await
                    .unwrap()
                    .id,
            );
        }
        let results = crate::test_support::race(&pools, |i, db| {
            let id = ids[i];
            async move {
                if i % 2 == 0 {
                    create(&db, v, minimal_part("New part", None))
                        .await
                        .map(|_| ())
                } else {
                    delete(&db, v, id).await
                }
            }
        })
        .await;
        assert!(results.iter().all(Result::is_ok), "{results:?}");
    }

    async fn refit(
        db: &DatabaseConnection,
        vid: i32,
        id: i32,
        fitting: Fitting,
    ) -> DomainResult<part::Model> {
        update(
            db,
            vid,
            id,
            UpdatePart {
                fitting,
                ..Default::default()
            },
        )
        .await
    }

    fn replace(
        date_sent: Option<Option<chrono::NaiveDate>>,
        odometer: Option<Option<i32>>,
    ) -> ReplacePatch {
        ReplacePatch {
            date: date_sent,
            odometer,
        }
    }

    #[tokio::test]
    async fn each_state_reads_back_as_written() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sid = seed_service(&db, vid).await;
        let install = Install {
            service_id: Some(sid),
            date: Some(date("2024-01-01")),
            odometer: Some(40_000),
        };
        let need_part = create(&db, vid, minimal_part("Pads", None)).await.unwrap();
        for state in [
            PartState::Purchased,
            PartState::Installed(install),
            PartState::Installed(Install::default()),
            PartState::Replaced {
                install,
                on: Some(date("2025-01-01")),
                odometer: Some(50_000),
            },
            PartState::Replaced {
                install: Install::default(),
                on: None,
                odometer: None,
            },
            PartState::Returned {
                on: date("2026-11-06"),
                reason: "wrong size".into(),
                need_id: None,
            },
        ] {
            let mut active: part::ActiveModel = need_part.clone().into();
            state.clone().write(&mut active);
            let stored = active.update(&db).await.unwrap();
            assert_eq!(PartState::of(&stored).unwrap(), state);
            // The reply keeps the flat keys.
            let json = serde_json::to_value(view(&db, stored).await.unwrap()).unwrap();
            for key in [
                "status",
                "installed_service_id",
                "installed_date",
                "replaced_date",
                "returned_on",
                "return_reason",
            ] {
                assert!(json.get(key).is_some(), "{key} in {json}");
            }
        }
    }

    #[tokio::test]
    async fn a_row_no_state_fits_reads_as_an_internal_error() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let p = create(&db, vid, minimal_part("Pads", None)).await.unwrap();
        let unreadable = [
            part::Model {
                installed_date: Some(date("2024-01-01")),
                ..p.clone()
            },
            part::Model {
                status: PartStatus::Installed,
                replaced_odometer: Some(1),
                ..p.clone()
            },
            part::Model {
                status: PartStatus::Returned,
                returned_on: Some(date("2024-01-01")),
                ..p.clone()
            },
            part::Model {
                return_reason: Some("wrong".into()),
                ..p.clone()
            },
        ];
        for row in unreadable {
            let err = PartState::of(&row).unwrap_err();
            assert!(
                matches!(&err, DomainError::Internal(m) if m.starts_with(&format!("part {} is", p.id))),
                "{err:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_installed_part_takes_replacement_details_only_as_replaced() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sid = seed_service(&db, vid).await;
        let p = seed_installed(&db, vid, sid).await;
        let err = refit(
            &db,
            vid,
            p.id,
            Fitting::Keep(
                InstallPatch::default(),
                replace(Some(Some(date("2025-01-01"))), None),
            ),
        )
        .await
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "Part {} is installed, so it takes no replacement details; set its status to \
                 replaced with them",
                p.id
            )
        );
        let err = Fitting::from_fields(
            Some(PartStatus::Installed),
            InstallPatch::default(),
            replace(None, Some(Some(50_000))),
        )
        .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");

        let replaced = refit(
            &db,
            vid,
            p.id,
            Fitting::Replaced(
                InstallPatch::default(),
                replace(Some(Some(date("2025-01-01"))), None),
            ),
        )
        .await
        .unwrap();
        assert_eq!(
            PartState::of(&replaced).unwrap(),
            PartState::Replaced {
                install: Install {
                    service_id: Some(sid),
                    date: Some(date("2024-01-01")),
                    odometer: Some(40_000),
                },
                on: Some(date("2025-01-01")),
                odometer: None,
            }
        );
    }

    #[tokio::test]
    async fn a_replaced_part_goes_back_to_installed_only_with_its_replacement_cleared() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sid = seed_service(&db, vid).await;
        let p = seed_installed(&db, vid, sid).await;
        refit(
            &db,
            vid,
            p.id,
            Fitting::Replaced(
                InstallPatch::default(),
                replace(Some(Some(date("2025-01-01"))), Some(Some(50_000))),
            ),
        )
        .await
        .unwrap();
        let to_installed = |replace_sent| {
            Fitting::from_fields(
                Some(PartStatus::Installed),
                InstallPatch::default(),
                replace_sent,
            )
            .unwrap()
        };
        for half in [
            replace(None, None),
            replace(Some(None), None),
            replace(None, Some(None)),
        ] {
            let err = refit(&db, vid, p.id, to_installed(half)).await.unwrap_err();
            assert_eq!(
                err.to_string(),
                format!(
                    "Part {} was replaced; clear its replacement date and odometer in the same \
                     change to set it back to installed",
                    p.id
                )
            );
        }
        let back = refit(
            &db,
            vid,
            p.id,
            to_installed(replace(Some(None), Some(None))),
        )
        .await
        .unwrap();
        assert_eq!(back.status, PartStatus::Installed);
        assert_eq!((back.replaced_date, back.replaced_odometer), (None, None));
        assert_eq!(back.installed_service_id, Some(sid));
    }

    #[tokio::test]
    async fn a_service_installing_a_replaced_part_clears_its_replacement() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let first = seed_service(&db, vid).await;
        let p = seed_replaced(&db, vid, first).await;
        let second = service_by_id(&db, seed_service(&db, vid).await).await;
        install(&db, vid, p.id, &second, None).await.unwrap();
        let after = get(&db, vid, p.id).await.unwrap();
        assert_eq!(
            PartState::of(&after).unwrap(),
            PartState::Installed(Install {
                service_id: Some(second.id),
                date: Some(date("2024-01-01")),
                odometer: Some(40_000),
            }),
            "the stored odometer is kept"
        );
    }

    #[tokio::test]
    async fn the_service_that_installed_a_replaced_part_leaves_it_replaced() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sid = seed_service(&db, vid).await;
        let p = seed_replaced(&db, vid, sid).await;
        let record = service_by_id(&db, sid).await;
        install(&db, vid, p.id, &record, None).await.unwrap();
        let after = get(&db, vid, p.id).await.unwrap();
        assert_eq!(
            PartState::of(&after).unwrap(),
            PartState::Replaced {
                install: Install {
                    service_id: Some(sid),
                    date: Some(date("2024-01-01")),
                    odometer: Some(40_000),
                },
                on: Some(date("2025-01-01")),
                odometer: None,
            }
        );
    }

    async fn service_by_id(db: &DatabaseConnection, id: i32) -> service_record::Model {
        service_record::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    // A part `sid` installed that a later service replaced on 2025-01-01.
    async fn seed_replaced(db: &DatabaseConnection, vid: i32, sid: i32) -> part::Model {
        let p = seed_installed(db, vid, sid).await;
        refit(
            db,
            vid,
            p.id,
            Fitting::Replaced(
                InstallPatch::default(),
                replace(Some(Some(date("2025-01-01"))), None),
            ),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_part_is_never_received_before_it_was_bought() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let p = create(
            &db,
            vid,
            NewPart {
                purchase_date: Some(date("2026-11-05")),
                ..minimal_part("Rotors", None)
            },
        )
        .await
        .unwrap();
        let early = UpdatePart {
            received_on: Some(Some(date("2026-11-04"))),
            ..Default::default()
        };
        let err = update(&db, vid, p.id, early).await.unwrap_err();
        assert_eq!(
            err.to_string(),
            "received_on: is before the purchase date 2026-11-05; correct the purchase date first \
             if it is wrong"
        );
        update(
            &db,
            vid,
            p.id,
            UpdatePart {
                received_on: Some(Some(date("2026-11-05"))),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        // A later purchase date is refused against the stored arrival.
        let err = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                purchase_date: Some(Some(date("2026-11-06"))),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "received_on: is before the purchase date 2026-11-06; correct the purchase date first \
             if it is wrong"
        );
        assert_eq!(
            get(&db, vid, p.id).await.unwrap().purchase_date,
            Some(date("2026-11-05"))
        );
    }
}
