//! Needs on a job and the researched options for each. A need belongs to a
//! work item, and its vehicle is the work item's vehicle. A wrong-vehicle id
//! reads exactly like a nonexistent one.

use sea_orm::{sea_query::Expr, *};
use serde::Serialize;

use crate::{
    entities::{
        job_need,
        need_option::{self, Fitment},
        part::{self, Supplier},
        todo,
        work_item::{self, JobStatus},
    },
    error::{DomainError, DomainResult},
    inputs::{
        need::{
            ArrivalChange, NewNeed, NewOption, OrderDetails, PartLink, ReturnInput, UpdateNeed,
            UpdateOption,
        },
        part::NewPart,
        todo::NewTodo,
    },
};

/// Where a need stands, from "nobody has decided" to "on the car".
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Undecided,
    ToOrder,
    ShopSupplies,
    Ordered,
    Received,
    Installed,
}

impl Stage {
    /// Whether the need still asks something of the owner before the visit:
    /// it is undecided, waiting to be ordered, or ordered and not received.
    #[must_use]
    pub fn is_open(self) -> bool {
        matches!(self, Stage::Undecided | Stage::ToOrder | Stage::Ordered)
    }

    /// Whether the checklist offers to order the need's part.
    #[must_use]
    pub fn can_order(self) -> bool {
        self == Stage::ToOrder
    }

    /// Whether the checklist offers to mark the need's part received.
    #[must_use]
    pub fn can_receive(self) -> bool {
        self == Stage::Ordered
    }

    /// Whether the checklist offers to clear the need's arrival date.
    #[must_use]
    pub fn can_unreceive(self) -> bool {
        self == Stage::Received
    }

    /// Whether the need has a linked part that is not yet on the car: it is
    /// `ordered` or `received`. Only such a part can be returned or have its
    /// arrival date set or cleared through the need.
    #[must_use]
    pub fn has_unfitted_part(self) -> bool {
        matches!(self, Stage::Ordered | Stage::Received)
    }
}

/// Whether a need at `stage` on a job whose status is `job_status` counts as
/// open: the stage is open and the job is still planned. Every count of open
/// needs uses this.
#[must_use]
pub(crate) fn counts_as_open(job_status: JobStatus, stage: Stage) -> bool {
    !job_status.is_finished() && stage.is_open()
}

/// Derives a need's stage. The first matching row wins, so a linked part
/// decides the stage whatever the supplier and chosen option say, and a part
/// that is installed or replaced reads `installed`. `part` is the need's
/// linked part, if any.
#[must_use]
pub fn stage(need: &job_need::Model, part: Option<&part::Model>) -> Stage {
    if let Some(part) = part {
        return if super::part::was_fitted(part) {
            Stage::Installed
        } else if part.received_on.is_some() {
            Stage::Received
        } else {
            Stage::Ordered
        };
    }
    match need.supplied_by {
        Some(Supplier::Shop) => Stage::ShopSupplies,
        Some(Supplier::Owner) if need.chosen_option_id.is_some() => Stage::ToOrder,
        _ => Stage::Undecided,
    }
}

fn validate_quantity(quantity: i32) -> DomainResult<()> {
    if quantity < 1 {
        return Err(DomainError::invalid("quantity", "must be at least 1"));
    }
    Ok(())
}

fn validate_price(price_cents: Option<i32>) -> DomainResult<()> {
    if price_cents.is_some_and(|p| p < 0) {
        return Err(DomainError::invalid("price_cents", "must not be negative"));
    }
    Ok(())
}

/// Returns true when `option` has been rejected. A rejected option cannot be
/// chosen, and it is kept as history.
#[must_use]
pub fn is_rejected(option: &need_option::Model) -> bool {
    // The table keeps a rejection's reason and date together.
    option.rejected_reason.is_some()
}

/// Returns true when `option` cannot be chosen: it has been rejected, or it
/// does not fit the car.
#[must_use]
pub fn is_ruled_out(option: &need_option::Model) -> bool {
    is_rejected(option) || option.fitment == Fitment::DoesNotFit
}

