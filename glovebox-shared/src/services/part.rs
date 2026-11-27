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
    inputs::{
        part::{
            Fitting, Install, InstallPatch, NewFitting, NewPart, NewPartContent, PartFilter,
            UpdatePart,
        },
        wear::WearTag,
    },
    services::{service_record::CompletedWork, warranty::WarrantyBounds},
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

/// Marks `p` returned on `on` for `reason`, remembering the need it was
/// returned from.
pub(crate) async fn mark_returned(
    db: &impl ConnectionTrait,
    p: part::Model,
    reason: String,
    need_id: i32,
    on: NaiveDate,
) -> DomainResult<part::Model> {
    let mut active: part::ActiveModel = p.into();
    PartState::Returned {
        on,
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

/// A part in a list, with how its warranty reads on the day of the list.
#[derive(Debug, Serialize)]
pub struct ListedPart {
    #[serde(flatten)]
    pub view: PartView,
    /// The part's warranty bounds as of the list's day and the vehicle's
    /// estimated mileage. The key is left out for a part with no warranty
    /// expiry date and no expiry reading. It reads the bounds alone, so a
    /// part that is off the car can still show `in_force`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warranty: Option<WarrantyBounds>,
}

/// Whether a part counts as under warranty: it is purchased or installed and
/// its warranty is in force.
#[must_use]
pub fn is_under_warranty(part: &part::Model, warranty: Option<&WarrantyBounds>) -> bool {
    matches!(part.status, PartStatus::Purchased | PartStatus::Installed)
        && warranty.is_some_and(|w| w.in_force)
}

/// The vehicle's parts, newest first, each with its kit contents. A filter on
/// `under_warranty` is judged as of the day the server runs.
pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    filter: PartFilter,
) -> DomainResult<Vec<PartView>> {
    let listed = list_on(db, vehicle_id, filter, crate::services::today()).await?;
    Ok(listed.into_iter().map(|p| p.view).collect())
}

/// The vehicle's parts, most recently recorded first, each with its kit
/// contents and its warranty as of `today`. `filter.limit` keeps that many
/// from the top of the filtered list, and a limit of zero is `Invalid`. A mileage bound is compared with the vehicle's
/// estimated mileage on that day.
pub async fn list_on(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    filter: PartFilter,
    today: NaiveDate,
) -> DomainResult<Vec<ListedPart>> {
    let vehicle = super::vehicle::require(db, vehicle_id).await?;
    super::validate_limit(filter.limit)?;
    let mut query = part::Entity::find().filter(part::Column::VehicleId.eq(vehicle_id));

    if let Some(status) = filter.status {
        query = query.filter(part::Column::Status.eq(status));
    }

    // Stamps hold whole seconds, so the id orders parts recorded together.
    let parts = query
        .order_by_desc(part::Column::CreatedAt)
        .order_by_desc(part::Column::Id)
        .all(db)
        .await?;
    let mileage = if parts.iter().any(|p| p.warranty_expires_miles.is_some()) {
        super::reminders::estimate_mileage(db, vehicle_id, &vehicle, today)
            .await?
            .0
    } else {
        0
    };
    let mut judged: Vec<(part::Model, Option<WarrantyBounds>)> = parts
        .into_iter()
        .map(|p| {
            let bounds = super::warranty::read(
                p.warranty_expires_on,
                p.warranty_expires_miles,
                mileage,
                today,
            );
            (p, bounds)
        })
        .collect();
    if let Some(wanted) = filter.under_warranty {
        judged.retain(|(p, bounds)| is_under_warranty(p, bounds.as_ref()) == wanted);
    }
    // The warranty filter is judged in memory, so the limit is applied after it.
    if let Some(limit) = filter.limit {
        judged.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    }

    let (parts, warranties): (Vec<_>, Vec<_>) = judged.into_iter().unzip();
    Ok(views(db, parts)
        .await?
        .into_iter()
        .zip(warranties)
        .map(|(view, warranty)| ListedPart { view, warranty })
        .collect())
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

// The tag's two columns are written only here, so they always hold a pair
// `WearTag` can read back.
fn write_wear_tag(active: &mut part::ActiveModel, tag: Option<WearTag>) {
    active.wears_component = Set(tag.map(WearTag::component));
    active.wears_axle = Set(tag.and_then(WearTag::axle));
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
        // The column defaults to 'USD'. No input names a currency, and every
        // other currency column holds NULL, so a part's does too.
        cost_currency: Set(None),
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
    write_wear_tag(&mut model, input.wears_as);
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
    if let Some(v) = input.wears_as {
        write_wear_tag(&mut active, v);
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
mod tests;
