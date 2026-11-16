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
pub fn counts_as_open(job_status: JobStatus, stage: Stage) -> bool {
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
    option.rejected_reason.is_some() || option.rejected_on.is_some()
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
pub async fn add(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    work_item_id: i32,
    input: NewNeed,
) -> DomainResult<job_need::Model> {
    super::validate_not_blank("name", &input.name)?;
    let quantity = input.quantity.unwrap_or(1);
    validate_quantity(quantity)?;
    super::work_item::get(db, vehicle_id, work_item_id).await?;
    let model = job_need::ActiveModel {
        work_item_id: Set(work_item_id),
        name: Set(input.name),
        quantity: Set(quantity),
        supplied_by: Set(input.supplied_by),
        approach: Set(input.approach),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

/// Changes a need's name, quantity, supplier, approach or decision reason. A
/// need with no chosen option holds a reason only while the shop supplies it,
/// judged with the supplier this call leaves: such a reason is refused, and
/// moving a need with no chosen option away from `shop` clears its stored
/// reason. Clearing a reason is always allowed.
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
    let txn = db.begin().await?;
    claim(&txn, need_id).await?;
    let updated = apply_update(&txn, vehicle_id, need_id, input).await?;
    txn.commit().await?;
    Ok(updated)
}

// The body of `update`, run inside a transaction that has claimed the need,
// so the checks below read the committed row and nothing changes it before
// the write.
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
    let can_hold = holds_reason(supplied_by, existing.chosen_option_id);
    if matches!(input.decision_reason, Some(Some(_))) && !can_hold {
        return Err(reason_refused());
    }
    // Only a real change of supplier clears, because the web supplier form
    // resends the stored supplier with every approach edit.
    let clears_reason = input.decision_reason.is_none()
        && existing.decision_reason.is_some()
        && existing.supplied_by == Some(Supplier::Shop)
        && supplied_by != existing.supplied_by
        && existing.chosen_option_id.is_none();
    let mut active: job_need::ActiveModel = existing.into();
    if let Some(name) = input.name {
        active.name = Set(name);
    }
    if let Some(quantity) = input.quantity {
        active.quantity = Set(quantity);
    }
    if let Some(value) = input.supplied_by {
        active.supplied_by = Set(value);
    }
    if let Some(value) = input.approach {
        active.approach = Set(value);
    }
    if let Some(value) = input.decision_reason {
        active.decision_reason = Set(value);
    } else if clears_reason {
        // A reason with no chosen option explains why the shop supplies the
        // need, so it goes when the shop stops supplying it, the way a
        // cleared choice takes its reason along.
        active.decision_reason = Set(None);
    }
    active.updated_at = Set(super::now());
    Ok(active.update(db).await?)
}

// Whether a need with this supplier and chosen option may hold a decision
// reason: one with a chosen option always may, and one with none only while
// the shop supplies it.
fn holds_reason(supplied_by: Option<Supplier>, chosen_option_id: Option<i32>) -> bool {
    chosen_option_id.is_some() || supplied_by == Some(Supplier::Shop)
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
pub async fn delete(db: &impl ConnectionTrait, vehicle_id: i32, need_id: i32) -> DomainResult<()> {
    require(db, vehicle_id, need_id).await?.delete(db).await?;
    Ok(())
}

/// Adds a researched option to a need.
///
/// # Errors
/// `NotFound` for a need that is not the vehicle's; `Invalid` for a blank
/// description or a negative price.
pub async fn add_option(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    need_id: i32,
    input: NewOption,
) -> DomainResult<need_option::Model> {
    super::validate_not_blank("description", &input.description)?;
    validate_price(input.price_cents)?;
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
    Ok(model.insert(db).await?)
}

// Clears the need's choice, the date it was made and the reason given for it
// when the need has chosen `option_id`. The database nulls the reference when
// an option row goes, but it cannot clear the other two.
async fn clear_choice(db: &impl ConnectionTrait, need_id: i32, option_id: i32) -> DomainResult<()> {
    job_need::Entity::update_many()
        .col_expr(job_need::Column::ChosenOptionId, Expr::value(None::<i32>))
        .col_expr(
            job_need::Column::DecidedOn,
            Expr::value(None::<chrono::NaiveDate>),
        )
        .col_expr(
            job_need::Column::DecisionReason,
            Expr::value(None::<String>),
        )
        .col_expr(job_need::Column::UpdatedAt, Expr::value(super::now()))
        .filter(job_need::Column::Id.eq(need_id))
        .filter(job_need::Column::ChosenOptionId.eq(option_id))
        .exec(db)
        .await?;
    Ok(())
}

// Copies the reason and date of the need's choice onto the option when the
// need has chosen `option_id`, so clearing the choice keeps the owner's
// reasoning for past decisions.
async fn keep_choice_on_option(
    db: &impl ConnectionTrait,
    need_id: i32,
    option_id: i32,
) -> DomainResult<()> {
    let Some(need) = job_need::Entity::find_by_id(need_id)
        .filter(job_need::Column::ChosenOptionId.eq(option_id))
        .one(db)
        .await?
    else {
        return Ok(());
    };
    need_option::Entity::update_many()
        .col_expr(
            need_option::Column::ChosenReason,
            Expr::value(need.decision_reason),
        )
        .col_expr(need_option::Column::ChosenOn, Expr::value(need.decided_on))
        .filter(need_option::Column::Id.eq(option_id))
        .exec(db)
        .await?;
    Ok(())
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
    let mut active: need_option::ActiveModel = existing.clone().into();
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

    let now_unusable = input.fitment == Some(Fitment::DoesNotFit)
        || input.rejected_reason.as_ref().is_some_and(Option::is_some);
    // The option write and the choice clearing commit together, so a need
    // never keeps a choice that was just rejected.
    let txn = db.begin().await?;
    let mut updated = active.update(&txn).await?;
    if now_unusable {
        keep_choice_on_option(&txn, updated.need_id, updated.id).await?;
        clear_choice(&txn, updated.need_id, updated.id).await?;
        updated = require_option(&txn, vehicle_id, option_id).await?;
    }
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
    let option = require_option(db, vehicle_id, option_id).await?;
    let txn = db.begin().await?;
    clear_choice(&txn, option.need_id, option.id).await?;
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
    let txn = db.begin().await?;
    claim(&txn, need_id).await?;
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
/// kept from an earlier choice. The writes are atomic only inside a
/// transaction the caller holds, as [`update_with_choice`] does.
///
/// # Errors
/// `NotFound` for a need that is not the vehicle's; `BadRequest` for an
/// option of another need, one marked `does_not_fit` or a rejected one;
/// `Invalid` on `decision_reason` for a reason sent without an option, unless
/// the shop supplies the need.
pub async fn decide(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    need_id: i32,
    option_id: Option<i32>,
    reason: Option<String>,
) -> DomainResult<job_need::Model> {
    let need = require(db, vehicle_id, need_id).await?;
    // The choice this call makes decides, never the stored one.
    if reason.is_some() && !holds_reason(need.supplied_by, option_id) {
        return Err(reason_refused());
    }
    let mut has_history = false;
    if let Some(option_id) = option_id {
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
        has_history = option.chosen_reason.is_some() || option.chosen_on.is_some();
    }
    let decided_on = if option_id.is_some() && option_id == need.chosen_option_id {
        need.decided_on
    } else {
        option_id.map(|_| crate::services::today())
    };
    let mut active: job_need::ActiveModel = need.into();
    active.chosen_option_id = Set(option_id);
    active.decision_reason = Set(reason);
    active.decided_on = Set(decided_on);
    active.updated_at = Set(super::now());
    if let Some(option_id) = option_id.filter(|_| has_history) {
        clear_choice_history(db, option_id).await?;
    }
    Ok(active.update(db).await?)
}

// Clears the reason and date an option kept from an earlier choice, which
// `keep_choice_on_option` copied there when that choice was ruled out.
async fn clear_choice_history(db: &impl ConnectionTrait, option_id: i32) -> DomainResult<()> {
    need_option::Entity::update_many()
        .col_expr(
            need_option::Column::ChosenReason,
            Expr::value(None::<String>),
        )
        .col_expr(
            need_option::Column::ChosenOn,
            Expr::value(None::<chrono::NaiveDate>),
        )
        .col_expr(need_option::Column::UpdatedAt, Expr::value(super::now()))
        .filter(need_option::Column::Id.eq(option_id))
        .exec(db)
        .await?;
    Ok(())
}

// Writes the need row without changing it, as `appointment::claim` does for
// an appointment. A write as the transaction's first statement takes SQLite's
// write lock, so a commit by the other program between this transaction's
// first read and its first write cannot fail it with a busy snapshot.
async fn claim(txn: &impl ConnectionTrait, need_id: i32) -> DomainResult<()> {
    job_need::Entity::update_many()
        .col_expr(
            job_need::Column::UpdatedAt,
            Expr::col(job_need::Column::UpdatedAt),
        )
        .filter(job_need::Column::Id.eq(need_id))
        .exec(txn)
        .await?;
    Ok(())
}

/// A need with the part linked to it and the stage that results.
#[derive(Serialize, Debug)]
pub struct NeedPurchase {
    pub need: job_need::Model,
    pub part: part::Model,
    pub stage: Stage,
}

/// What returning a part leaves behind: the need (unlinked, possibly with its
/// choice cleared), the returned part, the to-do to take it back and the
/// need's new stage.
#[derive(Serialize, Debug)]
pub struct ReturnOutcome {
    pub need: job_need::Model,
    pub returned_part: part::Model,
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

fn purchase(need: job_need::Model, part: part::Model) -> NeedPurchase {
    let stage = stage(&need, Some(&part));
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
/// `Invalid` for a negative cost.
pub async fn mark_ordered<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
    details: OrderDetails,
) -> DomainResult<NeedPurchase> {
    validate_price(details.cost_cents)?;
    let txn = db.begin().await?;
    // The claim is the transaction's first statement and a write, so SQLite
    // gives it the write lock (waiting out the busy timeout) before anything
    // is read. A second order for the same need then sees the first one's
    // part and claims nothing.
    let claimed = job_need::Entity::update_many()
        .col_expr(job_need::Column::UpdatedAt, Expr::value(super::now()))
        .filter(job_need::Column::Id.eq(need_id))
        .filter(job_need::Column::PartId.is_null())
        .exec(&txn)
        .await?
        .rows_affected;
    let need = require(&txn, vehicle_id, need_id).await?;
    if claimed == 0 {
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
    let created = super::part::create(
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
            ..Default::default()
        },
    )
    .await?;
    let need = set_need_part(&txn, need, Some(created.id), Some(Supplier::Owner)).await?;
    txn.commit().await?;
    Ok(purchase(need, created))
}

/// Links a part that is already recorded to a need, optionally stamping the
/// date it arrived. A need with no supplier takes the part's.
///
/// # Errors
/// `NotFound` for a need or part that is not the vehicle's; `BadRequest` for
/// a returned part, a part linked to another need, a need that already has
/// another part, or an arrival date given for an installed or replaced part.
pub async fn link_part<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
    part_id: i32,
    received_on: Option<chrono::NaiveDate>,
) -> DomainResult<NeedPurchase> {
    let need = require(db, vehicle_id, need_id).await?;
    let found = super::part::get(db, vehicle_id, part_id).await?;
    if super::part::is_returned(&found) {
        return Err(DomainError::BadRequest(format!(
            "Part {part_id} was returned and cannot be linked"
        )));
    }
    if let Some(other) = job_need::Entity::find()
        .filter(job_need::Column::PartId.eq(part_id))
        .filter(job_need::Column::Id.ne(need_id))
        .one(db)
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
        require_unfitted(need_id, stage(&need, Some(&found)), "be marked received")?;
    }
    let supplier = need.supplied_by.is_none().then_some(found.supplied_by);
    let txn = db.begin().await?;
    let found = match received_on {
        Some(date) => {
            let mut active: part::ActiveModel = found.into();
            active.received_on = Set(Some(date));
            active.updated_at = Set(super::now());
            active.update(&txn).await?
        }
        None => found,
    };
    let need = set_need_part(&txn, need, Some(part_id), supplier).await?;
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
pub async fn unlink_part(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    need_id: i32,
) -> DomainResult<job_need::Model> {
    let need = require(db, vehicle_id, need_id).await?;
    linked_part(db, &need).await?;
    set_need_part(db, need, None, None).await
}

// Refuses a need whose part is not `ordered` or `received`; `action` ends
// the sentence "only an ordered or received part can ...".
fn require_unfitted(need_id: i32, current: Stage, action: &str) -> DomainResult<()> {
    if current.has_unfitted_part() {
        return Ok(());
    }
    Err(DomainError::BadRequest(format!(
        "Need {need_id} is {}; only an ordered or received part can {action}",
        current.label()
    )))
}

async fn set_received<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
    on: Option<chrono::NaiveDate>,
    action: &str,
) -> DomainResult<NeedPurchase> {
    let txn = db.begin().await?;
    // The claim takes the write lock before the stage is read, so a part the
    // other program installs cannot slip in between the check and the write.
    claim(&txn, need_id).await?;
    let need = require(&txn, vehicle_id, need_id).await?;
    let found = linked_part(&txn, &need).await?;
    require_unfitted(need_id, stage(&need, Some(&found)), action)?;
    let mut active: part::ActiveModel = found.into();
    active.received_on = Set(on);
    active.updated_at = Set(super::now());
    let found = active.update(&txn).await?;
    txn.commit().await?;
    Ok(purchase(need, found))
}

/// Records the day a need's part arrived, today when `on` is `None`. A part
/// that already has a date takes the new one.
///
/// # Errors
/// `NotFound` for a need that is not the vehicle's; `BadRequest` when it has
/// no part or its part is installed or replaced.
pub async fn mark_received<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
    on: Option<chrono::NaiveDate>,
) -> DomainResult<NeedPurchase> {
    let on = on.unwrap_or_else(crate::services::today);
    set_received(db, vehicle_id, need_id, Some(on), "be marked received").await
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
    set_received(
        db,
        vehicle_id,
        need_id,
        None,
        "have its arrival date cleared",
    )
    .await
}

/// The arrival change that `received` and an optional date describe:
/// `received` with the date (today when it is `None`), or a cleared date.
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
/// need is not `ordered` or `received`; `Invalid` for a blank reason.
pub async fn return_part<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    need_id: i32,
    input: ReturnInput,
) -> DomainResult<ReturnOutcome> {
    super::validate_not_blank("reason", &input.reason)?;
    let need = require(db, vehicle_id, need_id).await?;
    let found = linked_part(db, &need).await?;
    require_unfitted(need_id, stage(&need, Some(&found)), "be returned")?;
    let job = super::work_item::get(db, vehicle_id, need.work_item_id).await?;
    let appointment_id = super::appointment::open_id(db, job.appointment_id).await?;
    let title = match found.seller.as_deref().and_then(super::trim_to_none) {
        Some(seller) => format!("Return {} to {seller}", found.name),
        None => format!("Return {}", found.name),
    };
    let chosen_option_id = need.chosen_option_id;

    let txn = db.begin().await?;
    let returned_part =
        super::part::mark_returned(&txn, found, input.reason.clone(), need_id).await?;
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
    let todo = super::todo::insert(&txn, vehicle_id, appointment_id, title).await?;
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
mod tests {
    use super::*;
    use crate::{
        entities::part::PartStatus,
        test_support::{VehicleFixture, add_job, date, row_count, test_db},
    };

    async fn seed_job(db: &DatabaseConnection) -> (i32, i32) {
        let vehicle_id = VehicleFixture::new().insert_id(db).await;
        let job = add_job(db, vehicle_id, "Rear brakes", None).await;
        (vehicle_id, job)
    }

    fn need(name: &str) -> NewNeed {
        NewNeed {
            name: name.into(),
            ..Default::default()
        }
    }

    fn option(description: &str) -> NewOption {
        NewOption {
            description: description.into(),
            ..Default::default()
        }
    }

    fn need_model(supplied_by: Option<Supplier>, chosen: Option<i32>) -> job_need::Model {
        job_need::Model {
            id: 1,
            work_item_id: 1,
            name: "Pads".into(),
            quantity: 1,
            supplied_by,
            approach: None,
            chosen_option_id: chosen,
            decision_reason: None,
            decided_on: None,
            part_id: None,
            created_at: chrono::DateTime::default(),
            updated_at: chrono::DateTime::default(),
        }
    }

    fn part_model(status: PartStatus, received_on: Option<&str>) -> part::Model {
        part::Model {
            id: 1,
            vehicle_id: 1,
            name: "Pads".into(),
            manufacturer: None,
            part_number: None,
            oe_part_number_replaced: None,
            seller: None,
            purchase_date: None,
            cost_cents: None,
            cost_currency: None,
            status,
            installed_date: None,
            installed_odometer: None,
            installed_service_id: None,
            replaced_date: None,
            replaced_odometer: None,
            notes: None,
            created_at: chrono::DateTime::default(),
            updated_at: chrono::DateTime::default(),
            manufacturer_url: None,
            retailer_url: None,
            location: None,
            warranty_expires_on: None,
            warranty_expires_miles: None,
            supplied_by: Supplier::Owner,
            received_on: received_on.map(date),
            returned_on: None,
            return_reason: None,
            order_ref: None,
            returned_from_need_id: None,
        }
    }

    async fn assert_choice_cleared(db: &DatabaseConnection, vehicle_id: i32, need_id: i32) {
        let n = require(db, vehicle_id, need_id).await.unwrap();
        assert_eq!(n.chosen_option_id, None);
        assert_eq!(n.decided_on, None);
        assert_eq!(n.decision_reason, None);
    }

    #[test]
    fn stage_table_first_match_wins() {
        let cases = [
            (need_model(None, None), None, Stage::Undecided),
            (
                need_model(Some(Supplier::Owner), None),
                None,
                Stage::Undecided,
            ),
            (
                need_model(Some(Supplier::Owner), Some(1)),
                None,
                Stage::ToOrder,
            ),
            (
                need_model(Some(Supplier::Shop), None),
                None,
                Stage::ShopSupplies,
            ),
            (
                need_model(Some(Supplier::Shop), Some(1)),
                None,
                Stage::ShopSupplies,
            ),
            (
                need_model(None, None),
                Some(part_model(PartStatus::Purchased, None)),
                Stage::Ordered,
            ),
            (
                need_model(Some(Supplier::Owner), Some(1)),
                Some(part_model(PartStatus::Purchased, None)),
                Stage::Ordered,
            ),
            (
                need_model(Some(Supplier::Owner), Some(1)),
                Some(part_model(PartStatus::Purchased, Some("2026-11-01"))),
                Stage::Received,
            ),
            (
                need_model(None, None),
                Some(part_model(PartStatus::Installed, Some("2026-11-01"))),
                Stage::Installed,
            ),
            (
                need_model(Some(Supplier::Shop), None),
                Some(part_model(PartStatus::Installed, None)),
                Stage::Installed,
            ),
            // A replaced part was fitted once, so it reads installed whether
            // or not its arrival was recorded.
            (
                need_model(Some(Supplier::Owner), Some(1)),
                Some(part_model(PartStatus::Replaced, Some("2026-11-01"))),
                Stage::Installed,
            ),
            (
                need_model(None, None),
                Some(part_model(PartStatus::Replaced, None)),
                Stage::Installed,
            ),
        ];
        for (need, part, expected) in cases {
            assert_eq!(stage(&need, part.as_ref()), expected, "{need:?} {part:?}");
        }
    }

    #[tokio::test]
    async fn add_rejects_zero_quantity() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let zero = NewNeed {
            quantity: Some(0),
            ..need("Pads")
        };
        let err = add(&db, vehicle_id, job_id, zero).await.unwrap_err();
        assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "quantity"));
        let added = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        assert_eq!(added.quantity, 1);
    }

    #[tokio::test]
    async fn decide_refuses_option_that_does_not_fit() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let o = add_option(
            &db,
            vehicle_id,
            n.id,
            NewOption {
                fitment: Some(Fitment::DoesNotFit),
                ..option("Wrong pads")
            },
        )
        .await
        .unwrap();
        let err = decide(&db, vehicle_id, n.id, Some(o.id), None)
            .await
            .unwrap_err();
        assert!(
            matches!(&err, DomainError::BadRequest(m) if m.contains("does not fit; change its fitment first"))
        );
    }

    #[tokio::test]
    async fn decide_refuses_rejected_option() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let o = add_option(&db, vehicle_id, n.id, option("Pads"))
            .await
            .unwrap();
        update_option(
            &db,
            vehicle_id,
            o.id,
            UpdateOption {
                rejected_reason: Some(Some("too loud".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let err = decide(&db, vehicle_id, n.id, Some(o.id), None)
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("Option {} is rejected; clear its rejection first", o.id)
        );
    }

    #[tokio::test]
    async fn decide_refuses_option_of_another_need() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let a = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let b = add(&db, vehicle_id, job_id, need("Rotors")).await.unwrap();
        let o = add_option(&db, vehicle_id, b.id, option("Rotors"))
            .await
            .unwrap();
        let err = decide(&db, vehicle_id, a.id, Some(o.id), None)
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)));
        let unchanged = require(&db, vehicle_id, a.id).await.unwrap();
        assert_eq!(unchanged.chosen_option_id, None);
    }

    #[tokio::test]
    async fn rejecting_the_chosen_option_clears_the_choice() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let o = add_option(&db, vehicle_id, n.id, option("Pads"))
            .await
            .unwrap();
        decide(&db, vehicle_id, n.id, Some(o.id), Some("cheap".into()))
            .await
            .unwrap();
        let rejected = update_option(
            &db,
            vehicle_id,
            o.id,
            UpdateOption {
                rejected_reason: Some(Some("too loud".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(rejected.rejected_on.is_some());
        assert_choice_cleared(&db, vehicle_id, n.id).await;

        let restored = update_option(
            &db,
            vehicle_id,
            o.id,
            UpdateOption {
                rejected_reason: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            (restored.rejected_reason, restored.rejected_on),
            (None, None)
        );
    }

    #[tokio::test]
    async fn marking_the_chosen_option_does_not_fit_clears_the_choice() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let o = add_option(&db, vehicle_id, n.id, option("Pads"))
            .await
            .unwrap();
        decide(&db, vehicle_id, n.id, Some(o.id), Some("cheap".into()))
            .await
            .unwrap();
        update_option(
            &db,
            vehicle_id,
            o.id,
            UpdateOption {
                fitment: Some(Fitment::DoesNotFit),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_choice_cleared(&db, vehicle_id, n.id).await;
    }

    #[tokio::test]
    async fn deleting_the_chosen_option_clears_the_choice() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let o = add_option(&db, vehicle_id, n.id, option("Pads"))
            .await
            .unwrap();
        decide(&db, vehicle_id, n.id, Some(o.id), Some("cheap".into()))
            .await
            .unwrap();
        delete_option(&db, vehicle_id, o.id).await.unwrap();
        assert_choice_cleared(&db, vehicle_id, n.id).await;
    }

    #[tokio::test]
    async fn deleting_a_chosen_option_nulls_the_reference_in_the_database() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let o = add_option(&db, vehicle_id, n.id, option("Pads"))
            .await
            .unwrap();
        decide(&db, vehicle_id, n.id, Some(o.id), None)
            .await
            .unwrap();
        need_option::Entity::delete_by_id(o.id)
            .exec(&db)
            .await
            .unwrap();
        let after = job_need::Entity::find_by_id(n.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.chosen_option_id, None);
    }

    #[tokio::test]
    async fn two_needs_cannot_share_one_part() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let a = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let b = add(&db, vehicle_id, job_id, need("Rotors")).await.unwrap();
        let part = crate::services::part::create(
            &db,
            vehicle_id,
            crate::inputs::part::NewPart {
                name: "Pads".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        for id in [a.id, b.id] {
            let link = job_need::ActiveModel {
                id: Unchanged(id),
                part_id: Set(Some(part.id)),
                ..Default::default()
            }
            .update(&db)
            .await;
            if id == a.id {
                link.unwrap();
            } else {
                assert!(link.is_err(), "a second need must not take the same part");
            }
        }
    }

    #[tokio::test]
    async fn decide_stamps_decided_on() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let o = add_option(&db, vehicle_id, n.id, option("Pads"))
            .await
            .unwrap();
        let decided = decide(
            &db,
            vehicle_id,
            n.id,
            Some(o.id),
            Some("best reviews".into()),
        )
        .await
        .unwrap();
        let today = crate::services::today();
        assert_eq!(decided.chosen_option_id, Some(o.id));
        assert_eq!(decided.decision_reason.as_deref(), Some("best reviews"));
        assert_eq!(decided.decided_on, Some(today));

        let cleared = decide(&db, vehicle_id, n.id, None, None).await.unwrap();
        assert_eq!(cleared.chosen_option_id, None);
        assert_eq!(cleared.decided_on, None);
    }

    #[tokio::test]
    async fn update_with_choice_applies_the_edit_and_the_choice_together() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let o = add_option(&db, vehicle_id, n.id, option("Pads"))
            .await
            .unwrap();
        let updated = update_with_choice(
            &db,
            vehicle_id,
            n.id,
            UpdateNeed {
                supplied_by: Some(Some(Supplier::Owner)),
                decision_reason: Some(Some("best reviews".into())),
                ..Default::default()
            },
            Some(Some(o.id)),
        )
        .await
        .unwrap();
        assert_eq!(updated.supplied_by, Some(Supplier::Owner));
        assert_eq!(updated.chosen_option_id, Some(o.id));
        assert_eq!(updated.decision_reason.as_deref(), Some("best reviews"));
        assert!(updated.decided_on.is_some());

        // Clearing the choice clears its reason and date.
        let cleared = update_with_choice(&db, vehicle_id, n.id, UpdateNeed::default(), Some(None))
            .await
            .unwrap();
        assert_eq!(cleared.chosen_option_id, None);
        assert_eq!(cleared.decision_reason, None);
        assert_eq!(cleared.decided_on, None);
        assert_eq!(cleared.supplied_by, Some(Supplier::Owner));
    }

    #[tokio::test]
    async fn update_with_choice_without_a_choice_edits_only_the_reason() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let o = add_option(&db, vehicle_id, n.id, option("Pads"))
            .await
            .unwrap();
        decide(&db, vehicle_id, n.id, Some(o.id), Some("old".into()))
            .await
            .unwrap();
        let updated = update_with_choice(
            &db,
            vehicle_id,
            n.id,
            UpdateNeed {
                decision_reason: Some(Some("new".into())),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
        assert_eq!(updated.chosen_option_id, Some(o.id));
        assert_eq!(updated.decision_reason.as_deref(), Some("new"));
    }

    #[tokio::test]
    async fn a_refused_choice_leaves_the_other_edits_unapplied() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let bad = add_option(
            &db,
            vehicle_id,
            n.id,
            NewOption {
                fitment: Some(Fitment::DoesNotFit),
                ..option("Wrong pads")
            },
        )
        .await
        .unwrap();
        let err = update_with_choice(
            &db,
            vehicle_id,
            n.id,
            UpdateNeed {
                name: Some("Renamed".into()),
                supplied_by: Some(Some(Supplier::Owner)),
                ..Default::default()
            },
            Some(Some(bad.id)),
        )
        .await
        .unwrap_err();
        assert!(matches!(&err, DomainError::BadRequest(m) if m.contains("does not fit")));
        let after = require(&db, vehicle_id, n.id).await.unwrap();
        assert_eq!(after.name, "Pads");
        assert_eq!(after.supplied_by, None);
        assert_eq!(after.chosen_option_id, None);
    }

    #[tokio::test]
    async fn update_with_choice_reads_a_wrong_vehicle_need_as_missing() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let other = VehicleFixture::new().name("Other").insert_id(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let missing = update_with_choice(&db, other, 99_999, UpdateNeed::default(), Some(None))
            .await
            .unwrap_err();
        let wrong = update_with_choice(&db, other, n.id, UpdateNeed::default(), Some(None))
            .await
            .unwrap_err();
        assert!(matches!(wrong, DomainError::NotFound(_)));
        assert_eq!(
            wrong.to_string().replace(&n.id.to_string(), "N"),
            missing.to_string().replace("99999", "N")
        );
    }

    #[tokio::test]
    async fn a_reason_needs_a_chosen_option() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let o = add_option(&db, vehicle_id, n.id, option("Pads"))
            .await
            .unwrap();
        decide(&db, vehicle_id, n.id, Some(o.id), Some("good".into()))
            .await
            .unwrap();

        let err = decide(&db, vehicle_id, n.id, None, Some("why".into()))
            .await
            .unwrap_err();
        assert_reason_refused(&err);
        let err = update_with_choice(
            &db,
            vehicle_id,
            n.id,
            UpdateNeed {
                name: Some("Renamed".into()),
                decision_reason: Some(Some("why".into())),
                ..Default::default()
            },
            Some(None),
        )
        .await
        .unwrap_err();
        assert_reason_refused(&err);
        let after = require(&db, vehicle_id, n.id).await.unwrap();
        assert_eq!(after.name, "Pads");
        assert_eq!(after.chosen_option_id, Some(o.id));
        assert_eq!(after.decision_reason.as_deref(), Some("good"));
    }

    fn assert_reason_refused(err: &DomainError) {
        assert!(
            matches!(err, DomainError::Invalid { field, message }
                if field == "decision_reason" && message.starts_with("needs a chosen option unless the shop supplies the need")),
            "got: {err}"
        );
    }

    fn reason(text: &str) -> UpdateNeed {
        UpdateNeed {
            decision_reason: Some(Some(text.into())),
            ..Default::default()
        }
    }

    fn supplier(who: Option<Supplier>) -> UpdateNeed {
        UpdateNeed {
            supplied_by: Some(who),
            ..Default::default()
        }
    }

    // Writes a reason straight to a need with no chosen option, bypassing the
    // rule that `update` enforces.
    async fn store_reason(db: &DatabaseConnection, need_id: i32, text: &str) {
        job_need::ActiveModel {
            id: Unchanged(need_id),
            decision_reason: Set(Some(text.into())),
            ..Default::default()
        }
        .update(db)
        .await
        .unwrap();
    }

    async fn shop_need(db: &DatabaseConnection, vehicle_id: i32, job_id: i32) -> job_need::Model {
        add(
            db,
            vehicle_id,
            job_id,
            NewNeed {
                supplied_by: Some(Supplier::Shop),
                ..need("Plugs")
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_reason_without_a_choice_is_refused_off_shop_supply() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        for who in [None, Some(Supplier::Owner)] {
            let n = add(
                &db,
                vehicle_id,
                job_id,
                NewNeed {
                    supplied_by: who,
                    ..need("Pads")
                },
            )
            .await
            .unwrap();
            let err = update(&db, vehicle_id, n.id, reason("why"))
                .await
                .unwrap_err();
            assert_reason_refused(&err);
            assert_eq!(
                require(&db, vehicle_id, n.id)
                    .await
                    .unwrap()
                    .decision_reason,
                None
            );
        }
    }

    #[tokio::test]
    async fn a_shop_need_with_no_choice_stores_a_reason() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let shop = shop_need(&db, vehicle_id, job_id).await;
        let stored = update(&db, vehicle_id, shop.id, reason("OEM plugs only"))
            .await
            .unwrap();
        assert_eq!(stored.decision_reason.as_deref(), Some("OEM plugs only"));
    }

    #[tokio::test]
    async fn the_supplier_sent_with_a_reason_decides_whether_it_is_held() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Fluid")).await.unwrap();
        let to_shop = update(
            &db,
            vehicle_id,
            n.id,
            UpdateNeed {
                supplied_by: Some(Some(Supplier::Shop)),
                ..reason("shop stocks it")
            },
        )
        .await
        .unwrap();
        assert_eq!(to_shop.decision_reason.as_deref(), Some("shop stocks it"));
        let err = update(
            &db,
            vehicle_id,
            n.id,
            UpdateNeed {
                supplied_by: Some(Some(Supplier::Owner)),
                ..reason("cheaper")
            },
        )
        .await
        .unwrap_err();
        assert_reason_refused(&err);
        let after = require(&db, vehicle_id, n.id).await.unwrap();
        assert_eq!(after.supplied_by, Some(Supplier::Shop));
        assert_eq!(after.decision_reason.as_deref(), Some("shop stocks it"));
    }

    #[tokio::test]
    async fn clearing_a_reason_is_always_allowed() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let owner = add(&db, vehicle_id, job_id, need("Wipers")).await.unwrap();
        store_reason(&db, owner.id, "legacy").await;
        let clear = UpdateNeed {
            decision_reason: Some(None),
            ..Default::default()
        };
        let cleared = update(&db, vehicle_id, owner.id, clear).await.unwrap();
        assert_eq!(cleared.decision_reason, None);
    }

    #[tokio::test]
    async fn a_shop_need_takes_a_reason_with_a_cleared_choice() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let shop = shop_need(&db, vehicle_id, job_id).await;
        let decided = decide(&db, vehicle_id, shop.id, None, Some("OEM only".into()))
            .await
            .unwrap();
        assert_eq!(decided.decision_reason.as_deref(), Some("OEM only"));
    }

    #[tokio::test]
    async fn leaving_shop_supply_without_a_choice_clears_the_reason() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        for who in [None, Some(Supplier::Owner)] {
            let shop = shop_need(&db, vehicle_id, job_id).await;
            update(&db, vehicle_id, shop.id, reason("OEM only"))
                .await
                .unwrap();
            let moved = update(&db, vehicle_id, shop.id, supplier(who))
                .await
                .unwrap();
            assert_eq!(moved.supplied_by, who);
            assert_eq!(moved.decision_reason, None);
        }

        // With a chosen option the reason belongs to the choice and stays.
        let shop = shop_need(&db, vehicle_id, job_id).await;
        let o = add_option(&db, vehicle_id, shop.id, option("NGK plugs"))
            .await
            .unwrap();
        decide(&db, vehicle_id, shop.id, Some(o.id), Some("NGK".into()))
            .await
            .unwrap();
        let moved = update(&db, vehicle_id, shop.id, supplier(Some(Supplier::Owner)))
            .await
            .unwrap();
        assert_eq!(moved.decision_reason.as_deref(), Some("NGK"));
    }

    #[tokio::test]
    async fn resending_the_stored_supplier_keeps_the_reason() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let shop = shop_need(&db, vehicle_id, job_id).await;
        update(&db, vehicle_id, shop.id, reason("OEM only"))
            .await
            .unwrap();
        let same = update(&db, vehicle_id, shop.id, supplier(Some(Supplier::Shop)))
            .await
            .unwrap();
        assert_eq!(same.decision_reason.as_deref(), Some("OEM only"));

        // The web supplier form sends the supplier and the approach together.
        let owner = add(
            &db,
            vehicle_id,
            job_id,
            NewNeed {
                supplied_by: Some(Supplier::Owner),
                ..need("Pads")
            },
        )
        .await
        .unwrap();
        store_reason(&db, owner.id, "legacy").await;
        let edited = update(
            &db,
            vehicle_id,
            owner.id,
            UpdateNeed {
                supplied_by: Some(Some(Supplier::Owner)),
                approach: Some(Some(job_need::Approach::Upgrade)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(edited.decision_reason.as_deref(), Some("legacy"));
    }

    #[tokio::test]
    async fn an_unrelated_edit_keeps_a_stored_reason() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        store_reason(&db, n.id, "legacy").await;
        let renamed = update(
            &db,
            vehicle_id,
            n.id,
            UpdateNeed {
                name: Some("Front pads".into()),
                quantity: Some(2),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(renamed.name, "Front pads");
        assert_eq!(renamed.decision_reason.as_deref(), Some("legacy"));

        // Only leaving the shop clears, so deciding an undecided supplier
        // keeps the stored reason.
        let owner = update(&db, vehicle_id, n.id, supplier(Some(Supplier::Owner)))
            .await
            .unwrap();
        assert_eq!(owner.decision_reason.as_deref(), Some("legacy"));
    }

    // Leaving the shop clears its reason inside the transaction, and the
    // refused reason on the cleared choice then rolls that back.
    #[tokio::test]
    async fn leaving_shop_with_a_cleared_choice_and_a_reason_is_refused_whole() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let shop = shop_need(&db, vehicle_id, job_id).await;
        update(&db, vehicle_id, shop.id, reason("OEM only"))
            .await
            .unwrap();
        let err = update_with_choice(
            &db,
            vehicle_id,
            shop.id,
            UpdateNeed {
                supplied_by: Some(Some(Supplier::Owner)),
                ..reason("cheaper myself")
            },
            Some(None),
        )
        .await
        .unwrap_err();
        assert_reason_refused(&err);
        let after = require(&db, vehicle_id, shop.id).await.unwrap();
        assert_eq!(after.supplied_by, Some(Supplier::Shop));
        assert_eq!(after.decision_reason.as_deref(), Some("OEM only"));
    }

    fn clear_choice_with(reason: Option<Option<&str>>) -> UpdateNeed {
        UpdateNeed {
            decision_reason: reason.map(|r| r.map(Into::into)),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn clearing_no_choice_keeps_a_shop_needs_reason() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let shop = shop_need(&db, vehicle_id, job_id).await;
        update(&db, vehicle_id, shop.id, reason("OEM only"))
            .await
            .unwrap();
        let kept = update_with_choice(
            &db,
            vehicle_id,
            shop.id,
            clear_choice_with(None),
            Some(None),
        )
        .await
        .unwrap();
        assert_eq!(kept.decision_reason.as_deref(), Some("OEM only"));
    }

    // An owner need that holds a reason and no choice is not refused when the
    // choice it does not have is cleared.
    #[tokio::test]
    async fn clearing_no_choice_on_an_owner_need_changes_nothing() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        store_reason(&db, n.id, "legacy").await;
        let same = update_with_choice(&db, vehicle_id, n.id, clear_choice_with(None), Some(None))
            .await
            .unwrap();
        assert_eq!(same.decision_reason.as_deref(), Some("legacy"));
    }

    #[tokio::test]
    async fn clearing_a_shop_choice_clears_its_reason_unless_a_new_one_is_sent() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let shop = shop_need(&db, vehicle_id, job_id).await;
        let o = add_option(&db, vehicle_id, shop.id, option("NGK plugs"))
            .await
            .unwrap();
        decide(
            &db,
            vehicle_id,
            shop.id,
            Some(o.id),
            Some("cheaper than OEM".into()),
        )
        .await
        .unwrap();
        let cleared = update_with_choice(
            &db,
            vehicle_id,
            shop.id,
            clear_choice_with(None),
            Some(None),
        )
        .await
        .unwrap();
        assert_eq!(cleared.chosen_option_id, None);
        assert_eq!(cleared.decision_reason, None);

        decide(
            &db,
            vehicle_id,
            shop.id,
            Some(o.id),
            Some("cheaper than OEM".into()),
        )
        .await
        .unwrap();
        let replaced = update_with_choice(
            &db,
            vehicle_id,
            shop.id,
            clear_choice_with(Some(Some("The shop stocks OEM"))),
            Some(None),
        )
        .await
        .unwrap();
        assert_eq!(replaced.chosen_option_id, None);
        assert_eq!(
            replaced.decision_reason.as_deref(),
            Some("The shop stocks OEM")
        );
    }

    #[tokio::test]
    async fn wrong_vehicle_need_reads_as_missing() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let other = VehicleFixture::new().insert_id(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let expected = format!("Need {} not found", n.id);
        let nonexistent = require(&db, vehicle_id, 9999)
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(nonexistent, "Need 9999 not found");
        let same = |err: DomainError| assert_eq!(err.to_string(), expected);
        same(require(&db, other, n.id).await.unwrap_err());
        same(
            update(&db, other, n.id, UpdateNeed::default())
                .await
                .unwrap_err(),
        );
        same(delete(&db, other, n.id).await.unwrap_err());
        same(add_option(&db, other, n.id, option("x")).await.unwrap_err());
        same(decide(&db, other, n.id, None, None).await.unwrap_err());
        let err = add(&db, other, job_id, need("Pads")).await.unwrap_err();
        assert_eq!(err.to_string(), format!("Work item {job_id} not found"));
        require(&db, vehicle_id, n.id).await.unwrap();
    }

    #[tokio::test]
    async fn wrong_vehicle_option_reads_as_missing() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let other = VehicleFixture::new().insert_id(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let o = add_option(&db, vehicle_id, n.id, option("Pads"))
            .await
            .unwrap();
        let missing = update_option(&db, vehicle_id, 9999, UpdateOption::default())
            .await
            .unwrap_err()
            .to_string();
        let wrong = update_option(&db, other, o.id, UpdateOption::default())
            .await
            .unwrap_err()
            .to_string();
        let expected = format!("Option {} not found", o.id);
        assert_eq!(missing, "Option 9999 not found");
        assert_eq!(wrong, expected);
        let wrong = delete_option(&db, other, o.id)
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(wrong, expected);
        assert_eq!(
            need_option::Entity::find().count(&db).await.unwrap(),
            1,
            "the option must survive"
        );
    }

    // A need with one option that has been chosen, plus the option.
    async fn chosen_need(
        db: &DatabaseConnection,
        vehicle_id: i32,
        job_id: i32,
        name: &str,
        seller: Option<&str>,
        price: Option<i32>,
    ) -> (job_need::Model, need_option::Model) {
        let n = add(db, vehicle_id, job_id, need(name)).await.unwrap();
        let o = add_option(
            db,
            vehicle_id,
            n.id,
            NewOption {
                manufacturer: Some("Zimmermann".into()),
                part_number: Some("100.3394.20".into()),
                seller: seller.map(String::from),
                price_cents: price,
                url: Some("https://example.com/rotors".into()),
                ..option(name)
            },
        )
        .await
        .unwrap();
        decide(db, vehicle_id, n.id, Some(o.id), Some("fits".into()))
            .await
            .unwrap();
        (require(db, vehicle_id, n.id).await.unwrap(), o)
    }

    async fn ordered_need(
        db: &DatabaseConnection,
        vehicle_id: i32,
        job_id: i32,
    ) -> (job_need::Model, need_option::Model) {
        let (n, o) = chosen_need(
            db,
            vehicle_id,
            job_id,
            "Rear rotors",
            Some("FCP Euro"),
            Some(18_000),
        )
        .await;
        mark_ordered(db, vehicle_id, n.id, OrderDetails::default())
            .await
            .unwrap();
        (require(db, vehicle_id, n.id).await.unwrap(), o)
    }

    async fn new_part(db: &DatabaseConnection, vehicle_id: i32, name: &str) -> part::Model {
        crate::services::part::create(
            db,
            vehicle_id,
            crate::inputs::part::NewPart {
                name: name.into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn mark_ordered_copies_the_option_and_lets_details_win() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let (n, o) = chosen_need(
            &db,
            vehicle_id,
            job_id,
            "Rear rotors",
            Some("FCP Euro"),
            Some(18_000),
        )
        .await;

        let bought = mark_ordered(&db, vehicle_id, n.id, OrderDetails::default())
            .await
            .unwrap();
        let today = crate::services::today();
        assert_eq!(bought.part.name, o.description);
        assert_eq!(bought.part.manufacturer.as_deref(), Some("Zimmermann"));
        assert_eq!(bought.part.part_number.as_deref(), Some("100.3394.20"));
        assert_eq!(bought.part.seller.as_deref(), Some("FCP Euro"));
        assert_eq!(bought.part.cost_cents, Some(18_000));
        assert_eq!(
            bought.part.retailer_url.as_deref(),
            Some("https://example.com/rotors")
        );
        assert_eq!(bought.part.purchase_date, Some(today));
        assert_eq!(bought.part.supplied_by, Supplier::Owner);
        assert_eq!(bought.part.status, PartStatus::Purchased);
        assert_eq!(bought.need.part_id, Some(bought.part.id));
        assert_eq!(bought.need.supplied_by, Some(Supplier::Owner));
        assert_eq!(bought.stage, Stage::Ordered);

        let (n2, _) = chosen_need(
            &db,
            vehicle_id,
            job_id,
            "Pads",
            Some("FCP Euro"),
            Some(9_000),
        )
        .await;
        let bought = mark_ordered(
            &db,
            vehicle_id,
            n2.id,
            OrderDetails {
                seller: Some("Amazon".into()),
                purchase_date: Some(date("2026-11-02")),
                cost_cents: Some(8_500),
                order_ref: Some("112-445".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(bought.part.seller.as_deref(), Some("Amazon"));
        assert_eq!(bought.part.purchase_date, Some(date("2026-11-02")));
        assert_eq!(bought.part.cost_cents, Some(8_500));
        assert_eq!(bought.part.order_ref.as_deref(), Some("112-445"));
    }

    #[tokio::test]
    async fn mark_ordered_prices_the_whole_quantity() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let (n, o) = chosen_need(&db, vehicle_id, job_id, "Rotors", None, Some(6_900)).await;
        update(
            &db,
            vehicle_id,
            n.id,
            UpdateNeed {
                quantity: Some(2),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let n = require(&db, vehicle_id, n.id).await.unwrap();
        assert_eq!(order_cost_cents(&n, &o), Some(13_800));

        let bought = mark_ordered(&db, vehicle_id, n.id, OrderDetails::default())
            .await
            .unwrap();
        assert_eq!(bought.part.cost_cents, Some(13_800));
    }

    #[tokio::test]
    async fn mark_ordered_without_a_choice_is_refused() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let err = mark_ordered(&db, vehicle_id, n.id, OrderDetails::default())
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        assert_eq!(row_count::<part::Entity>(&db).await, 0);

        let (n, _) = chosen_need(&db, vehicle_id, job_id, "Rotors", None, None).await;
        mark_ordered(&db, vehicle_id, n.id, OrderDetails::default())
            .await
            .unwrap();
        let err = mark_ordered(&db, vehicle_id, n.id, OrderDetails::default())
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        assert_eq!(row_count::<part::Entity>(&db).await, 1);
    }

    #[tokio::test]
    async fn deciding_the_same_option_again_keeps_the_decision_date() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let (n, o) = chosen_need(&db, vehicle_id, job_id, "Pads", None, None).await;
        crate::test_support::set_decided_on(&db, n.id, "2026-01-02").await;

        let same = decide(&db, vehicle_id, n.id, Some(o.id), Some("quieter".into()))
            .await
            .unwrap();
        assert_eq!(same.decided_on, Some(date("2026-01-02")));
        assert_eq!(same.decision_reason.as_deref(), Some("quieter"));

        let other = add_option(&db, vehicle_id, n.id, option("Other pads"))
            .await
            .unwrap();
        let changed = decide(&db, vehicle_id, n.id, Some(other.id), None)
            .await
            .unwrap();
        let today = crate::services::today();
        assert_eq!(changed.decided_on, Some(today));
    }

    async fn reject(db: &DatabaseConnection, vehicle_id: i32, option_id: i32, why: Option<&str>) {
        update_option(
            db,
            vehicle_id,
            option_id,
            UpdateOption {
                rejected_reason: Some(why.map(Into::into)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    async fn option_row(db: &DatabaseConnection, option_id: i32) -> need_option::Model {
        need_option::Entity::find_by_id(option_id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn choosing_an_option_again_clears_its_choice_history() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let first = add_option(&db, vehicle_id, n.id, option("First"))
            .await
            .unwrap();
        let second = add_option(&db, vehicle_id, n.id, option("Second"))
            .await
            .unwrap();
        for (o, why) in [(&first, "fits"), (&second, "cheaper")] {
            decide(&db, vehicle_id, n.id, Some(o.id), Some(why.into()))
                .await
                .unwrap();
            reject(&db, vehicle_id, o.id, Some("wrong size")).await;
        }
        assert_eq!(
            option_row(&db, first.id).await.chosen_reason.as_deref(),
            Some("fits")
        );

        reject(&db, vehicle_id, first.id, None).await;
        decide(
            &db,
            vehicle_id,
            n.id,
            Some(first.id),
            Some("fits after all".into()),
        )
        .await
        .unwrap();

        let chosen = option_row(&db, first.id).await;
        assert_eq!((chosen.chosen_reason, chosen.chosen_on), (None, None));
        // The other option keeps the history of its own earlier choice.
        let other = option_row(&db, second.id).await;
        assert_eq!(other.chosen_reason.as_deref(), Some("cheaper"));
        assert!(other.chosen_on.is_some());
    }

    // A choice made without a reason leaves the option a date and no reason.
    #[tokio::test]
    async fn choosing_again_clears_a_date_kept_without_a_reason() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let o = add_option(&db, vehicle_id, n.id, option("Pads"))
            .await
            .unwrap();
        decide(&db, vehicle_id, n.id, Some(o.id), None)
            .await
            .unwrap();
        reject(&db, vehicle_id, o.id, Some("wrong size")).await;
        let kept = option_row(&db, o.id).await;
        assert_eq!(kept.chosen_reason, None);
        assert!(kept.chosen_on.is_some());

        reject(&db, vehicle_id, o.id, None).await;
        decide(&db, vehicle_id, n.id, Some(o.id), None)
            .await
            .unwrap();

        assert_eq!(option_row(&db, o.id).await.chosen_on, None);
    }

    #[tokio::test]
    async fn an_ordinary_choice_leaves_the_option_untouched() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let o = add_option(&db, vehicle_id, n.id, option("Pads"))
            .await
            .unwrap();
        let old: chrono::DateTime<chrono::Utc> = "2026-01-02T03:04:05Z".parse().unwrap();
        need_option::ActiveModel {
            id: Unchanged(o.id),
            updated_at: Set(old),
            ..Default::default()
        }
        .update(&db)
        .await
        .unwrap();

        decide(&db, vehicle_id, n.id, Some(o.id), Some("fits".into()))
            .await
            .unwrap();

        assert_eq!(option_row(&db, o.id).await.updated_at, old);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_orders_for_one_need_create_one_part() {
        // Each `connect` is its own one-connection pool, as the MCP server and
        // the web page are. Two orders share each pool, as two page posts do,
        // and the pools race on one file.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glovebox.db");
        let path = path.to_str().unwrap();
        let first = crate::db::connect(path).await.unwrap();
        let (vehicle_id, job_id) = seed_job(&first).await;
        let (n, _) = chosen_need(
            &first,
            vehicle_id,
            job_id,
            "Rear rotors",
            Some("FCP Euro"),
            Some(13_800),
        )
        .await;
        let mut pools = vec![first];
        for _ in 1..3 {
            pools.push(crate::db::connect(path).await.unwrap());
        }

        let mut orders = tokio::task::JoinSet::new();
        for db in pools.iter().chain(pools.iter()).cloned() {
            orders.spawn(async move {
                mark_ordered(&db, vehicle_id, n.id, OrderDetails::default()).await
            });
        }
        let results = orders.join_all().await;

        assert_eq!(
            results.iter().filter(|r| r.is_ok()).count(),
            1,
            "{results:?}"
        );
        for err in results.into_iter().filter_map(Result::err) {
            assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        }
        assert_eq!(row_count::<part::Entity>(&pools[0]).await, 1);
    }

    // Decisions and need edits racing on one file from several pools all
    // succeed: each transaction takes the write lock before it reads.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_decisions_on_one_need_all_succeed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glovebox.db");
        let path = path.to_str().unwrap();
        let first = crate::db::connect(path).await.unwrap();
        let (vehicle_id, job_id) = seed_job(&first).await;
        let (n, o) = chosen_need(&first, vehicle_id, job_id, "Pads", None, None).await;
        let mut pools = vec![first];
        for _ in 1..3 {
            pools.push(crate::db::connect(path).await.unwrap());
        }

        let mut decisions = tokio::task::JoinSet::new();
        for (i, db) in pools.iter().chain(pools.iter()).cloned().enumerate() {
            decisions.spawn(async move {
                let input = UpdateNeed {
                    decision_reason: Some(Some(format!("reason {i}"))),
                    ..Default::default()
                };
                if i % 2 == 0 {
                    update_with_choice(&db, vehicle_id, n.id, input, Some(Some(o.id))).await
                } else {
                    update(&db, vehicle_id, n.id, input).await
                }
            });
        }
        let results = decisions.join_all().await;

        assert!(results.iter().all(Result::is_ok), "{results:?}");
    }

    // Arrival writes racing on one file from several pools all succeed: each
    // transaction takes the write lock before it reads the stage.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_arrival_writes_on_one_need_all_succeed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glovebox.db");
        let path = path.to_str().unwrap();
        let first = crate::db::connect(path).await.unwrap();
        let (vehicle_id, job_id) = seed_job(&first).await;
        let (n, _) = ordered_need(&first, vehicle_id, job_id).await;
        let mut pools = vec![first];
        for _ in 1..3 {
            pools.push(crate::db::connect(path).await.unwrap());
        }

        let mut writes = tokio::task::JoinSet::new();
        for (i, db) in pools.iter().chain(pools.iter()).cloned().enumerate() {
            writes.spawn(async move {
                if i % 2 == 0 {
                    mark_received(&db, vehicle_id, n.id, None).await
                } else {
                    clear_received(&db, vehicle_id, n.id).await
                }
            });
        }
        let results = writes.join_all().await;

        assert!(results.iter().all(Result::is_ok), "{results:?}");
    }

    #[tokio::test]
    async fn mark_ordered_on_a_shop_need_is_refused() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let (n, _) = chosen_need(&db, vehicle_id, job_id, "Pads", None, None).await;
        update(
            &db,
            vehicle_id,
            n.id,
            UpdateNeed {
                supplied_by: Some(Some(Supplier::Shop)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let err = mark_ordered(&db, vehicle_id, n.id, OrderDetails::default())
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        assert_eq!(row_count::<part::Entity>(&db).await, 0);
    }

    #[tokio::test]
    async fn link_part_refuses_a_part_linked_elsewhere() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let a = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let b = add(&db, vehicle_id, job_id, need("Rotors")).await.unwrap();
        let p = new_part(&db, vehicle_id, "Pads").await;
        link_part(&db, vehicle_id, a.id, p.id, None).await.unwrap();
        let err = link_part(&db, vehicle_id, b.id, p.id, None)
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("Part {} is already linked to need {}", p.id, a.id)
        );
        assert_eq!(require(&db, vehicle_id, b.id).await.unwrap().part_id, None);
    }

    #[tokio::test]
    async fn link_part_refuses_a_returned_part() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let p = new_part(&db, vehicle_id, "Pads").await;
        part::ActiveModel {
            id: Unchanged(p.id),
            status: Set(PartStatus::Returned),
            ..Default::default()
        }
        .update(&db)
        .await
        .unwrap();
        let err = link_part(&db, vehicle_id, n.id, p.id, None)
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
    }

    #[tokio::test]
    async fn link_part_sets_received_when_a_date_is_given() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let p = new_part(&db, vehicle_id, "Pads").await;
        let linked = link_part(&db, vehicle_id, n.id, p.id, Some(date("2026-11-05")))
            .await
            .unwrap();
        assert_eq!(linked.part.received_on, Some(date("2026-11-05")));
        assert_eq!(linked.stage, Stage::Received);
        assert_eq!(linked.need.supplied_by, Some(Supplier::Owner));

        let m = add(&db, vehicle_id, job_id, need("Rotors")).await.unwrap();
        let q = new_part(&db, vehicle_id, "Rotors").await;
        let linked = link_part(&db, vehicle_id, m.id, q.id, None).await.unwrap();
        assert_eq!(linked.part.received_on, None);
        assert_eq!(linked.stage, Stage::Ordered);
    }

    #[tokio::test]
    async fn received_then_cleared_round_trips() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let (n, _) = ordered_need(&db, vehicle_id, job_id).await;
        // Clearing a date the part does not have is allowed.
        let cleared = clear_received(&db, vehicle_id, n.id).await.unwrap();
        assert_eq!(cleared.stage, Stage::Ordered);
        mark_received(&db, vehicle_id, n.id, Some(date("2026-11-06")))
            .await
            .unwrap();
        // A received part takes a new date.
        let got = mark_received(&db, vehicle_id, n.id, Some(date("2026-11-07")))
            .await
            .unwrap();
        assert_eq!(got.part.received_on, Some(date("2026-11-07")));
        assert_eq!(got.stage, Stage::Received);
        let cleared = clear_received(&db, vehicle_id, n.id).await.unwrap();
        assert_eq!(cleared.part.received_on, None);
        assert_eq!(cleared.stage, Stage::Ordered);
        let today = crate::services::today();
        let got = mark_received(&db, vehicle_id, n.id, None).await.unwrap();
        assert_eq!(got.part.received_on, Some(today));

        let bare = add(&db, vehicle_id, job_id, need("Fluid")).await.unwrap();
        let err = mark_received(&db, vehicle_id, bare.id, None)
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
    }

    #[tokio::test]
    async fn unlink_part_keeps_the_part_and_frees_the_need() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let (n, _) = ordered_need(&db, vehicle_id, job_id).await;
        let freed = unlink_part(&db, vehicle_id, n.id).await.unwrap();
        assert_eq!(freed.part_id, None);
        assert_eq!(row_count::<part::Entity>(&db).await, 1);
        let err = unlink_part(&db, vehicle_id, n.id).await.unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
    }

    #[tokio::test]
    async fn return_part_marks_returned_unlinks_and_adds_a_todo() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let (n, o) = ordered_need(&db, vehicle_id, job_id).await;
        let out = return_part(
            &db,
            vehicle_id,
            n.id,
            ReturnInput {
                reason: "wrong diameter".into(),
                option_was_wrong: false,
            },
        )
        .await
        .unwrap();
        let today = crate::services::today();
        assert_eq!(out.returned_part.status, PartStatus::Returned);
        assert_eq!(
            out.returned_part.return_reason.as_deref(),
            Some("wrong diameter")
        );
        assert_eq!(out.returned_part.returned_on, Some(today));
        assert_eq!(out.returned_part.returned_from_need_id, Some(n.id));
        assert_eq!(out.need.part_id, None);
        assert_eq!(out.need.chosen_option_id, Some(o.id));
        assert_eq!(out.stage, Stage::ToOrder);
        assert_eq!(out.todo.title, "Return Rear rotors to FCP Euro");
        assert_eq!(out.todo.vehicle_id, vehicle_id);
        assert_eq!(out.todo.appointment_id, None);
        let stored = require(&db, vehicle_id, n.id).await.unwrap();
        assert_eq!(stored.part_id, None);
    }

    #[tokio::test]
    async fn return_puts_the_todo_on_the_jobs_appointment_and_names_no_missing_seller() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let appointment = crate::entities::appointment::ActiveModel {
            vehicle_id: Set(vehicle_id),
            purpose: Set("Brakes".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        work_item::ActiveModel {
            id: Unchanged(job_id),
            appointment_id: Set(Some(appointment.id)),
            ..Default::default()
        }
        .update(&db)
        .await
        .unwrap();
        let (n, _) = chosen_need(&db, vehicle_id, job_id, "Pads", None, None).await;
        mark_ordered(&db, vehicle_id, n.id, OrderDetails::default())
            .await
            .unwrap();
        let out = return_part(
            &db,
            vehicle_id,
            n.id,
            ReturnInput {
                reason: "defective".into(),
                option_was_wrong: false,
            },
        )
        .await
        .unwrap();
        assert_eq!(out.todo.title, "Return Pads");
        assert_eq!(out.todo.appointment_id, Some(appointment.id));
    }

    // The home page and `whats_coming` list no closed or cancelled
    // appointment, so a to-do filed on one would never be seen.
    #[tokio::test]
    async fn return_from_a_closed_or_cancelled_appointment_puts_the_todo_on_the_vehicle() {
        use crate::{inputs::appointment::NewAppointment, services::appointment};
        for cancel in [false, true] {
            let db = test_db().await;
            let vehicle_id = VehicleFixture::new().insert_id(&db).await;
            let visit = appointment::create(
                &db,
                vehicle_id,
                NewAppointment {
                    purpose: "Brakes".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            let job_id = add_job(&db, vehicle_id, "Rear brakes", Some(visit.id)).await;
            let (n, _) = ordered_need(&db, vehicle_id, job_id).await;
            // A done job stays on the appointment when it is put away.
            crate::test_support::set_job_status(&db, vehicle_id, job_id, JobStatus::Done).await;
            if cancel {
                appointment::cancel(&db, vehicle_id, visit.id)
                    .await
                    .unwrap();
            } else {
                appointment::close(&db, vehicle_id, visit.id).await.unwrap();
            }
            let out = return_part(
                &db,
                vehicle_id,
                n.id,
                ReturnInput {
                    reason: "not fitted".into(),
                    option_was_wrong: false,
                },
            )
            .await
            .unwrap();
            assert_eq!(out.todo.appointment_id, None, "cancelled: {cancel}");
        }
    }

    #[tokio::test]
    async fn a_blank_rejection_reason_is_refused_and_keeps_the_choice() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let (n, o) = chosen_need(&db, vehicle_id, job_id, "Pads", None, None).await;
        for blank in ["", "   "] {
            let err = update_option(
                &db,
                vehicle_id,
                o.id,
                UpdateOption {
                    rejected_reason: Some(Some(blank.into())),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
            assert!(
                matches!(&err, DomainError::Invalid { field, .. } if field == "rejected_reason"),
                "{blank:?}: {err:?}"
            );
            assert_eq!(
                err.to_string(),
                "rejected_reason: must not be blank; clear the reason to un-reject the option"
            );
        }
        let after = require(&db, vehicle_id, n.id).await.unwrap();
        assert_eq!(after.chosen_option_id, Some(o.id));
        let option = need_option::Entity::find_by_id(o.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(!is_rejected(&option));
    }

    #[tokio::test]
    async fn return_with_wrong_option_rejects_it_and_goes_undecided() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let (n, o) = ordered_need(&db, vehicle_id, job_id).await;
        let out = return_part(
            &db,
            vehicle_id,
            n.id,
            ReturnInput {
                reason: "does not fit the GTI".into(),
                option_was_wrong: true,
            },
        )
        .await
        .unwrap();
        assert_eq!(out.stage, Stage::Undecided);
        assert_choice_cleared(&db, vehicle_id, n.id).await;
        let rejected = need_option::Entity::find_by_id(o.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            rejected.rejected_reason.as_deref(),
            Some("does not fit the GTI")
        );
        // The reason the owner chose it survives on the rejected option.
        let today = crate::services::today();
        assert_eq!(rejected.chosen_reason.as_deref(), Some("fits"));
        assert_eq!(rejected.chosen_on, Some(today));
    }

    #[tokio::test]
    async fn return_before_ordering_is_refused() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let (n, _) = chosen_need(&db, vehicle_id, job_id, "Pads", None, None).await;
        let input = || ReturnInput {
            reason: "x".into(),
            option_was_wrong: false,
        };
        let err = return_part(&db, vehicle_id, n.id, input())
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        let (m, _) = ordered_need(&db, vehicle_id, job_id).await;
        fit(&db, vehicle_id, m.part_id.unwrap(), PartStatus::Installed).await;
        let err = return_part(&db, vehicle_id, m.id, input())
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        assert_eq!(
            err.to_string(),
            format!(
                "Need {} is installed; only an ordered or received part can be returned",
                m.id
            )
        );
        assert_eq!(row_count::<todo::Entity>(&db).await, 0);
    }

    #[tokio::test]
    async fn deleting_a_linked_part_leaves_the_need_to_order() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let (n, _) = ordered_need(&db, vehicle_id, job_id).await;
        crate::services::part::delete(&db, vehicle_id, n.part_id.unwrap())
            .await
            .unwrap();
        let after = require(&db, vehicle_id, n.id).await.unwrap();
        assert_eq!(after.part_id, None);
        assert_eq!(stage(&after, None), Stage::ToOrder);
    }

    #[tokio::test]
    async fn wrong_vehicle_ids_read_as_missing_when_buying_and_returning() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let other = VehicleFixture::new().insert_id(&db).await;
        let (n, _) = ordered_need(&db, vehicle_id, job_id).await;
        let spare = add(&db, vehicle_id, job_id, need("Fluid")).await.unwrap();
        let mine = new_part(&db, vehicle_id, "Fluid").await;
        let theirs = new_part(&db, other, "Fluid").await;
        let missing_need = format!("Need {} not found", n.id);
        let same = |err: DomainError| assert_eq!(err.to_string(), missing_need);
        same(
            mark_ordered(&db, other, n.id, OrderDetails::default())
                .await
                .unwrap_err(),
        );
        same(
            link_part(&db, other, n.id, mine.id, None)
                .await
                .unwrap_err(),
        );
        same(unlink_part(&db, other, n.id).await.unwrap_err());
        same(mark_received(&db, other, n.id, None).await.unwrap_err());
        same(clear_received(&db, other, n.id).await.unwrap_err());
        same(
            return_part(
                &db,
                other,
                n.id,
                ReturnInput {
                    reason: "x".into(),
                    option_was_wrong: false,
                },
            )
            .await
            .unwrap_err(),
        );
        assert_eq!(
            link_part(&db, other, 9999, mine.id, None)
                .await
                .unwrap_err()
                .to_string(),
            "Need 9999 not found"
        );

        let nonexistent = link_part(&db, vehicle_id, spare.id, 9999, None)
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(nonexistent, "Part 9999 not found");
        let wrong = link_part(&db, vehicle_id, spare.id, theirs.id, None)
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(wrong, format!("Part {} not found", theirs.id));
        assert_eq!(
            require(&db, vehicle_id, spare.id).await.unwrap().part_id,
            None
        );
    }

    #[tokio::test]
    async fn options_store_blank_text_as_null_and_trim_the_rest() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let blank = || Some("   ".to_string());
        let added = add_option(
            &db,
            vehicle_id,
            n.id,
            NewOption {
                description: "Pads".into(),
                manufacturer: blank(),
                part_number: Some("  5Q0615601E ".into()),
                seller: blank(),
                url: blank(),
                fitment_note: Some(String::new()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(added.manufacturer, None);
        assert_eq!(added.part_number.as_deref(), Some("5Q0615601E"));
        assert_eq!(added.seller, None);
        assert_eq!(added.url, None);
        assert_eq!(added.fitment_note, None);

        let updated = update_option(
            &db,
            vehicle_id,
            added.id,
            UpdateOption {
                manufacturer: Some(Some(" Textar ".into())),
                part_number: Some(blank()),
                seller: Some(Some(" FCP Euro".into())),
                url: Some(blank()),
                fitment_note: Some(Some("\t".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.manufacturer.as_deref(), Some("Textar"));
        assert_eq!(updated.part_number, None);
        assert_eq!(updated.seller.as_deref(), Some("FCP Euro"));
        assert_eq!(updated.url, None);
        assert_eq!(updated.fitment_note, None);
    }

    #[tokio::test]
    async fn a_blank_option_description_is_invalid_on_add_and_update() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
        let err = add_option(&db, vehicle_id, n.id, option("  "))
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { .. }));
        let o = add_option(&db, vehicle_id, n.id, option("Pads"))
            .await
            .unwrap();
        let err = update_option(
            &db,
            vehicle_id,
            o.id,
            UpdateOption {
                description: Some(" ".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { .. }));
    }

    // Puts the ordered need's part on the car with `status`.
    async fn fit(db: &DatabaseConnection, vehicle_id: i32, part_id: i32, status: PartStatus) {
        crate::services::part::update(
            db,
            vehicle_id,
            part_id,
            crate::inputs::part::UpdatePart {
                status: Some(status),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    async fn received_on(
        db: &DatabaseConnection,
        vehicle_id: i32,
        part_id: i32,
    ) -> Option<chrono::NaiveDate> {
        crate::services::part::get(db, vehicle_id, part_id)
            .await
            .unwrap()
            .received_on
    }

    #[tokio::test]
    async fn the_arrival_date_of_a_fitted_part_cannot_change() {
        for status in [PartStatus::Installed, PartStatus::Replaced] {
            let db = test_db().await;
            let (vehicle_id, job_id) = seed_job(&db).await;
            let (n, _) = ordered_need(&db, vehicle_id, job_id).await;
            let part_id = n.part_id.unwrap();
            mark_received(&db, vehicle_id, n.id, Some(date("2026-11-01")))
                .await
                .unwrap();
            fit(&db, vehicle_id, part_id, status).await;

            let err = mark_received(&db, vehicle_id, n.id, Some(date("2026-11-03")))
                .await
                .unwrap_err();
            assert_eq!(
                err.to_string(),
                format!(
                    "Need {} is installed; only an ordered or received part can be marked received",
                    n.id
                ),
                "{status:?}"
            );
            assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
            let err = clear_received(&db, vehicle_id, n.id).await.unwrap_err();
            assert_eq!(
                err.to_string(),
                format!(
                    "Need {} is installed; only an ordered or received part can have its arrival \
                     date cleared",
                    n.id
                ),
                "{status:?}"
            );
            assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
            assert_eq!(
                received_on(&db, vehicle_id, part_id).await,
                Some(date("2026-11-01")),
                "{status:?}"
            );
        }
    }

    #[test]
    fn has_unfitted_part_holds_for_ordered_and_received_only() {
        for (s, expected) in [
            (Stage::Undecided, false),
            (Stage::ToOrder, false),
            (Stage::ShopSupplies, false),
            (Stage::Ordered, true),
            (Stage::Received, true),
            (Stage::Installed, false),
        ] {
            assert_eq!(s.has_unfitted_part(), expected, "{s:?}");
        }
    }

    #[test]
    fn part_link_reads_the_part_and_the_date() {
        let on = Some(date("2026-11-01"));
        assert_eq!(
            part_link(Some(Some(7)), on).unwrap(),
            PartLink::Link {
                part_id: 7,
                received_on: on
            }
        );
        assert_eq!(
            part_link(Some(Some(7)), None).unwrap(),
            PartLink::Link {
                part_id: 7,
                received_on: None
            }
        );
        assert_eq!(part_link(Some(None), None).unwrap(), PartLink::Unlink);
        let err = part_link(None, None).unwrap_err();
        assert_eq!(
            err.to_string(),
            "part_id: is required: name a part to link, or unlink the need's current part"
        );
        assert!(matches!(err, DomainError::Invalid { .. }), "{err:?}");
        let err = part_link(Some(None), on).unwrap_err();
        assert_eq!(
            err.to_string(),
            "received_on: cannot be given when unlinking a part"
        );
        assert!(matches!(err, DomainError::Invalid { .. }), "{err:?}");
    }

    #[test]
    fn arrival_reads_the_flag_and_the_date() {
        let on = Some(date("2026-11-01"));
        assert_eq!(arrival(true, on).unwrap(), ArrivalChange::Received(on));
        assert_eq!(arrival(true, None).unwrap(), ArrivalChange::Received(None));
        assert_eq!(arrival(false, None).unwrap(), ArrivalChange::NotReceived);
        let err = arrival(false, on).unwrap_err();
        assert_eq!(
            err.to_string(),
            "received_on: cannot be given when clearing the arrival date"
        );
        assert!(matches!(err, DomainError::Invalid { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn set_part_links_and_unlinks() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let n = add(&db, vehicle_id, job_id, need("Fluid")).await.unwrap();
        let p = new_part(&db, vehicle_id, "Fluid").await;

        let linked = set_part(
            &db,
            vehicle_id,
            n.id,
            PartLink::Link {
                part_id: p.id,
                received_on: Some(date("2026-11-01")),
            },
        )
        .await
        .unwrap();
        let PartLinkOutcome::Linked(purchase) = &linked else {
            panic!("expected a link: {linked:?}");
        };
        assert_eq!(purchase.need.part_id, Some(p.id));
        assert_eq!(purchase.part.received_on, Some(date("2026-11-01")));
        assert_eq!(purchase.stage, Stage::Received);
        let json = serde_json::to_value(&linked).unwrap();
        assert_eq!(json["need"]["part_id"], p.id, "{json}");
        assert_eq!(json["stage"], "received", "{json}");

        let unlinked = set_part(&db, vehicle_id, n.id, PartLink::Unlink)
            .await
            .unwrap();
        let PartLinkOutcome::Unlinked(model) = &unlinked else {
            panic!("expected an unlink: {unlinked:?}");
        };
        assert_eq!(model.part_id, None);
        let json = serde_json::to_value(&unlinked).unwrap();
        assert_eq!(json["id"], n.id, "{json}");
        assert_eq!(json["part_id"], serde_json::Value::Null, "{json}");
    }

    #[tokio::test]
    async fn set_arrival_stamps_and_clears() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let (n, _) = ordered_need(&db, vehicle_id, job_id).await;

        let got = set_arrival(
            &db,
            vehicle_id,
            n.id,
            ArrivalChange::Received(Some(date("2026-11-01"))),
        )
        .await
        .unwrap();
        assert_eq!(got.part.received_on, Some(date("2026-11-01")));
        assert_eq!(got.stage, Stage::Received);
        let got = set_arrival(&db, vehicle_id, n.id, ArrivalChange::NotReceived)
            .await
            .unwrap();
        assert_eq!(got.part.received_on, None);
        assert_eq!(got.stage, Stage::Ordered);
    }

    #[tokio::test]
    async fn linking_with_a_date_refuses_a_fitted_part() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        // The need's own part, installed, linked again with a new date.
        let (n, _) = ordered_need(&db, vehicle_id, job_id).await;
        let own = n.part_id.unwrap();
        mark_received(&db, vehicle_id, n.id, Some(date("2026-11-01")))
            .await
            .unwrap();
        fit(&db, vehicle_id, own, PartStatus::Installed).await;
        let err = link_part(&db, vehicle_id, n.id, own, Some(date("2026-11-03")))
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "Need {} is installed; only an ordered or received part can be marked received",
                n.id
            )
        );
        assert_eq!(
            received_on(&db, vehicle_id, own).await,
            Some(date("2026-11-01"))
        );

        // A replaced part linked to no need, linked to a bare need with a date.
        let bare = add(&db, vehicle_id, job_id, need("Fluid")).await.unwrap();
        let loose = new_part(&db, vehicle_id, "Fluid").await;
        fit(&db, vehicle_id, loose.id, PartStatus::Replaced).await;
        let err = link_part(&db, vehicle_id, bare.id, loose.id, Some(date("2026-11-03")))
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        assert_eq!(received_on(&db, vehicle_id, loose.id).await, None);
        assert_eq!(
            require(&db, vehicle_id, bare.id).await.unwrap().part_id,
            None
        );

        // Without a date, a fitted part still links and reads installed.
        let linked = link_part(&db, vehicle_id, bare.id, loose.id, None)
            .await
            .unwrap();
        assert_eq!(linked.stage, Stage::Installed);
        assert_eq!(linked.part.received_on, None);
    }
}