/// Loads a need that belongs to `vehicle_id`, resolving need, work item and
/// vehicle in one joined query.
///
/// # Errors
/// `NotFound` when the need does not exist or belongs to another vehicle.
pub async fn require(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    need_id: i32,
) -> DomainResult<job_need::Model> {
    job_need::Entity::find_by_id(need_id)
        .inner_join(work_item::Entity)
        .filter(work_item::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Need {need_id} not found")))
}

async fn require_option(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    option_id: i32,
) -> DomainResult<need_option::Model> {
    let not_found = || DomainError::NotFound(format!("Option {option_id} not found"));
    let option = need_option::Entity::find_by_id(option_id)
        .one(db)
        .await?
        .ok_or_else(not_found)?;
    require(db, vehicle_id, option.need_id)
        .await
        .map_err(|_| not_found())?;
    Ok(option)
}

/// Adds a need to a work item of the vehicle.
///
/// # Errors
/// `NotFound` for a work item that is not the vehicle's; `Invalid` for a
/// blank name or a quantity below 1.
pub async fn add<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    work_item_id: i32,
    input: NewNeed,
) -> DomainResult<job_need::Model> {
    super::validate_not_blank("name", &input.name)?;
    let quantity = input.quantity.unwrap_or(1);
    validate_quantity(quantity)?;
    let txn = super::begin_write(db).await?;
    let db = &txn;
    super::work_item::get(db, vehicle_id, work_item_id).await?;
    let model = job_need::ActiveModel {
        work_item_id: Set(work_item_id),
        name: Set(input.name),
        quantity: Set(quantity),
        supplied_by: Set(input.supplied_by),
        approach: Set(input.approach),
        ..Default::default()
    };
    let out = model.insert(db).await?;
    txn.commit().await?;
    Ok(out)
}

/// Changes a need's name, quantity, supplier, approach or decision reason. A
/// need with no chosen option holds a reason only while the shop supplies it,
/// judged with the supplier this call leaves, so any other reason is refused.
/// Moving a need with no chosen option away from `shop` clears its stored
/// reason, whether or not the call sends that reason back. Clearing a reason
/// is always allowed.
///
/// # Errors
/// `NotFound` for a need that is not the vehicle's; `Invalid` as for [`add`],
/// and on `decision_reason` for a reason the need cannot hold.
pub async fn update<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
    input: UpdateNeed,
) -> DomainResult<job_need::Model> {
    let txn = super::begin_write(db).await?;
    let updated = apply_update(&txn, vehicle_id, need_id, input).await?;
    txn.commit().await?;
    Ok(updated)
}

// Applies an edit on the caller's write transaction, for `update` and
// `update_with_choice`.
async fn apply_update(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    need_id: i32,
    input: UpdateNeed,
) -> DomainResult<job_need::Model> {
    let existing = require(db, vehicle_id, need_id).await?;
    if let Some(name) = &input.name {
        super::validate_not_blank("name", name)?;
    }
    if let Some(quantity) = input.quantity {
        validate_quantity(quantity)?;
    }
    let supplied_by = input.supplied_by.unwrap_or(existing.supplied_by);
    let mut decision = Decision::of(&existing)?.for_supplier(supplied_by);
    match input.decision_reason {
        None => {}
        // A reason the need held before a supplier change in the same call
        // is no reason sent, so it goes with the shop.
        Some(Some(reason))
            if decision == Decision::Undecided
                && existing.decision_reason.as_deref().map(str::trim) == Some(reason.trim()) => {}
        Some(reason) => decision = decision.with_reason(reason)?,
    }
    let mut active: job_need::ActiveModel = existing.into();
    if let Some(name) = input.name {
        active.name = Set(name);
    }
    if let Some(quantity) = input.quantity {
        active.quantity = Set(quantity);
    }
    active.supplied_by = Set(supplied_by);
    if let Some(value) = input.approach {
        active.approach = Set(value);
    }
    decision.write(&mut active);
    active.updated_at = Set(super::now());
    Ok(active.update(db).await?)
}

/// What a need has decided. It is read from and written to the need's chosen
/// option, decision date and decision reason, which hold together as the
/// table's CHECK constraints require. `supplied_by` stays its own column: it
/// only tells `ShopSupplies` from `Undecided` when no option is chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    /// No option is chosen and the shop does not supply the need.
    Undecided,
    /// No option is chosen and the shop supplies the need, perhaps with the
    /// reason it does.
    ShopSupplies { reason: Option<String> },
    /// An option was chosen on `on`, whoever supplies the need.
    Chosen {
        option_id: i32,
        on: chrono::NaiveDate,
        reason: Option<String>,
    },
}

impl Decision {
    /// Reads a stored need.
    ///
    /// # Errors
    /// `Internal` for a row whose columns fit no decision.
    pub(crate) fn of(need: &job_need::Model) -> DomainResult<Self> {
        let reason = need.decision_reason.clone();
        match (need.chosen_option_id, need.decided_on) {
            (Some(option_id), Some(on)) => Ok(Self::Chosen {
                option_id,
                on,
                reason,
            }),
            (None, None) if need.supplied_by == Some(Supplier::Shop) => {
                Ok(Self::ShopSupplies { reason })
            }
            (None, None) if reason.is_none() => Ok(Self::Undecided),
            _ => Err(DomainError::Internal(format!(
                "need {} holds a choice, decision date or reason that do not fit together",
                need.id
            ))),
        }
    }

    // The decision once `supplied_by` supplies the need. A choice stays; with
    // none, the shop's reason goes when the shop stops supplying it.
    fn for_supplier(self, supplied_by: Option<Supplier>) -> Self {
        match self {
            Self::Chosen { .. } => self,
            _ if supplied_by != Some(Supplier::Shop) => Self::Undecided,
            Self::ShopSupplies { reason } => Self::ShopSupplies { reason },
            Self::Undecided => Self::ShopSupplies { reason: None },
        }
    }

