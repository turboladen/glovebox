use super::*;
use crate::{
    entities::part::PartStatus,
    test_support::{VehicleFixture, add_job, date, race_pools, row_count, test_db},
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

// A shop need's reason goes when the shop stops supplying it, even when
// the same call sends that reason back.
#[tokio::test]
async fn moving_off_the_shop_clears_a_resent_reason() {
    let db = test_db().await;
    let (vehicle_id, job_id) = seed_job(&db).await;
    let n = add(
        &db,
        vehicle_id,
        job_id,
        NewNeed {
            supplied_by: Some(Supplier::Shop),
            ..need("Brake fluid")
        },
    )
    .await
    .unwrap();
    update(
        &db,
        vehicle_id,
        n.id,
        UpdateNeed {
            decision_reason: Some(Some("They bleed it".into())),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let moved = update(
        &db,
        vehicle_id,
        n.id,
        UpdateNeed {
            supplied_by: Some(Some(Supplier::Owner)),
            decision_reason: Some(Some("They bleed it ".into())),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        (moved.supplied_by, moved.decision_reason),
        (Some(Supplier::Owner), None)
    );
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
async fn the_database_refuses_to_delete_a_chosen_option() {
    let db = test_db().await;
    let (vehicle_id, job_id) = seed_job(&db).await;
    let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
    let o = add_option(&db, vehicle_id, n.id, option("Pads"))
        .await
        .unwrap();
    decide(&db, vehicle_id, n.id, Some(o.id), None)
        .await
        .unwrap();
    let err = need_option::Entity::delete_by_id(o.id)
        .exec(&db)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("FOREIGN KEY"), "{err}");
    let after = job_need::Entity::find_by_id(n.id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.chosen_option_id, Some(o.id));
}

#[tokio::test]
async fn a_row_no_decision_fits_reads_as_an_internal_error() {
    let db = test_db().await;
    let (vehicle_id, job_id) = seed_job(&db).await;
    let n = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
    let unreadable = [
        job_need::Model {
            chosen_option_id: Some(1),
            ..n.clone()
        },
        job_need::Model {
            decided_on: Some(date("2026-11-01")),
            ..n.clone()
        },
        job_need::Model {
            supplied_by: Some(Supplier::Owner),
            decision_reason: Some("cheap".into()),
            ..n.clone()
        },
    ];
    for row in unreadable {
        let err = Decision::of(&row).unwrap_err();
        assert!(matches!(err, DomainError::Internal(_)), "{err:?}");
    }
}

#[tokio::test]
async fn each_decision_reads_back_as_written() {
    let db = test_db().await;
    let (vehicle_id, job_id) = seed_job(&db).await;
    let n = shop_need(&db, vehicle_id, job_id).await;
    let o = add_option(&db, vehicle_id, n.id, option("Pads"))
        .await
        .unwrap();
    for decision in [
        Decision::Undecided,
        Decision::ShopSupplies { reason: None },
        Decision::ShopSupplies {
            reason: Some("OEM only".into()),
        },
        Decision::Chosen {
            option_id: o.id,
            on: date("2026-11-01"),
            reason: Some("quiet".into()),
        },
    ] {
        let supplier = match decision {
            Decision::Undecided => None,
            _ => Some(Supplier::Shop),
        };
        let mut active: job_need::ActiveModel = n.clone().into();
        active.supplied_by = Set(supplier);
        decision.clone().write(&mut active);
        let stored = active.update(&db).await.unwrap();
        assert_eq!(Decision::of(&stored).unwrap(), decision);
    }
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
    let shop = shop_need(&db, vehicle_id, job_id).await;
    update(&db, vehicle_id, shop.id, reason("OEM only"))
        .await
        .unwrap();
    let clear = UpdateNeed {
        decision_reason: Some(None),
        ..Default::default()
    };
    let cleared = update(&db, vehicle_id, shop.id, clear).await.unwrap();
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
    let edited = update(
        &db,
        vehicle_id,
        shop.id,
        UpdateNeed {
            supplied_by: Some(Some(Supplier::Shop)),
            approach: Some(Some(job_need::Approach::Upgrade)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(edited.decision_reason.as_deref(), Some("OEM only"));
}

#[tokio::test]
async fn an_unrelated_edit_keeps_a_stored_reason() {
    let db = test_db().await;
    let (vehicle_id, job_id) = seed_job(&db).await;
    let n = shop_need(&db, vehicle_id, job_id).await;
    update(&db, vehicle_id, n.id, reason("OEM only"))
        .await
        .unwrap();
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
    assert_eq!(renamed.decision_reason.as_deref(), Some("OEM only"));
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
    // Bought before every arrival date the tests give.
    let details = OrderDetails {
        purchase_date: Some(date("2026-11-01")),
        ..OrderDetails::default()
    };
    mark_ordered(db, vehicle_id, n.id, details).await.unwrap();
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
    assert_eq!(bought.part.part.name, o.description);
    assert_eq!(bought.part.part.manufacturer.as_deref(), Some("Zimmermann"));
    assert_eq!(bought.part.part.part_number.as_deref(), Some("100.3394.20"));
    assert_eq!(bought.part.part.seller.as_deref(), Some("FCP Euro"));
    assert_eq!(bought.part.part.cost_cents, Some(18_000));
    assert_eq!(
        bought.part.part.retailer_url.as_deref(),
        Some("https://example.com/rotors")
    );
    assert_eq!(bought.part.part.purchase_date, Some(today));
    assert_eq!(bought.part.part.supplied_by, Supplier::Owner);
    assert_eq!(bought.part.part.status, PartStatus::Purchased);
    assert_eq!(bought.need.part_id, Some(bought.part.part.id));
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
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(bought.part.part.seller.as_deref(), Some("Amazon"));
    assert_eq!(bought.part.part.purchase_date, Some(date("2026-11-02")));
    assert_eq!(bought.part.part.cost_cents, Some(8_500));
    assert_eq!(bought.part.part.order_ref.as_deref(), Some("112-445"));
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
    assert_eq!(bought.part.part.cost_cents, Some(13_800));
}

#[tokio::test]
async fn mark_ordered_records_a_kits_contents() {
    let db = test_db().await;
    let (vehicle_id, job_id) = seed_job(&db).await;
    let (n, _) = chosen_need(&db, vehicle_id, job_id, "Oil change", None, None).await;
    let item = |name: &str| crate::inputs::part::NewPartContent {
        name: name.into(),
        ..Default::default()
    };

    let bought = mark_ordered(
        &db,
        vehicle_id,
        n.id,
        OrderDetails {
            contents: vec![item("Oil filter"), item("Drain plug")],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let names: Vec<&str> = bought
        .part
        .contents
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, ["Oil filter", "Drain plug"]);
}

#[tokio::test]
async fn a_bad_kit_item_refuses_the_order_and_leaves_the_need_unordered() {
    let db = test_db().await;
    let (vehicle_id, job_id) = seed_job(&db).await;
    let (n, _) = chosen_need(&db, vehicle_id, job_id, "Oil change", None, None).await;

    let before = stage(&n, None);
    let err = mark_ordered(
        &db,
        vehicle_id,
        n.id,
        OrderDetails {
            contents: vec![crate::inputs::part::NewPartContent {
                name: " ".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
    )
    .await
    .unwrap_err();

    assert!(matches!(err, DomainError::Invalid { .. }), "{err:?}");
    assert_eq!(row_count::<part::Entity>(&db).await, 0);
    let n = require(&db, vehicle_id, n.id).await.unwrap();
    assert_eq!(n.part_id, None);
    assert_eq!(stage(&n, None), before);
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
    let (_dir, pools) = race_pools().await;
    let first = &pools[0];
    let (vehicle_id, job_id) = seed_job(first).await;
    let (n, _) = chosen_need(
        first,
        vehicle_id,
        job_id,
        "Rear rotors",
        Some("FCP Euro"),
        Some(13_800),
    )
    .await;

    let results = crate::test_support::race(&pools, |_, db| async move {
        mark_ordered(&db, vehicle_id, n.id, OrderDetails::default()).await
    })
    .await;

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
    let (_dir, pools) = race_pools().await;
    let first = &pools[0];
    let (vehicle_id, job_id) = seed_job(first).await;
    let (n, o) = chosen_need(first, vehicle_id, job_id, "Pads", None, None).await;

    let results = crate::test_support::race(&pools, |i, db| async move {
        let input = UpdateNeed {
            decision_reason: Some(Some(format!("reason {i}"))),
            ..Default::default()
        };
        if i % 2 == 0 {
            update_with_choice(&db, vehicle_id, n.id, input, Some(Some(o.id))).await
        } else {
            update(&db, vehicle_id, n.id, input).await
        }
    })
    .await;

    assert!(results.iter().all(Result::is_ok), "{results:?}");
}

// Arrival writes racing on one file from several pools all succeed: each
// transaction takes the write lock before it reads the stage.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_arrival_writes_on_one_need_all_succeed() {
    let (_dir, pools) = race_pools().await;
    let first = &pools[0];
    let (vehicle_id, job_id) = seed_job(first).await;
    let (n, _) = ordered_need(first, vehicle_id, job_id).await;

    let results = crate::test_support::race(&pools, |i, db| async move {
        if i % 2 == 0 {
            mark_received(&db, vehicle_id, n.id, Some(date("2026-11-01"))).await
        } else {
            clear_received(&db, vehicle_id, n.id).await
        }
    })
    .await;

    assert!(results.iter().all(Result::is_ok), "{results:?}");
}

// Links and unlinks racing on one file from several pools never fail on
// the database: each one succeeds or is refused for the need's state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_links_and_unlinks_on_one_need_never_fail_on_the_database() {
    let (_dir, pools) = race_pools().await;
    let first = &pools[0];
    let (vehicle_id, job_id) = seed_job(first).await;
    let n = add(first, vehicle_id, job_id, need("Fluid")).await.unwrap();
    let p = new_part(first, vehicle_id, "Fluid").await;

    let mut writes = tokio::task::JoinSet::new();
    for (i, db) in pools.iter().chain(pools.iter()).cloned().enumerate() {
        writes.spawn(async move {
            let mut results = Vec::new();
            for round in 0..30 {
                let is_link = (i + round) % 2 == 0;
                let result = if is_link {
                    link_part(&db, vehicle_id, n.id, p.id, Some(date("2026-11-01")))
                        .await
                        .map(|_| ())
                } else {
                    unlink_part(&db, vehicle_id, n.id).await.map(|_| ())
                };
                results.push((is_link, result));
            }
            results
        });
    }
    let results: Vec<_> = writes.join_all().await.into_iter().flatten().collect();

    for (_, result) in &results {
        assert!(
            matches!(result, Ok(()) | Err(DomainError::BadRequest(_))),
            "{results:?}"
        );
    }
    let links = results
        .iter()
        .filter(|(is_link, result)| *is_link && result.is_ok())
        .count();
    assert!(links > 0, "{results:?}");
    // The need and the part agree: the part is linked to this need or to
    // none, and it keeps the arrival date every link gave it.
    let after = require(first, vehicle_id, n.id).await.unwrap();
    let holders = job_need::Entity::find()
        .filter(job_need::Column::PartId.eq(p.id))
        .all(first)
        .await
        .unwrap();
    match after.part_id {
        Some(id) => {
            assert_eq!(id, p.id);
            assert_eq!(holders.len(), 1);
        }
        None => assert!(holders.is_empty(), "{holders:?}"),
    }
    assert_eq!(
        received_on(first, vehicle_id, p.id).await,
        Some(date("2026-11-01"))
    );
}

// Returns racing on one file from several pools return the part once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_returns_of_one_part_return_it_once() {
    let (_dir, pools) = race_pools().await;
    let first = &pools[0];
    let (vehicle_id, job_id) = seed_job(first).await;
    let (n, _) = ordered_need(first, vehicle_id, job_id).await;

    let results = crate::test_support::race(&pools, |_, db| async move {
        let input = ReturnInput {
            reason: "Wrong size".into(),
            option_was_wrong: false,
        };
        return_part(&db, vehicle_id, n.id, input).await.map(|_| ())
    })
    .await;

    assert_eq!(
        results.iter().filter(|r| r.is_ok()).count(),
        1,
        "{results:?}"
    );
    for err in results.iter().filter_map(|r| r.as_ref().err()) {
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
    }
    assert_eq!(row_count::<todo::Entity>(&pools[0]).await, 1);
}

// A correction that waits on a return reads the returned part once the
// return commits, so it refuses rather than install a part that is gone.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_correction_racing_a_return_sees_the_returned_part() {
    let (_dir, pools) = race_pools().await;
    let first = &pools[0];
    let second = pools[1].clone();
    let (vehicle_id, job_id) = seed_job(first).await;
    let (n, _) = ordered_need(first, vehicle_id, job_id).await;
    let part_id = n.part_id.unwrap();

    let txn = crate::services::begin_write(first).await.unwrap();
    let input = ReturnInput {
        reason: "Wrong size".into(),
        option_was_wrong: false,
    };
    return_part(&txn, vehicle_id, n.id, input).await.unwrap();
    let correcting = tokio::spawn(async move {
        let input = crate::inputs::part::UpdatePart {
            fitting: crate::inputs::part::Fitting::from_fields(
                Some(PartStatus::Installed),
                crate::inputs::part::InstallPatch::default(),
                crate::inputs::part::ReplacePatch::default(),
            )
            .unwrap(),
            ..Default::default()
        };
        crate::services::part::update(&second, vehicle_id, part_id, input).await
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(
        !correcting.is_finished(),
        "the correction ran ahead of the open return"
    );
    txn.commit().await.unwrap();

    let err = correcting.await.unwrap().unwrap_err();
    assert!(err.to_string().contains("was returned"), "{err}");
    let p = crate::services::part::get(first, vehicle_id, part_id)
        .await
        .unwrap();
    assert_eq!(p.status, PartStatus::Returned);
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
    let mut returned: part::ActiveModel = p.clone().into();
    crate::services::part::PartState::Returned {
        on: date("2026-11-06"),
        reason: "Wrong size".into(),
        need_id: None,
    }
    .write(&mut returned);
    returned.update(&db).await.unwrap();
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
    assert_eq!(linked.part.part.received_on, Some(date("2026-11-05")));
    assert_eq!(linked.stage, Stage::Received);
    assert_eq!(linked.need.supplied_by, Some(Supplier::Owner));

    let m = add(&db, vehicle_id, job_id, need("Rotors")).await.unwrap();
    let q = new_part(&db, vehicle_id, "Rotors").await;
    let linked = link_part(&db, vehicle_id, m.id, q.id, None).await.unwrap();
    assert_eq!(linked.part.part.received_on, None);
    assert_eq!(linked.stage, Stage::Ordered);
}

#[tokio::test]
async fn a_need_part_is_never_received_before_it_was_bought() {
    let db = test_db().await;
    let (vehicle_id, job_id) = seed_job(&db).await;
    let (n, _) = ordered_need(&db, vehicle_id, job_id).await;
    let err = mark_received(&db, vehicle_id, n.id, Some(date("2026-10-31")))
        .await
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "received_on: is before the purchase date 2026-11-01; correct the purchase date first if \
         it is wrong"
    );
    let part = new_part(&db, vehicle_id, "Pads").await;
    let mut dated: part::ActiveModel = part.into();
    dated.purchase_date = Set(Some(date("2026-11-01")));
    let part = dated.update(&db).await.unwrap();
    let other = add(&db, vehicle_id, job_id, need("Pads")).await.unwrap();
    let err = link_part(&db, vehicle_id, other.id, part.id, Some(date("2026-10-31")))
        .await
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "received_on: is before the purchase date 2026-11-01; correct the purchase date first if \
         it is wrong"
    );
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
    assert_eq!(got.part.part.received_on, Some(date("2026-11-07")));
    assert_eq!(got.stage, Stage::Received);
    let cleared = clear_received(&db, vehicle_id, n.id).await.unwrap();
    assert_eq!(cleared.part.part.received_on, None);
    assert_eq!(cleared.stage, Stage::Ordered);
    let today = crate::services::today();
    let got = mark_received(&db, vehicle_id, n.id, None).await.unwrap();
    assert_eq!(got.part.part.received_on, Some(today));
    // With no date given, a part that has one keeps it.
    let err = mark_received(&db, vehicle_id, n.id, None)
        .await
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        format!("Need {}'s part was already received on {today}", n.id)
    );
    assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
    let part_id = got.part.part.id;
    assert_eq!(received_on(&db, vehicle_id, part_id).await, Some(today));

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
    assert_eq!(out.returned_part.part.status, PartStatus::Returned);
    assert_eq!(
        out.returned_part.part.return_reason.as_deref(),
        Some("wrong diameter")
    );
    assert_eq!(out.returned_part.part.returned_on, Some(today));
    assert_eq!(out.returned_part.part.returned_from_need_id, Some(n.id));
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
    let part_id = m.part_id.unwrap();
    fit(&db, vehicle_id, part_id, PartStatus::Installed).await;
    let err = return_part(&db, vehicle_id, m.id, input())
        .await
        .unwrap_err();
    assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
    assert_eq!(
        err.to_string(),
        format!("Part {part_id} is installed; only an ordered or received part can be returned")
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
            fitting: crate::inputs::part::Fitting::from_fields(
                Some(status),
                crate::inputs::part::InstallPatch::default(),
                crate::inputs::part::ReplacePatch::default(),
            )
            .unwrap(),
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
                "Part {part_id} is {status}; only an ordered or received part can be marked \
                 received"
            ),
            "{status:?}"
        );
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        let err = clear_received(&db, vehicle_id, n.id).await.unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "Part {part_id} is {status}; only an ordered or received part can have its \
                 arrival date cleared"
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
    assert_eq!(purchase.part.part.received_on, Some(date("2026-11-01")));
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
    assert_eq!(got.part.part.received_on, Some(date("2026-11-01")));
    assert_eq!(got.stage, Stage::Received);
    let got = set_arrival(&db, vehicle_id, n.id, ArrivalChange::NotReceived)
        .await
        .unwrap();
    assert_eq!(got.part.part.received_on, None);
    assert_eq!(got.stage, Stage::Ordered);
}

#[tokio::test]
async fn linking_with_a_date_refuses_a_fitted_part() {
    let db = test_db().await;
    let (vehicle_id, job_id) = seed_job(&db).await;
    // The need's own part goes on the car and is linked again with a new
    // date.
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
        format!("Part {own} is installed; only an ordered or received part can be marked received")
    );
    assert_eq!(
        received_on(&db, vehicle_id, own).await,
        Some(date("2026-11-01"))
    );

    // A replaced part that no need holds is linked to a bare need with a
    // date.
    let bare = add(&db, vehicle_id, job_id, need("Fluid")).await.unwrap();
    let loose = new_part(&db, vehicle_id, "Fluid").await;
    fit(&db, vehicle_id, loose.id, PartStatus::Replaced).await;
    let err = link_part(&db, vehicle_id, bare.id, loose.id, Some(date("2026-11-03")))
        .await
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "Part {} is replaced; only an ordered or received part can be marked received",
            loose.id
        )
    );
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
    assert_eq!(linked.part.part.received_on, None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn racing_adds_options_and_deletes_all_succeed() {
    let (_dir, pools) = race_pools().await;
    let first = &pools[0];
    let (v, job) = seed_job(first).await;
    let mut ids = Vec::new();
    for i in 0..pools.len() * 2 {
        ids.push(
            add(first, v, job, need(&format!("Need {i}")))
                .await
                .unwrap()
                .id,
        );
    }
    let results = crate::test_support::race(&pools, |i, db| {
        let id = ids[i];
        async move {
            match i % 3 {
                0 => add(&db, v, job, need("New need")).await.map(|_| ()),
                1 => add_option(&db, v, id, option("Pads")).await.map(|_| ()),
                _ => delete(&db, v, id).await,
            }
        }
    })
    .await;
    assert!(results.iter().all(Result::is_ok), "{results:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn racing_choices_on_one_need_all_succeed() {
    let (_dir, pools) = race_pools().await;
    let first = &pools[0];
    let (v, job) = seed_job(first).await;
    let n = add(first, v, job, need("Pads")).await.unwrap().id;
    let a = add_option(first, v, n, option("Brand A")).await.unwrap().id;
    let b = add_option(first, v, n, option("Brand B")).await.unwrap().id;
    let results = crate::test_support::race(&pools, |i, db| async move {
        let chosen = if i % 2 == 0 { a } else { b };
        decide(&db, v, n, Some(chosen), None).await
    })
    .await;
    assert!(results.iter().all(Result::is_ok), "{results:?}");
}
