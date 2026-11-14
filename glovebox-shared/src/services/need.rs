//! Needs on a job and the researched options for each. A need belongs to a
//! work item, and its vehicle is the work item's vehicle. A wrong-vehicle id
//! reads exactly like a nonexistent one.

use sea_orm::{sea_query::Expr, *};
use serde::Serialize;

use crate::{
    entities::{job_need, need_option, part, work_item},
    error::{DomainError, DomainResult},
    inputs::need::{NewNeed, NewOption, UpdateNeed, UpdateOption},
};

/// Allowed values for `job_needs.supplied_by`.
pub const VALID_SUPPLIERS: [&str; 2] = ["self", "shop"];
/// Allowed values for `job_needs.approach`.
pub const VALID_APPROACHES: [&str; 2] = ["replacement", "upgrade"];
/// Allowed values for `need_options.fitment`.
pub const VALID_FITMENTS: [&str; 3] = ["confirmed", "unconfirmed", "does_not_fit"];

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

/// Derives a need's stage. The first matching row wins, so a linked part
/// decides the stage whatever the supplier and chosen option say. `part` is
/// the need's linked part, if any.
#[must_use]
pub fn stage(need: &job_need::Model, part: Option<&part::Model>) -> Stage {
    if let Some(part) = part {
        return if part.status == "installed" {
            Stage::Installed
        } else if part.received_on.is_some() {
            Stage::Received
        } else {
            Stage::Ordered
        };
    }
    match need.supplied_by.as_deref() {
        Some("shop") => Stage::ShopSupplies,
        Some("self") if need.chosen_option_id.is_some() => Stage::ToOrder,
        _ => Stage::Undecided,
    }
}

fn validate_vocabulary(field: &str, value: &str, allowed: &[&str]) -> DomainResult<()> {
    if allowed.contains(&value) {
        return Ok(());
    }
    Err(DomainError::invalid(
        field,
        format!(
            "'{value}' is not valid. Must be one of: {}",
            allowed.join(", ")
        ),
    ))
}

fn validate_optional_vocabulary(
    field: &str,
    value: Option<&str>,
    allowed: &[&str],
) -> DomainResult<()> {
    value.map_or(Ok(()), |v| validate_vocabulary(field, v, allowed))
}