    fn with_reason(self, reason: Option<String>) -> DomainResult<Self> {
        Ok(match self {
            Self::Chosen { option_id, on, .. } => Self::Chosen {
                option_id,
                on,
                reason,
            },
            Self::ShopSupplies { .. } => Self::ShopSupplies { reason },
            Self::Undecided if reason.is_none() => Self::Undecided,
            Self::Undecided => return Err(reason_refused()),
        })
    }

    /// Sets the need's chosen option, decision date and decision reason.
    pub(crate) fn write(self, active: &mut job_need::ActiveModel) {
        let (option_id, on, reason) = match self {
            Self::Undecided => (None, None, None),
            Self::ShopSupplies { reason } => (None, None, reason),
            Self::Chosen {
                option_id,
                on,
                reason,
            } => (Some(option_id), Some(on), reason),
        };
        active.chosen_option_id = Set(option_id);
        active.decided_on = Set(on);
        active.decision_reason = Set(reason);
    }
}

fn reason_refused() -> DomainError {
    DomainError::invalid(
        "decision_reason",
        "needs a chosen option unless the shop supplies the need; choose an option first, or \
         clear the reason",
    )
}

/// Deletes a need and, through the database, its options.
///
/// # Errors
/// `NotFound` for a need that is not the vehicle's.
pub async fn delete<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
) -> DomainResult<()> {
    let txn = super::begin_write(db).await?;
    let db = &txn;
    require(db, vehicle_id, need_id).await?.delete(db).await?;
    txn.commit().await?;
    Ok(())
}

/// Adds a researched option to a need.
///
/// # Errors
/// `NotFound` for a need that is not the vehicle's; `Invalid` for a blank
/// description or a negative price.
pub async fn add_option<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
    input: NewOption,
) -> DomainResult<need_option::Model> {
    super::validate_not_blank("description", &input.description)?;
    validate_price(input.price_cents)?;
    let txn = super::begin_write(db).await?;
    let db = &txn;
    require(db, vehicle_id, need_id).await?;
    let model = need_option::ActiveModel {
        need_id: Set(need_id),
        description: Set(input.description),
        manufacturer: Set(input.manufacturer.as_deref().and_then(super::trim_to_none)),
        part_number: Set(input.part_number.as_deref().and_then(super::trim_to_none)),
        seller: Set(input.seller.as_deref().and_then(super::trim_to_none)),
        price_cents: Set(input.price_cents),
        url: Set(input.url.as_deref().and_then(super::trim_to_none)),
        fitment: Set(input.fitment.unwrap_or(Fitment::Unconfirmed)),
        fitment_note: Set(input.fitment_note.as_deref().and_then(super::trim_to_none)),
        ..Default::default()
    };
    let out = model.insert(db).await?;
    txn.commit().await?;
    Ok(out)
}

// The reason and date a need gave its choice, which an option keeps after
// that choice is ruled out.
type KeptChoice = (Option<String>, chrono::NaiveDate);

// Takes the choice off the need that has chosen `option` and returns the
// reason and date it held. A need that chose another option, or none, is left
// alone.
async fn unchoose(
    db: &impl ConnectionTrait,
    option: &need_option::Model,
) -> DomainResult<Option<KeptChoice>> {
    let Some(need) = job_need::Entity::find_by_id(option.need_id)
        .filter(job_need::Column::ChosenOptionId.eq(option.id))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    let Decision::Chosen { on, reason, .. } = Decision::of(&need)? else {
        return Ok(None);
    };
    let supplied_by = need.supplied_by;
    let mut active: job_need::ActiveModel = need.into();
    Decision::Undecided
        .for_supplier(supplied_by)
        .write(&mut active);
    active.updated_at = Set(super::now());
    active.update(db).await?;
    Ok(Some((reason, on)))
}

// Sets the choice an option keeps, or clears it for `None`. The table keeps a
// kept reason only with its date.
fn keep_choice(active: &mut need_option::ActiveModel, kept: Option<KeptChoice>) {
    let (reason, on) = kept.map_or((None, None), |(reason, on)| (reason, Some(on)));
    active.chosen_reason = Set(reason);
    active.chosen_on = Set(on);
}

/// Changes an option. Rejecting or marking `does_not_fit` the option its need
/// has chosen clears that choice, its date and its reason in the same
/// transaction, and keeps the reason and date on the option as
/// `chosen_reason` and `chosen_on`.
///
/// # Errors
/// `NotFound` for an option that is not the vehicle's; `Invalid` as for
/// [`add_option`], and for a blank rejection reason.
pub async fn update_option<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    option_id: i32,
    input: UpdateOption,
) -> DomainResult<need_option::Model> {
    let txn = super::begin_write(db).await?;
    let db = &txn;
    let existing = require_option(db, vehicle_id, option_id).await?;
    if let Some(description) = &input.description {
        super::validate_not_blank("description", description)?;
    }
    if let Some(price) = input.price_cents {
        validate_price(price)?;
    }
    // A caller may send "" where it means no reason. Storing that would reject
    // the option and clear the need's choice, so only real text rejects.
    if let Some(Some(reason)) = &input.rejected_reason
        && reason.trim().is_empty()
    {
        return Err(DomainError::invalid(
            "rejected_reason",
            "must not be blank; clear the reason to un-reject the option",
        ));
    }
    // Ruling out the chosen option takes the choice off the need in the same
    // transaction, so a need never keeps a choice that was just rejected.
    let now_unusable = input.fitment == Some(Fitment::DoesNotFit)
        || input.rejected_reason.as_ref().is_some_and(Option::is_some);
    let kept = if now_unusable {
        unchoose(db, &existing).await?
    } else {
        None
    };
    let mut active: need_option::ActiveModel = existing.into();
    if kept.is_some() {
        keep_choice(&mut active, kept);
    }
    if let Some(value) = input.description {
        active.description = Set(value);
    }
    if let Some(value) = input.manufacturer {
        active.manufacturer = Set(value.as_deref().and_then(super::trim_to_none));
    }
    if let Some(value) = input.part_number {
        active.part_number = Set(value.as_deref().and_then(super::trim_to_none));
    }
    if let Some(value) = input.seller {
        active.seller = Set(value.as_deref().and_then(super::trim_to_none));
    }
    if let Some(value) = input.price_cents {
        active.price_cents = Set(value);
    }
    if let Some(value) = input.url {
        active.url = Set(value.as_deref().and_then(super::trim_to_none));
    }
    if let Some(value) = input.fitment {
        active.fitment = Set(value);
    }
    if let Some(value) = input.fitment_note {
        active.fitment_note = Set(value.as_deref().and_then(super::trim_to_none));
    }
    if let Some(reason) = &input.rejected_reason {
        active.rejected_on = Set(reason.is_some().then(crate::services::today));
        active.rejected_reason = Set(reason.clone());
    }
    active.updated_at = Set(super::now());
    let updated = active.update(db).await?;
    txn.commit().await?;
    Ok(updated)
}

/// Deletes an option. A need that had chosen it is left with no choice, no
/// decision date and no decision reason.
///
/// # Errors
/// `NotFound` for an option that is not the vehicle's.
pub async fn delete_option<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    option_id: i32,
) -> DomainResult<()> {
    let txn = super::begin_write(db).await?;
    let option = require_option(&txn, vehicle_id, option_id).await?;
    // The database refuses to delete an option a need has chosen.
    unchoose(&txn, &option).await?;
    option.delete(&txn).await?;
    txn.commit().await?;
    Ok(())
}

/// Changes a need and, when `choice` is present, chooses an option for it (or
/// clears the choice for `Some(None)`) in one transaction. The reason stored
/// with a choice is `input.decision_reason`, and it is cleared when that field
/// is absent or `None`; without a `choice`, `decision_reason` edits only the
/// reason. `Some(None)` on a need with no chosen option changes no choice, so
/// with `decision_reason` absent it keeps the stored reason. A refused choice
/// leaves every other change unapplied.
///
/// # Errors
/// `NotFound` for a need that is not the vehicle's; `Invalid` as for
/// [`update`] and [`decide`]; `BadRequest` as for [`decide`].
pub async fn update_with_choice<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
    mut input: UpdateNeed,
    choice: Option<Option<i32>>,
) -> DomainResult<job_need::Model> {
    let Some(option_id) = choice else {
        return update(db, vehicle_id, need_id, input).await;
    };
    // `decide` writes the reason itself, so the edit leaves it alone.
    let reason = input.decision_reason.take();
    let txn = super::begin_write(db).await?;
    let updated = apply_update(&txn, vehicle_id, need_id, input).await?;
    if option_id.is_none() && reason.is_none() && updated.chosen_option_id.is_none() {
        txn.commit().await?;
        return Ok(updated);
    }
    let decided = decide(&txn, vehicle_id, need_id, option_id, reason.flatten()).await?;
    txn.commit().await?;
    Ok(decided)
}

/// Chooses an option for a need, or clears the choice when `option_id` is
/// `None`, and records the reason in the same write. Choosing a different
/// option stamps `decided_on` with today's date, choosing the option already
/// chosen keeps the date and replaces only the reason, and clearing removes
/// the date. Choosing an option clears the `chosen_reason` and `chosen_on` it
/// kept from an earlier choice, all in one transaction.
///
/// # Errors
/// `NotFound` for a need that is not the vehicle's; `BadRequest` for an
/// option of another need, one marked `does_not_fit` or a rejected one;
/// `Invalid` on `decision_reason` for a reason sent without an option, unless
/// the shop supplies the need.
pub async fn decide<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
    option_id: Option<i32>,
    reason: Option<String>,
) -> DomainResult<job_need::Model> {
    decide_on(db, vehicle_id, need_id, option_id, reason, super::today()).await
}