fn validate_name(field: &str, value: &str) -> DomainResult<()> {
    if value.trim().is_empty() {
        return Err(DomainError::invalid(field, "must not be blank"));
    }
    Ok(())
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
/// blank name, a quantity below 1 or a value outside the vocabulary.
pub async fn add(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    work_item_id: i32,
    input: NewNeed,
) -> DomainResult<job_need::Model> {
    validate_name("name", &input.name)?;
    let quantity = input.quantity.unwrap_or(1);
    validate_quantity(quantity)?;
    validate_optional_vocabulary(
        "supplied_by",
        input.supplied_by.as_deref(),
        &VALID_SUPPLIERS,
    )?;
    validate_optional_vocabulary("approach", input.approach.as_deref(), &VALID_APPROACHES)?;
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

/// Changes a need's name, quantity, supplier, approach or decision reason.
///
/// # Errors
/// `NotFound` for a need that is not the vehicle's; `Invalid` as for [`add`].
pub async fn update(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    need_id: i32,
    input: UpdateNeed,
) -> DomainResult<job_need::Model> {
    let existing = require(db, vehicle_id, need_id).await?;
    if let Some(name) = &input.name {
        validate_name("name", name)?;
    }
    if let Some(quantity) = input.quantity {
        validate_quantity(quantity)?;
    }
    if let Some(value) = &input.supplied_by {
        validate_optional_vocabulary("supplied_by", value.as_deref(), &VALID_SUPPLIERS)?;
    }
    if let Some(value) = &input.approach {
        validate_optional_vocabulary("approach", value.as_deref(), &VALID_APPROACHES)?;
    }
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
    }
    active.updated_at = Set(super::now_stamp());
    Ok(active.update(db).await?)
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
/// description, a negative price or an unknown fitment.
pub async fn add_option(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    need_id: i32,
    input: NewOption,
) -> DomainResult<need_option::Model> {
    validate_name("description", &input.description)?;
    validate_price(input.price_cents)?;
    validate_optional_vocabulary("fitment", input.fitment.as_deref(), &VALID_FITMENTS)?;
    require(db, vehicle_id, need_id).await?;
    let model = need_option::ActiveModel {
        need_id: Set(need_id),
        description: Set(input.description),
        manufacturer: Set(input.manufacturer),
        part_number: Set(input.part_number),
        seller: Set(input.seller),
        price_cents: Set(input.price_cents),
        url: Set(input.url),
        fitment: Set(input.fitment.unwrap_or_else(|| "unconfirmed".to_string())),
        fitment_note: Set(input.fitment_note),
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
        .col_expr(job_need::Column::DecidedOn, Expr::value(None::<String>))
        .col_expr(
            job_need::Column::DecisionReason,
            Expr::value(None::<String>),
        )
        .col_expr(job_need::Column::UpdatedAt, Expr::value(super::now_stamp()))
        .filter(job_need::Column::Id.eq(need_id))
        .filter(job_need::Column::ChosenOptionId.eq(option_id))
        .exec(db)
        .await?;
    Ok(())
}

/// Changes an option. Rejecting or marking `does_not_fit` the option its need
/// has chosen clears that choice, its date and its reason in the same
/// transaction.
///
/// # Errors
/// `NotFound` for an option that is not the vehicle's; `Invalid` as for
/// [`add_option`].
pub async fn update_option<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    option_id: i32,
    input: UpdateOption,
) -> DomainResult<need_option::Model> {
    let existing = require_option(db, vehicle_id, option_id).await?;
    if let Some(description) = &input.description {
        validate_name("description", description)?;
    }
    if let Some(price) = input.price_cents {
        validate_price(price)?;
    }
    validate_optional_vocabulary("fitment", input.fitment.as_deref(), &VALID_FITMENTS)?;
    let mut active: need_option::ActiveModel = existing.clone().into();
    if let Some(value) = input.description {
        active.description = Set(value);
    }
    if let Some(value) = input.manufacturer {
        active.manufacturer = Set(value);
    }
    if let Some(value) = input.part_number {
        active.part_number = Set(value);
    }
    if let Some(value) = input.seller {
        active.seller = Set(value);
    }
    if let Some(value) = input.price_cents {
        active.price_cents = Set(value);
    }
    if let Some(value) = input.url {
        active.url = Set(value);
    }
    if let Some(value) = &input.fitment {
        active.fitment = Set(value.clone());
    }
    if let Some(value) = input.fitment_note {
        active.fitment_note = Set(value);
    }
    if let Some(reason) = &input.rejected_reason {
        active.rejected_on = Set(reason
            .is_some()
            .then(|| super::today().format("%Y-%m-%d").to_string()));
        active.rejected_reason = Set(reason.clone());
    }
    active.updated_at = Set(super::now_stamp());

    let now_unusable = input.fitment.as_deref() == Some("does_not_fit")
        || input.rejected_reason.as_ref().is_some_and(Option::is_some);
    // The option write and the choice clearing commit together, so a need
    // never keeps a choice that was just rejected.
    let txn = db.begin().await?;
    let updated = active.update(&txn).await?;
    if now_unusable {
        clear_choice(&txn, updated.need_id, updated.id).await?;
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

/// Chooses an option for a need, or clears the choice when `option_id` is
/// `None`, and records the reason in the same write. Choosing stamps
/// `decided_on` with today's date; clearing removes it.
///
/// # Errors
/// `NotFound` for a need that is not the vehicle's; `BadRequest` for an
/// option of another need, one marked `does_not_fit` or a rejected one.
pub async fn decide(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    need_id: i32,
    option_id: Option<i32>,
    reason: Option<String>,
) -> DomainResult<job_need::Model> {
    let need = require(db, vehicle_id, need_id).await?;
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
        if option.fitment == "does_not_fit" {
            return Err(DomainError::BadRequest(format!(
                "Option {option_id} does not fit; change its fitment first"
            )));
        }
        if option.rejected_reason.is_some() {
            return Err(DomainError::BadRequest(format!(
                "Option {option_id} is rejected; clear its rejection first"
            )));
        }
    }
    let mut active: job_need::ActiveModel = need.into();
    active.chosen_option_id = Set(option_id);
    active.decision_reason = Set(reason);
    active.decided_on = Set(option_id.map(|_| super::today().format("%Y-%m-%d").to_string()));
    active.updated_at = Set(super::now_stamp());
    Ok(active.update(db).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inputs::work_item::NewWorkItem,
        test_support::{VehicleFixture, test_db},
    };

    async fn seed_job(db: &DatabaseConnection) -> (i32, i32) {
        let vehicle_id = VehicleFixture::new().insert_id(db).await;
        let job = super::super::work_item::create(
            db,
            vehicle_id,
            NewWorkItem {
                title: "Rear brakes".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        (vehicle_id, job.id)
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

    fn need_model(supplied_by: Option<&str>, chosen: Option<i32>) -> job_need::Model {
        job_need::Model {
            id: 1,
            work_item_id: 1,
            name: "Pads".into(),
            quantity: 1,
            supplied_by: supplied_by.map(String::from),
            approach: None,
            chosen_option_id: chosen,
            decision_reason: None,
            decided_on: None,
            part_id: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn part_model(status: &str, received_on: Option<&str>) -> part::Model {
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
            status: status.into(),
            installed_date: None,
            installed_odometer: None,
            installed_service_id: None,
            replaced_date: None,
            replaced_odometer: None,
            notes: None,
            created_at: String::new(),
            updated_at: String::new(),
            manufacturer_url: None,
            retailer_url: None,
            location: None,
            warranty_expires_on: None,
            warranty_expires_miles: None,
            supplied_by: "self".into(),
            received_on: received_on.map(String::from),
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
            (need_model(Some("self"), None), None, Stage::Undecided),
            (need_model(Some("self"), Some(1)), None, Stage::ToOrder),
            (need_model(Some("shop"), None), None, Stage::ShopSupplies),
            (need_model(Some("shop"), Some(1)), None, Stage::ShopSupplies),
            (
                need_model(None, None),
                Some(part_model("purchased", None)),
                Stage::Ordered,
            ),
            (
                need_model(Some("self"), Some(1)),
                Some(part_model("purchased", None)),
                Stage::Ordered,
            ),
            (
                need_model(Some("self"), Some(1)),
                Some(part_model("purchased", Some("2026-11-01"))),
                Stage::Received,
            ),
            (
                need_model(None, None),
                Some(part_model("installed", Some("2026-11-01"))),
                Stage::Installed,
            ),
            (
                need_model(Some("shop"), None),
                Some(part_model("installed", None)),
                Stage::Installed,
            ),
        ];
        for (need, part, expected) in cases {
            assert_eq!(stage(&need, part.as_ref()), expected, "{need:?} {part:?}");
        }
    }

    #[tokio::test]
    async fn add_rejects_zero_quantity_and_unknown_supplier() {
        let db = test_db().await;
        let (vehicle_id, job_id) = seed_job(&db).await;
        let zero = NewNeed {
            quantity: Some(0),
            ..need("Pads")
        };
        let err = add(&db, vehicle_id, job_id, zero).await.unwrap_err();
        assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "quantity"));
        let bad = NewNeed {
            supplied_by: Some("neighbor".into()),
            ..need("Pads")
        };
        let err = add(&db, vehicle_id, job_id, bad).await.unwrap_err();
        assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "supplied_by"));
        let bad = NewNeed {
            approach: Some("rebuild".into()),
            ..need("Pads")
        };
        let err = add(&db, vehicle_id, job_id, bad).await.unwrap_err();
        assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "approach"));
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
                fitment: Some("does_not_fit".into()),
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
                fitment: Some("does_not_fit".into()),
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
        let today = crate::services::today().format("%Y-%m-%d").to_string();
        assert_eq!(decided.chosen_option_id, Some(o.id));
        assert_eq!(decided.decision_reason.as_deref(), Some("best reviews"));
        assert_eq!(decided.decided_on.as_deref(), Some(today.as_str()));

        let cleared = decide(&db, vehicle_id, n.id, None, None).await.unwrap();
        assert_eq!(cleared.chosen_option_id, None);
        assert_eq!(cleared.decided_on, None);
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
}