/// [`decide`] for a choice made on `today`, which is the date a new choice
/// is stamped with.
///
/// # Errors
/// As for [`decide`].
pub async fn decide_on<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
    option_id: Option<i32>,
    reason: Option<String>,
    today: chrono::NaiveDate,
) -> DomainResult<job_need::Model> {
    let txn = super::begin_write(db).await?;
    let db = &txn;
    let need = require(db, vehicle_id, need_id).await?;
    let current = Decision::of(&need)?;
    let decision = match option_id {
        None => Decision::Undecided
            .for_supplier(need.supplied_by)
            .with_reason(reason)?,
        Some(option_id) => {
            let option = choosable(db, need_id, option_id).await?;
            if option.chosen_on.is_some() {
                let mut kept: need_option::ActiveModel = option.into();
                keep_choice(&mut kept, None);
                kept.updated_at = Set(super::now());
                kept.update(db).await?;
            }
            let on = match current {
                Decision::Chosen {
                    option_id: kept,
                    on,
                    ..
                } if kept == option_id => on,
                _ => today,
            };
            Decision::Chosen {
                option_id,
                on,
                reason,
            }
        }
    };
    let mut active: job_need::ActiveModel = need.into();
    decision.write(&mut active);
    active.updated_at = Set(super::now());
    let out = active.update(db).await?;
    txn.commit().await?;
    Ok(out)
}

// Loads `option_id` when the need may choose it: it is the need's, it fits
// and it is not rejected.
async fn choosable(
    db: &impl ConnectionTrait,
    need_id: i32,
    option_id: i32,
) -> DomainResult<need_option::Model> {
    let option = need_option::Entity::find_by_id(option_id)
        .filter(need_option::Column::NeedId.eq(need_id))
        .one(db)
        .await?
        .ok_or_else(|| {
            DomainError::BadRequest(format!(
                "Option {option_id} does not belong to need {need_id}"
            ))
        })?;
    if option.fitment == Fitment::DoesNotFit {
        return Err(DomainError::BadRequest(format!(
            "Option {option_id} does not fit; change its fitment first"
        )));
    }
    if is_rejected(&option) {
        return Err(DomainError::BadRequest(format!(
            "Option {option_id} is rejected; clear its rejection first"
        )));
    }
    Ok(option)
}

/// A need with the part linked to it and the stage that results.
#[derive(Serialize, Debug)]
pub struct NeedPurchase {
    pub need: job_need::Model,
    /// The linked part, with its kit contents.
    pub part: super::part::PartView,
    pub stage: Stage,
}

/// What returning a part leaves behind: the need (unlinked, possibly with its
/// choice cleared), the returned part, the to-do to take it back and the
/// need's new stage.
#[derive(Serialize, Debug)]
pub struct ReturnOutcome {
    pub need: job_need::Model,
    /// The returned part, with its kit contents.
    pub returned_part: super::part::PartView,
    pub todo: todo::Model,
    pub stage: Stage,
}

impl Stage {
    /// The stage as the lowercase word the tools and pages use, such as
    /// `to_order`.
    pub fn label(self) -> &'static str {
        match self {
            Stage::Undecided => "undecided",
            Stage::ToOrder => "to_order",
            Stage::ShopSupplies => "shop_supplies",
            Stage::Ordered => "ordered",
            Stage::Received => "received",
            Stage::Installed => "installed",
        }
    }
}

// Loads the part linked to `need`, or refuses when the need has none.
async fn linked_part(
    db: &impl ConnectionTrait,
    need: &job_need::Model,
) -> DomainResult<part::Model> {
    let Some(part_id) = need.part_id else {
        return Err(DomainError::BadRequest(format!(
            "Need {} has no part linked",
            need.id
        )));
    };
    part::Entity::find_by_id(part_id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::Internal(format!("Need {} links a missing part", need.id)))
}

async fn set_need_part(
    db: &impl ConnectionTrait,
    need: job_need::Model,
    part_id: Option<i32>,
    supplied_by: Option<Supplier>,
) -> DomainResult<job_need::Model> {
    let mut active: job_need::ActiveModel = need.into();
    active.part_id = Set(part_id);
    if let Some(supplier) = supplied_by {
        active.supplied_by = Set(Some(supplier));
    }
    active.updated_at = Set(super::now());
    Ok(active.update(db).await?)
}

fn purchase(need: job_need::Model, part: super::part::PartView) -> NeedPurchase {
    let stage = stage(&need, Some(&part.part));
    NeedPurchase { need, part, stage }
}

/// The price of buying `quantity` of the option for `need`: the option's
/// unit price times the need's quantity, or `None` when the option has no
/// price or the product does not fit in an `i32`.
#[must_use]
pub fn order_cost_cents(need: &job_need::Model, option: &need_option::Model) -> Option<i32> {
    option
        .price_cents
        .and_then(|unit| unit.checked_mul(need.quantity))
}

/// Creates the part a need's chosen option describes, owned by the owner
/// (`supplied_by` `self`), and links it to the need. A field in `details`
/// wins over the option's value; the cost defaults to [`order_cost_cents`]
/// and the purchase date to today.
///
/// # Errors
/// `NotFound` for a need that is not the vehicle's; `BadRequest` when the
/// need has no chosen option, already has a part or is supplied by the shop;
/// `Invalid` for a negative cost or for a kit item with a blank name or a
/// quantity below 1.
pub async fn mark_ordered<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
    details: OrderDetails,
) -> DomainResult<NeedPurchase> {
    validate_price(details.cost_cents)?;
    super::part::validate_contents("contents", &details.contents)?;
    let txn = super::begin_write(db).await?;
    // Only the first of two orders for one need links a part: the second
    // finds the part linked and its conditional write changes nothing.
    let linked = job_need::Entity::update_many()
        .col_expr(job_need::Column::UpdatedAt, Expr::value(super::now()))
        .filter(job_need::Column::Id.eq(need_id))
        .filter(job_need::Column::PartId.is_null())
        .exec(&txn)
        .await?
        .rows_affected;
    let need = require(&txn, vehicle_id, need_id).await?;
    if linked == 0 {
        return Err(DomainError::BadRequest(format!(
            "Need {need_id} already has a part linked"
        )));
    }
    if need.supplied_by == Some(Supplier::Shop) {
        return Err(DomainError::BadRequest(format!(
            "Need {need_id} is supplied by the shop, so there is nothing to order"
        )));
    }
    let option = match need.chosen_option_id {
        Some(option_id) => need_option::Entity::find_by_id(option_id).one(&txn).await?,
        None => None,
    }
    .ok_or_else(|| {
        DomainError::BadRequest(format!(
            "Need {need_id} has no chosen option; choose one before ordering"
        ))
    })?;
    let purchase_date = details.purchase_date.unwrap_or_else(crate::services::today);
    let cost_cents = details.cost_cents.or(order_cost_cents(&need, &option));
    let created = super::part::record(
        &txn,
        vehicle_id,
        NewPart {
            name: option.description,
            manufacturer: option.manufacturer,
            part_number: option.part_number,
            seller: details.seller.or(option.seller),
            purchase_date: Some(purchase_date),
            cost_cents,
            retailer_url: option.url,
            supplied_by: Some(Supplier::Owner),
            order_ref: details.order_ref,
            contents: details.contents,
            ..Default::default()
        },
    )
    .await?;
    let need = set_need_part(&txn, need, Some(created.part.id), Some(Supplier::Owner)).await?;
    txn.commit().await?;
    Ok(purchase(need, created))
}

/// Links a part that is already recorded to a need, optionally stamping the
/// date it arrived. A need with no supplier takes the part's.
///
/// # Errors
/// `NotFound` for a need or part that is not the vehicle's; `BadRequest` for
/// a returned part, a part linked to another need, a need that already has
/// another part, or an arrival date given for an installed or replaced part;
/// `Invalid` for an arrival date before the part's purchase date.
pub async fn link_part<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
    part_id: i32,
    received_on: Option<chrono::NaiveDate>,
) -> DomainResult<NeedPurchase> {
    let txn = super::begin_write(db).await?;
    let need = require(&txn, vehicle_id, need_id).await?;
    let found = super::part::get(&txn, vehicle_id, part_id).await?;
    if super::part::is_returned(&found) {
        return Err(DomainError::BadRequest(format!(
            "Part {part_id} was returned and cannot be linked"
        )));
    }
    if let Some(other) = job_need::Entity::find()
        .filter(job_need::Column::PartId.eq(part_id))
        .filter(job_need::Column::Id.ne(need_id))
        .one(&txn)
        .await?
    {
        return Err(DomainError::BadRequest(format!(
            "Part {part_id} is already linked to need {}",
            other.id
        )));
    }
    if let Some(current) = need.part_id.filter(|current| *current != part_id) {
        return Err(DomainError::BadRequest(format!(
            "Need {need_id} already has part {current} linked; unlink it first"
        )));
    }
    if received_on.is_some() {
        require_unfitted(&found, PartAction::MarkReceived)?;
    }
    let supplier = need.supplied_by.is_none().then_some(found.supplied_by);
    let found = match received_on {
        Some(date) => {
            let purchase_date = found.purchase_date;
            let mut active: part::ActiveModel = found.into();
            super::part::set_received(&mut active, Some(date), purchase_date)?;
            active.updated_at = Set(super::now());
            active.update(&txn).await?
        }
        None => found,
    };
    let need = set_need_part(&txn, need, Some(part_id), supplier).await?;
    let found = super::part::view(&txn, found).await?;
    txn.commit().await?;
    Ok(purchase(need, found))
}

/// What [`set_part`] did: the need with its newly linked part, or the need
/// with no part.
#[derive(Serialize, Debug)]
#[serde(untagged)]
pub enum PartLinkOutcome {
    Linked(Box<NeedPurchase>),
    Unlinked(job_need::Model),
}

/// The part link that `part_id` and an optional date describe. `part_id` is
/// `None` when no part and no unlink was asked for, `Some(None)` to unlink,
/// and `Some(Some(id))` to link that part.
///
/// # Errors
/// `Invalid` on `part_id` when it is `None`, and on `received_on` for a date
/// given with an unlink.
pub fn part_link(
    part_id: Option<Option<i32>>,
    received_on: Option<chrono::NaiveDate>,
) -> DomainResult<PartLink> {
    match (part_id, received_on) {
        (None, _) => Err(DomainError::invalid(
            "part_id",
            "is required: name a part to link, or unlink the need's current part",
        )),
        (Some(None), Some(_)) => Err(DomainError::invalid(
            "received_on",
            "cannot be given when unlinking a part",
        )),
        (Some(None), None) => Ok(PartLink::Unlink),
        (Some(Some(part_id)), received_on) => Ok(PartLink::Link {
            part_id,
            received_on,
        }),
    }
}

/// Applies `link` through [`link_part`] or [`unlink_part`].
///
/// # Errors
/// As for [`link_part`] and [`unlink_part`].
pub async fn set_part<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
    link: PartLink,
) -> DomainResult<PartLinkOutcome> {
    match link {
        PartLink::Link {
            part_id,
            received_on,
        } => link_part(db, vehicle_id, need_id, part_id, received_on)
            .await
            .map(|purchase| PartLinkOutcome::Linked(Box::new(purchase))),
        PartLink::Unlink => unlink_part(db, vehicle_id, need_id)
            .await
            .map(PartLinkOutcome::Unlinked),
    }
}

/// Detaches a need's part without changing the part.
///
/// # Errors
/// `NotFound` for a need that is not the vehicle's; `BadRequest` when it has
/// no part.
pub async fn unlink_part<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
) -> DomainResult<job_need::Model> {
    let txn = super::begin_write(db).await?;
    let need = require(&txn, vehicle_id, need_id).await?;
    linked_part(&txn, &need).await?;
    let need = set_need_part(&txn, need, None, None).await?;
    txn.commit().await?;
    Ok(need)
}

// What a caller of `require_unfitted` is about to do with a part.
#[derive(Clone, Copy)]
enum PartAction {
    MarkReceived,
    ClearReceived,
    Return,
}

// Refuses a part that has been on the car (installed or replaced). For a
// need's linked part this is the same test as `Stage::has_unfitted_part`,
// because `stage` reads such a part as `installed`.
fn require_unfitted(found: &part::Model, action: PartAction) -> DomainResult<()> {
    if !super::part::was_fitted(found) {
        return Ok(());
    }
    let what = match action {
        PartAction::MarkReceived => "be marked received",
        PartAction::ClearReceived => "have its arrival date cleared",
        PartAction::Return => "be returned",
    };
    Err(DomainError::BadRequest(format!(
        "Part {} is {}; only an ordered or received part can {what}",
        found.id, found.status
    )))
}

// Writes the date `date_for` picks for the need's part, after the
// fitted-part check. `date_for` sees the stored part and may refuse it.
async fn set_received<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
    action: PartAction,
    date_for: impl FnOnce(&part::Model) -> DomainResult<Option<chrono::NaiveDate>>,
) -> DomainResult<NeedPurchase> {
    let txn = super::begin_write(db).await?;
    let need = require(&txn, vehicle_id, need_id).await?;
    let found = linked_part(&txn, &need).await?;
    require_unfitted(&found, action)?;
    let on = date_for(&found)?;
    let purchase_date = found.purchase_date;
    let mut active: part::ActiveModel = found.into();
    super::part::set_received(&mut active, on, purchase_date)?;
    active.updated_at = Set(super::now());
    let found = super::part::view(&txn, active.update(&txn).await?).await?;
    txn.commit().await?;
    Ok(purchase(need, found))
}

/// Records the day a need's part arrived, today when `on` is `None`. A part
/// that already has a date takes a new one only when `on` names it.
///
/// # Errors
/// `NotFound` for a need that is not the vehicle's; `BadRequest` when it has
/// no part, its part is installed or replaced, or `on` is `None` and the
/// part already has a date; `Invalid` for a date before the part's purchase
/// date.
pub async fn mark_received<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
    on: Option<chrono::NaiveDate>,
) -> DomainResult<NeedPurchase> {
    set_received(
        db,
        vehicle_id,
        need_id,
        PartAction::MarkReceived,
        |found| match (on, found.received_on) {
            (Some(date), _) => Ok(Some(date)),
            (None, None) => Ok(Some(crate::services::today())),
            (None, Some(stored)) => Err(DomainError::BadRequest(format!(
                "Need {need_id}'s part was already received on {stored}"
            ))),
        },
    )
    .await
}

/// Clears the arrival date of a need's part, so the need reads `ordered`
/// again.
///
/// # Errors
/// `NotFound` for a need that is not the vehicle's; `BadRequest` when it has
/// no part or its part is installed or replaced.
pub async fn clear_received<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
) -> DomainResult<NeedPurchase> {
    set_received(db, vehicle_id, need_id, PartAction::ClearReceived, |_| {
        Ok(None)
    })
    .await
}

/// The arrival change that `received` and an optional date describe:
/// `received` with the date, or a cleared date. A `None` date is resolved by
/// [`mark_received`]: today for a part with no arrival date, and a refusal
/// for a part that has one.
///
/// # Errors
/// `Invalid` on `received_on` for a date given with `received` false.
pub fn arrival(
    received: bool,
    received_on: Option<chrono::NaiveDate>,
) -> DomainResult<ArrivalChange> {
    match (received, received_on) {
        (true, on) => Ok(ArrivalChange::Received(on)),
        (false, None) => Ok(ArrivalChange::NotReceived),
        (false, Some(_)) => Err(DomainError::invalid(
            "received_on",
            "cannot be given when clearing the arrival date",
        )),
    }
}

/// Applies `change` through [`mark_received`] or [`clear_received`].
///
/// # Errors
/// As for [`mark_received`] and [`clear_received`].
pub async fn set_arrival<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
    change: ArrivalChange,
) -> DomainResult<NeedPurchase> {
    match change {
        ArrivalChange::Received(on) => mark_received(db, vehicle_id, need_id, on).await,
        ArrivalChange::NotReceived => clear_received(db, vehicle_id, need_id).await,
    }
}

/// Sends back the part of an `ordered` or `received` need. In one
/// transaction the part becomes `returned` with the reason and today's date,
/// the need loses the part (and, when `option_was_wrong`, its chosen option,
/// which is rejected for the same reason; the flag has no effect when the
/// need has no chosen option), and a to-do to take the part back
/// goes on the job's appointment while that appointment is open. The to-do
/// goes on the vehicle when the job has no appointment or its appointment is
/// closed or cancelled, because no list shows the to-dos of those.
///
/// # Errors
/// `NotFound` for a need that is not the vehicle's; `BadRequest` when the
/// need has no part linked or its part is installed or replaced; `Invalid`
/// for a blank reason.
pub async fn return_part<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
    input: ReturnInput,
) -> DomainResult<ReturnOutcome> {
    return_part_on(db, vehicle_id, need_id, input, super::today()).await
}

/// [`return_part`] for a part sent back on `today`, which is the date the
/// part's return is stamped with.
///
/// # Errors
/// As for [`return_part`].
pub async fn return_part_on<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
    input: ReturnInput,
    today: chrono::NaiveDate,
) -> DomainResult<ReturnOutcome> {
    super::validate_not_blank("reason", &input.reason)?;
    let txn = super::begin_write(db).await?;
    let need = require(&txn, vehicle_id, need_id).await?;
    let found = linked_part(&txn, &need).await?;
    require_unfitted(&found, PartAction::Return)?;
    let job = super::work_item::get(&txn, vehicle_id, need.work_item_id).await?;
    let appointment_id = super::appointment::open_id(&txn, vehicle_id, job.appointment_id).await?;
    let title = match found.seller.as_deref().and_then(super::trim_to_none) {
        Some(seller) => format!("Return {} to {seller}", found.name),
        None => format!("Return {}", found.name),
    };
    let chosen_option_id = need.chosen_option_id;

    let returned_part =
        super::part::mark_returned(&txn, found, input.reason.clone(), need_id, today).await?;
    let returned_part = super::part::view(&txn, returned_part).await?;
    set_need_part(&txn, need, None, None).await?;
    if let Some(option_id) = chosen_option_id.filter(|_| input.option_was_wrong) {
        // The rejection goes through the option update so the need's whole
        // choice clears the same way it does for any rejected option.
        update_option(
            &txn,
            vehicle_id,
            option_id,
            UpdateOption {
                rejected_reason: Some(Some(input.reason)),
                ..Default::default()
            },
        )
        .await?;
    }
    let todo = super::todo::insert(
        &txn,
        vehicle_id,
        NewTodo {
            title,
            appointment_id,
            ..Default::default()
        },
    )
    .await?;
    let need = require(&txn, vehicle_id, need_id).await?;
    txn.commit().await?;
    let stage = stage(&need, None);
    Ok(ReturnOutcome {
        need,
        returned_part,
        todo,
        stage,
    })
}

#[cfg(test)]
mod tests;
