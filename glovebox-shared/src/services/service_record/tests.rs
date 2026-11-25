use super::*;
use crate::{
    entities::{part::Supplier, service_record_line_item::WorkKind, work_item::JobStatus},
    inputs::{incident::NewIncident, part::NewPart},
    services::incident,
    test_support::{VehicleFixture, date, row_count, test_db},
};

use crate::test_support::seed_vehicle;

async fn seed_part(db: &impl ConnectionTrait, vehicle_id: i32, name: &str) -> i32 {
    part::ActiveModel {
        vehicle_id: Set(vehicle_id),
        name: Set(name.into()),
        status: Set(PartStatus::Purchased),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id
}

async fn seed_schedule_item(db: &impl ConnectionTrait, vehicle_id: i32, name: &str) -> i32 {
    use crate::entities::maintenance_schedule_item;
    maintenance_schedule_item::ActiveModel {
        vehicle_id: Set(Some(vehicle_id)),
        name: Set(name.into()),
        enabled: Set(true),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id
}

#[tokio::test]
async fn create_wires_line_items_schedule_links_and_parts() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let part_id = seed_part(&db, vid, "Oil filter").await;
    let sched_id = seed_schedule_item(&db, vid, "Oil change").await;

    let created = create(
        &db,
        vid,
        NewServiceRecord {
            service_date: date("2024-03-01"),
            mileage: Some(50_000),
            description: Some("Oil change".into()),
            parts_cost_cents: None,
            labor_cost_cents: None,
            total_cost_cents: Some(6_000),
            shop: None,
            notes: None,
            paid_by: None,
            payer_note: None,
            schedule_item_ids: Some(vec![sched_id]),
            part_ids: Some(vec![part_id]),
            line_items: Some(vec![NewLineItem {
                paid_by: None,
                description: "Synthetic oil".into(),
                work_kind: WorkKind::Maintenance,
                quantity: Some(5.0),
                unit_cost_cents: Some(1_000),
                cost_cents: Some(5_000),
            }]),
            invoice_ref: None,
            end_date: None,
            performed_by: None,
            work_item_ids: None,
            resolved_incident_ids: None,
            noticed_incident_ids: None,
            recall_finding_ids: None,
            parts: None,
            appointment_id: None,
        },
    )
    .await
    .unwrap();

    // The record links back to schedule item, part, and line item
    assert_eq!(created.schedule_item_ids, vec![sched_id]);
    assert_eq!(created.part_ids, vec![part_id]);
    assert_eq!(created.line_items.len(), 1);
    assert_eq!(created.line_items[0].description, "Synthetic oil");

    // The linked part was marked installed and wired to this service
    let p = part::Entity::find_by_id(part_id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(p.status, PartStatus::Installed);
    assert_eq!(p.installed_service_id, Some(created.record.id));
    assert_eq!(p.installed_odometer, Some(50_000));

    // A mileage log entry was created for the provided mileage
    let logs = mileage_log::Entity::find()
        .filter(mileage_log::Column::VehicleId.eq(vid))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].mileage, 50_000);

    // get() reads back the same wiring
    let fetched = get(&db, vid, created.record.id).await.unwrap();
    assert_eq!(fetched.schedule_item_ids, vec![sched_id]);
    assert_eq!(fetched.part_ids, vec![part_id]);
    assert_eq!(fetched.line_items.len(), 1);
}

#[tokio::test]
async fn update_replaces_line_items_and_relinks_parts() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let part_a = seed_part(&db, vid, "Part A").await;
    let part_b = seed_part(&db, vid, "Part B").await;

    let created = create(
        &db,
        vid,
        NewServiceRecord {
            service_date: date("2024-03-01"),
            mileage: Some(1_000),
            description: None,
            parts_cost_cents: None,
            labor_cost_cents: None,
            total_cost_cents: None,
            shop: None,
            notes: None,
            paid_by: None,
            payer_note: None,
            schedule_item_ids: None,
            part_ids: Some(vec![part_a]),
            line_items: Some(vec![NewLineItem {
                paid_by: None,
                description: "old".into(),
                work_kind: WorkKind::Maintenance,
                quantity: None,
                unit_cost_cents: None,
                cost_cents: Some(100),
            }]),
            invoice_ref: None,
            end_date: None,
            performed_by: None,
            work_item_ids: None,
            resolved_incident_ids: None,
            noticed_incident_ids: None,
            recall_finding_ids: None,
            parts: None,
            appointment_id: None,
        },
    )
    .await
    .unwrap();

    let updated = update(
        &db,
        vid,
        created.record.id,
        UpdateServiceRecord {
            part_ids: Some(vec![part_b]),
            completed_work: Some(CompletedWork::DidNotHappen),
            line_items: Some(vec![NewLineItem {
                paid_by: None,
                description: "new".into(),
                work_kind: WorkKind::Maintenance,
                quantity: None,
                unit_cost_cents: None,
                cost_cents: Some(200),
            }]),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Line items replaced
    assert_eq!(updated.line_items.len(), 1);
    assert_eq!(updated.line_items[0].description, "new");
    // Parts relinked: A unlinked (reverted to purchased), B installed
    assert_eq!(updated.part_ids, vec![part_b]);
    let a = part::Entity::find_by_id(part_a)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(a.status, PartStatus::Purchased);
    assert_eq!(a.installed_service_id, None);
    let b = part::Entity::find_by_id(part_b)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(b.status, PartStatus::Installed);
    assert_eq!(b.installed_service_id, Some(created.record.id));
}

#[tokio::test]
async fn delete_unlinks_parts_and_removes_links() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let part_id = seed_part(&db, vid, "Part").await;
    let sched_id = seed_schedule_item(&db, vid, "Item").await;

    let created = create(
        &db,
        vid,
        NewServiceRecord {
            service_date: date("2024-03-01"),
            mileage: None,
            description: None,
            parts_cost_cents: None,
            labor_cost_cents: None,
            total_cost_cents: None,
            shop: None,
            notes: None,
            paid_by: None,
            payer_note: None,
            schedule_item_ids: Some(vec![sched_id]),
            part_ids: Some(vec![part_id]),
            line_items: None,
            invoice_ref: None,
            end_date: None,
            performed_by: None,
            work_item_ids: None,
            resolved_incident_ids: None,
            noticed_incident_ids: None,
            recall_finding_ids: None,
            parts: None,
            appointment_id: None,
        },
    )
    .await
    .unwrap();

    let kept = delete(&db, vid, created.record.id, UNDO)
        .await
        .unwrap()
        .kept_part_ids;
    assert_eq!(kept, vec![part_id], "the kept parts are reported");

    // Record gone
    assert!(matches!(
        get(&db, vid, created.record.id).await.unwrap_err(),
        DomainError::NotFound(_)
    ));
    // Part reverted
    let p = part::Entity::find_by_id(part_id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(p.status, PartStatus::Purchased);
    assert_eq!(p.installed_service_id, None);
    // Schedule links removed
    let links = service_schedule_link::Entity::find()
        .all(&db)
        .await
        .unwrap();
    crate::test_support::assert_empty(&links);
}

#[tokio::test]
async fn delete_clears_finding_links_and_removes_incident_links() {
    use crate::entities::incident;

    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let svc_id = service_record::ActiveModel {
        vehicle_id: Set(vid),
        service_date: Set(date("2024-03-01")),
        paid_by: Set(Payer::Owner),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;

    // An incident linked to the record via the M2M link table.
    let inc = incident::ActiveModel {
        vehicle_id: Set(vid),
        category: Set(IncidentCategory::Noise),
        title: Set("Rattle".into()),
        occurred_at: Set(date("2024-02-01")),
        resolved: Set(false),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    incident_service_link::ActiveModel {
        incident_id: Set(inc.id),
        service_record_id: Set(svc_id),
        role: Set(LinkRole::Resolved),
    }
    .insert(&db)
    .await
    .unwrap();

    delete(&db, vid, svc_id, UNDO).await.unwrap();

    // The incident link rows are gone.
    let links = incident_service_link::Entity::find()
        .filter(incident_service_link::Column::ServiceRecordId.eq(svc_id))
        .all(&db)
        .await
        .unwrap();
    crate::test_support::assert_empty(&links);
    // The incident itself survives.
    assert!(
        incident::Entity::find_by_id(inc.id)
            .one(&db)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn create_links_auto_mileage_log_via_fk() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;

    let created = create(
        &db,
        vid,
        NewServiceRecord {
            mileage: Some(50_000),
            ..minimal_record(None)
        },
    )
    .await
    .unwrap();

    let logs = mileage_log::Entity::find()
        .filter(mileage_log::Column::VehicleId.eq(vid))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].service_record_id, Some(created.record.id));
    assert_eq!(logs[0].source.as_deref(), Some("service"));
}

#[tokio::test]
async fn delete_removes_auto_created_mileage_log() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;

    let created = create(
        &db,
        vid,
        NewServiceRecord {
            mileage: Some(48_000),
            ..minimal_record(None)
        },
    )
    .await
    .unwrap();

    // A manual log must survive the service delete.
    let manual = mileage_log::ActiveModel {
        vehicle_id: Set(vid),
        mileage: Set(47_000),
        recorded_at: Set(date("2024-02-01 09:00:00")),
        source: Set(Some("manual".into())),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    delete(&db, vid, created.record.id, UNDO).await.unwrap();

    let logs = mileage_log::Entity::find()
        .filter(mileage_log::Column::VehicleId.eq(vid))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(
        logs.iter().map(|l| l.id).collect::<Vec<_>>(),
        vec![manual.id],
        "only the manual log survives; the auto-log is gone from the DB"
    );
}

#[tokio::test]
async fn update_syncs_auto_mileage_log() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;

    let created = create(
        &db,
        vid,
        NewServiceRecord {
            mileage: Some(50_000),
            ..minimal_record(None)
        },
    )
    .await
    .unwrap();

    // Changing the record's mileage updates the linked auto-log in place.
    update(
        &db,
        vid,
        created.record.id,
        UpdateServiceRecord {
            mileage: Some(Some(51_000)),
            service_date: Some(date("2024-04-01")),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let logs = mileage_log::Entity::find()
        .filter(mileage_log::Column::ServiceRecordId.eq(created.record.id))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].mileage, 51_000);
    assert_eq!(logs[0].recorded_at, date("2024-04-01"));

    // Clearing the mileage removes the auto-log.
    update(
        &db,
        vid,
        created.record.id,
        UpdateServiceRecord {
            mileage: Some(None),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    crate::test_support::assert_empty(
        &mileage_log::Entity::find()
            .filter(mileage_log::Column::ServiceRecordId.eq(created.record.id))
            .all(&db)
            .await
            .unwrap(),
    );

    // Setting mileage again (no log left) creates a fresh linked auto-log.
    update(
        &db,
        vid,
        created.record.id,
        UpdateServiceRecord {
            mileage: Some(Some(52_000)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let logs = mileage_log::Entity::find()
        .filter(mileage_log::Column::ServiceRecordId.eq(created.record.id))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].mileage, 52_000);
    assert_eq!(logs[0].source.as_deref(), Some("service"));
}

#[tokio::test]
async fn get_missing_is_not_found() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    assert!(matches!(
        get(&db, vid, 999).await.unwrap_err(),
        DomainError::NotFound(_)
    ));
}

#[tokio::test]
async fn a_shop_id_that_names_no_shop_is_not_found() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let err = create(
        &db,
        vid,
        NewServiceRecord {
            shop: Some(ShopChoice::Id(9999)),
            ..minimal_record(None)
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.to_string(), "Shop 9999 not found");
    crate::test_support::assert_empty(&list_between(&db, vid, None, None).await.unwrap());

    let made = create(&db, vid, minimal_record(None)).await.unwrap();
    let err = update(
        &db,
        vid,
        made.record.id,
        UpdateServiceRecord {
            shop: Some(Some(ShopChoice::Id(9999))),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.to_string(), "Shop 9999 not found");
}

fn minimal_record(schedule_item_ids: Option<Vec<i32>>) -> NewServiceRecord {
    NewServiceRecord {
        service_date: date("2024-03-01"),
        mileage: None,
        description: None,
        parts_cost_cents: None,
        labor_cost_cents: None,
        total_cost_cents: None,
        shop: None,
        notes: None,
        paid_by: None,
        payer_note: None,
        schedule_item_ids,
        part_ids: None,
        line_items: None,
        invoice_ref: None,
        end_date: None,
        performed_by: None,
        work_item_ids: None,
        resolved_incident_ids: None,
        noticed_incident_ids: None,
        recall_finding_ids: None,
        parts: None,
        appointment_id: None,
    }
}

// ─── invoice_ref idempotency + possible_duplicates advisory (hwaf) ───

// Recording the same invoice_ref twice returns the existing record
// UNTOUCHED — no second row, and the second call's extra links/line-items
// (and even a bogus schedule id, which the short-circuit skips guarding)
// are ignored.
#[tokio::test]
async fn record_same_invoice_ref_is_hard_idempotent_and_untouched() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;

    let first = create(
        &db,
        vid,
        NewServiceRecord {
            invoice_ref: Some("INV-42".into()),
            ..minimal_record(None)
        },
    )
    .await
    .unwrap();
    assert!(first.line_items.is_empty());

    let second = create(
        &db,
        vid,
        NewServiceRecord {
            invoice_ref: Some("INV-42".into()),
            // These MUST be ignored on the idempotent path — a bogus
            // schedule id would fail the guard if it ran.
            schedule_item_ids: Some(vec![9999]),
            line_items: Some(vec![NewLineItem {
                paid_by: None,
                description: "should not be added".into(),
                work_kind: WorkKind::Maintenance,
                quantity: None,
                unit_cost_cents: None,
                cost_cents: Some(100),
            }]),
            ..minimal_record(None)
        },
    )
    .await
    .unwrap();

    assert_eq!(second.record.id, first.record.id, "same ref → same record");
    assert!(
        second.line_items.is_empty(),
        "idempotent return must not add the 2nd call's line items"
    );
    assert_eq!(list(&db, vid).await.unwrap().len(), 1, "no duplicate row");
}

// Distinct invoice_refs are distinct records.
#[tokio::test]
async fn record_different_invoice_refs_are_distinct() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    for reference in ["A-1", "A-2"] {
        create(
            &db,
            vid,
            NewServiceRecord {
                invoice_ref: Some(reference.into()),
                ..minimal_record(None)
            },
        )
        .await
        .unwrap();
    }
    assert_eq!(list(&db, vid).await.unwrap().len(), 2);
}

// Regression: two records with the SAME day and SAME cost but NO
// invoice_ref must BOTH persist — resemblance is never a dedup signal.
#[tokio::test]
async fn record_no_ref_same_day_same_cost_coexist() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    for _ in 0..2 {
        create(
            &db,
            vid,
            NewServiceRecord {
                total_cost_cents: Some(5_000),
                ..minimal_record(None)
            },
        )
        .await
        .unwrap();
    }
    assert_eq!(
        list(&db, vid).await.unwrap().len(),
        2,
        "distinct same-day/same-cost records must coexist"
    );
}

// A ref-less create surfaces same-day peers as a soft advisory; the first
// (no peers) is empty, the second sees the first.
#[tokio::test]
async fn record_no_ref_surfaces_possible_duplicates() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;

    let first = create(&db, vid, minimal_record(None)).await.unwrap();
    assert!(
        first.possible_duplicates.is_empty(),
        "no prior same-day record → no advisory"
    );

    let second = create(&db, vid, minimal_record(None)).await.unwrap();
    assert_eq!(second.possible_duplicates.len(), 1);
    assert_eq!(second.possible_duplicates[0].id, first.record.id);
}

// A blank invoice_ref is normalized to absent: two ref-less records sent
// with `Some("")`/whitespace must coexist, not collide on the unique
// index. And a ref with surrounding whitespace is the same identity.
#[tokio::test]
async fn blank_invoice_ref_is_treated_as_absent() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;

    for reference in ["", "   "] {
        create(
            &db,
            vid,
            NewServiceRecord {
                invoice_ref: Some(reference.into()),
                ..minimal_record(None)
            },
        )
        .await
        .expect("blank ref must not collide");
    }
    assert_eq!(list(&db, vid).await.unwrap().len(), 2, "both must persist");

    // Whitespace around a real ref is trimmed → same identity → idempotent.
    let a = create(
        &db,
        vid,
        NewServiceRecord {
            invoice_ref: Some("INV-9".into()),
            ..minimal_record(None)
        },
    )
    .await
    .unwrap();
    let b = create(
        &db,
        vid,
        NewServiceRecord {
            invoice_ref: Some("  INV-9  ".into()),
            ..minimal_record(None)
        },
    )
    .await
    .unwrap();
    assert_eq!(b.record.id, a.record.id, "trimmed ref is the same identity");
}

// The partial unique index is a DB-level backstop: a raw second insert of
// the same (vehicle_id, invoice_ref) — bypassing the service's
// short-circuit — is rejected.
#[tokio::test]
async fn invoice_ref_partial_unique_index_rejects_raw_duplicate() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;

    let insert_raw = |reference: &str| {
        let reference = reference.to_string();
        service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set(date("2024-03-01")),
            paid_by: Set(Payer::Owner),
            invoice_ref: Set(Some(reference)),
            ..Default::default()
        }
    };
    insert_raw("DUP-1").insert(&db).await.unwrap();
    let err = insert_raw("DUP-1").insert(&db).await;
    assert!(
        err.is_err(),
        "duplicate (vehicle, invoice_ref) must be rejected"
    );

    // NULL refs are exempt from the partial index — two coexist.
    for _ in 0..2 {
        service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set(date("2024-03-01")),
            paid_by: Set(Payer::Owner),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn create_round_trips_payer_and_defaults_to_self() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;

    // Explicit payer round-trips, note included.
    let insured = create(
        &db,
        vid,
        NewServiceRecord {
            paid_by: Some(Payer::Insurance),
            payer_note: Some("Progressive claim #12345".into()),
            ..minimal_record(None)
        },
    )
    .await
    .unwrap();
    assert_eq!(insured.record.paid_by, Payer::Insurance);
    assert_eq!(
        insured.record.payer_note.as_deref(),
        Some("Progressive claim #12345")
    );
    let fetched = get(&db, vid, insured.record.id).await.unwrap();
    assert_eq!(fetched.record.paid_by, Payer::Insurance);
    assert_eq!(
        fetched.record.payer_note.as_deref(),
        Some("Progressive claim #12345")
    );

    // Omitted payer defaults to self.
    let plain = create(&db, vid, minimal_record(None)).await.unwrap();
    assert_eq!(plain.record.paid_by, Payer::Owner);
    assert_eq!(plain.record.payer_note, None);
}

#[tokio::test]
async fn update_changes_payer_and_sets_then_clears_note() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let created = create(&db, vid, minimal_record(None)).await.unwrap();
    assert_eq!(created.record.paid_by, Payer::Owner);

    // Change payer + set the note.
    let updated = update(
        &db,
        vid,
        created.record.id,
        UpdateServiceRecord {
            paid_by: Some(Payer::ThirdParty),
            payer_note: Some(Some("Neighbor's insurance, side-swipe".into())),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.record.paid_by, Payer::ThirdParty);
    assert_eq!(
        updated.record.payer_note.as_deref(),
        Some("Neighbor's insurance, side-swipe")
    );

    // An update not mentioning payer fields leaves them alone.
    let untouched = update(
        &db,
        vid,
        created.record.id,
        UpdateServiceRecord {
            notes: Some(Some("unrelated".into())),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(untouched.record.paid_by, Payer::ThirdParty);
    assert!(untouched.record.payer_note.is_some());

    // Explicit null clears the note (double-option).
    let cleared = update(
        &db,
        vid,
        created.record.id,
        UpdateServiceRecord {
            payer_note: Some(None),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(cleared.record.payer_note, None);
}

#[tokio::test]
async fn create_rejects_other_vehicles_schedule_items() {
    let db = test_db().await;
    let owner = seed_vehicle(&db).await;
    let other = seed_vehicle(&db).await;
    let foreign_item = seed_schedule_item(&db, other, "Foreign").await;

    // Linking another vehicle's schedule item must 404 and create nothing.
    assert!(matches!(
        create(&db, owner, minimal_record(Some(vec![foreign_item])))
            .await
            .unwrap_err(),
        DomainError::NotFound(_)
    ));
    assert!(list(&db, owner).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_repeated_schedule_item_id_links_once() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let item = seed_schedule_item(&db, vid, "Oil change").await;

    let created = create(&db, vid, minimal_record(Some(vec![item, item])))
        .await
        .unwrap();
    assert_eq!(created.schedule_item_ids, vec![item]);

    let updated = update(
        &db,
        vid,
        created.record.id,
        UpdateServiceRecord {
            schedule_item_ids: Some(vec![item, item]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.schedule_item_ids, vec![item]);
}

#[tokio::test]
async fn update_rejects_other_vehicles_schedule_items() {
    let db = test_db().await;
    let owner = seed_vehicle(&db).await;
    let other = seed_vehicle(&db).await;
    let own_item = seed_schedule_item(&db, owner, "Mine").await;
    let foreign_item = seed_schedule_item(&db, other, "Foreign").await;
    let created = create(&db, owner, minimal_record(Some(vec![own_item])))
        .await
        .unwrap();

    assert!(matches!(
        update(
            &db,
            owner,
            created.record.id,
            UpdateServiceRecord {
                schedule_item_ids: Some(vec![foreign_item]),
                ..Default::default()
            },
        )
        .await
        .unwrap_err(),
        DomainError::NotFound(_)
    ));
    // Existing links survive the rejected update (transaction rolled back).
    let fetched = get(&db, owner, created.record.id).await.unwrap();
    assert_eq!(fetched.schedule_item_ids, vec![own_item]);
}

#[tokio::test]
async fn link_schedule_items_add_unions_and_dedups() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let item_a = seed_schedule_item(&db, vid, "Oil change").await;
    let item_b = seed_schedule_item(&db, vid, "Tire rotation").await;
    let created = create(&db, vid, minimal_record(Some(vec![item_a])))
        .await
        .unwrap();

    // Add mode unions with the existing link; re-sending item_a is a no-op.
    let linked = link_schedule_items(
        &db,
        vid,
        created.record.id,
        &[item_a, item_b],
        LinkMode::Add,
    )
    .await
    .unwrap();
    let mut ids = linked.schedule_item_ids.clone();
    ids.sort_unstable();
    assert_eq!(ids, vec![item_a, item_b]);

    // No duplicate link rows were written.
    let links = service_schedule_link::Entity::find()
        .filter(service_schedule_link::Column::ServiceRecordId.eq(created.record.id))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(links.len(), 2);
}

#[tokio::test]
async fn link_schedule_items_replace_overwrites() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let item_a = seed_schedule_item(&db, vid, "Oil change").await;
    let item_b = seed_schedule_item(&db, vid, "Tire rotation").await;
    let created = create(&db, vid, minimal_record(Some(vec![item_a])))
        .await
        .unwrap();

    let linked = link_schedule_items(&db, vid, created.record.id, &[item_b], LinkMode::Replace)
        .await
        .unwrap();
    assert_eq!(linked.schedule_item_ids, vec![item_b]);
}

#[tokio::test]
async fn link_schedule_items_wrong_parent_is_byte_identical_not_found() {
    let db = test_db().await;
    let owner = seed_vehicle(&db).await;
    let other = seed_vehicle(&db).await;
    let own_item = seed_schedule_item(&db, owner, "Mine").await;
    let foreign_item = seed_schedule_item(&db, other, "Foreign").await;
    let mine = create(&db, owner, minimal_record(None)).await.unwrap();
    let theirs = create(&db, other, minimal_record(None)).await.unwrap();

    // Another vehicle's service record must read exactly like a
    // nonexistent one.
    let wrong_parent =
        link_schedule_items(&db, owner, theirs.record.id, &[own_item], LinkMode::Add)
            .await
            .unwrap_err();
    assert_eq!(
        wrong_parent.to_string(),
        format!("Service record {} not found", theirs.record.id)
    );

    // Another vehicle's schedule item likewise; nothing was linked.
    let wrong_item =
        link_schedule_items(&db, owner, mine.record.id, &[foreign_item], LinkMode::Add)
            .await
            .unwrap_err();
    assert_eq!(
        wrong_item.to_string(),
        format!("Schedule item {foreign_item} not found")
    );
    let fetched = get(&db, owner, mine.record.id).await.unwrap();
    crate::test_support::assert_empty(&fetched.schedule_item_ids);
}

#[tokio::test]
async fn create_allows_inherited_template_and_platform_schedule_items() {
    use crate::entities::{maintenance_schedule_item, model_template, platform};
    let db = test_db().await;

    let platform_id = platform::ActiveModel {
        name: Set("MQB".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;
    let template_id = model_template::ActiveModel {
        platform_id: Set(Some(platform_id)),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;
    let vid = VehicleFixture::new()
        .model_template_id(template_id)
        .insert_id(&db)
        .await;

    let platform_item = maintenance_schedule_item::ActiveModel {
        platform_id: Set(Some(platform_id)),
        name: Set("Platform oil change".into()),
        enabled: Set(true),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;
    let template_item = maintenance_schedule_item::ActiveModel {
        model_template_id: Set(Some(template_id)),
        name: Set("Template inspection".into()),
        enabled: Set(true),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;

    // Items inherited via the vehicle's template/platform chain are legit link targets.
    let created = create(
        &db,
        vid,
        minimal_record(Some(vec![platform_item, template_item])),
    )
    .await
    .unwrap();
    assert_eq!(
        created.schedule_item_ids,
        vec![platform_item, template_item]
    );
}

// --- invoice detail: who did the work, date range, line kinds, parts ---

fn line(description: &str, work_kind: WorkKind) -> NewLineItem {
    NewLineItem {
        paid_by: None,
        description: description.into(),
        work_kind,
        quantity: None,
        unit_cost_cents: None,
        cost_cents: None,
    }
}

#[tokio::test]
async fn a_line_stores_its_own_payer() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let got = create(
        &db,
        vid,
        NewServiceRecord {
            service_date: date("2026-11-03"),
            line_items: Some(vec![
                line("Oil change", WorkKind::Maintenance),
                NewLineItem {
                    paid_by: Some(Payer::Manufacturer),
                    ..line("Water pump under warranty", WorkKind::Repair)
                },
            ]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let payers: Vec<Option<Payer>> = got.line_items.iter().map(|l| l.paid_by).collect();
    assert_eq!(payers, vec![None, Some(Payer::Manufacturer)]);
    assert_eq!(effective_payer(None, Payer::Insurance), Payer::Insurance);
    assert_eq!(
        effective_payer(Some(Payer::Owner), Payer::Insurance),
        Payer::Owner
    );
}

fn inline_part(name: &str, supplied_by: Supplier) -> NewPart {
    NewPart {
        name: name.into(),
        supplied_by: Some(supplied_by),
        ..Default::default()
    }
}

fn kinds(got: &ServiceRecordWithLinks) -> Vec<WorkKind> {
    got.line_items.iter().map(|l| l.work_kind).collect()
}

#[tokio::test]
async fn create_stores_who_did_it_the_date_range_and_line_kinds() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let got = create(
        &db,
        vid,
        NewServiceRecord {
            service_date: date("2025-03-24"),
            end_date: Some(date("2025-03-25")),
            performed_by: Some(Performer::Shop),
            line_items: Some(vec![
                line("Replace suction jet pump", WorkKind::Repair),
                line("Intake upgrade", WorkKind::Mod),
            ]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(got.record.performed_by, Performer::Shop);
    assert_eq!(got.record.end_date, Some(date("2025-03-25")));
    assert_eq!(kinds(&got), vec![WorkKind::Repair, WorkKind::Mod]);
}

#[tokio::test]
async fn create_rejects_an_end_date_before_the_start() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let err = create(
        &db,
        vid,
        NewServiceRecord {
            service_date: date("2025-03-25"),
            end_date: Some(date("2025-03-24")),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "end_date"));
    assert_eq!(row_count::<service_record::Entity>(&db).await, 0);
}

async fn add_shop(db: &DatabaseConnection, name: &str, address: Option<&str>) -> i32 {
    crate::services::shop::create(
        db,
        crate::inputs::shop::NewShop {
            name: name.into(),
            address: address.map(Into::into),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id
}

#[tokio::test]
async fn a_self_performed_service_names_no_shop() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let shop_id = add_shop(&db, "Example VW", None).await;
    // A name not in the directory gets the same answer, since a
    // self-performed service names no shop at all.
    for (choice, field) in [
        (ShopChoice::Name("Example VW".into()), "shop_name"),
        (ShopChoice::Name("My Garage".into()), "shop_name"),
        (ShopChoice::Id(shop_id), "shop_id"),
    ] {
        let err = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: date("2025-03-24"),
                performed_by: Some(Performer::Owner),
                shop: Some(choice),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { .. }), "{err}");
        assert!(err.to_string().starts_with(field), "{err}");
    }

    let at_shop = create(
        &db,
        vid,
        NewServiceRecord {
            service_date: date("2025-03-24"),
            shop: Some(ShopChoice::Id(shop_id)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let self_done = UpdateServiceRecord {
        performed_by: Some(Performer::Owner),
        ..Default::default()
    };
    let err = update(&db, vid, at_shop.record.id, self_done)
        .await
        .unwrap_err();
    assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "shop_id"));
    let self_done_elsewhere = UpdateServiceRecord {
        performed_by: Some(Performer::Owner),
        shop: Some(None),
        ..Default::default()
    };
    let got = update(&db, vid, at_shop.record.id, self_done_elsewhere)
        .await
        .unwrap();
    assert_eq!((got.record.shop_id, got.shop), (None, None));
}

#[tokio::test]
async fn a_blank_shop_name_names_no_shop() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let got = create(
        &db,
        vid,
        NewServiceRecord {
            service_date: date("2025-03-24"),
            performed_by: Some(Performer::Owner),
            shop: Some(ShopChoice::Name("  ".into())),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(got.record.shop_id, None);
    let blank = UpdateServiceRecord {
        shop: Some(Some(ShopChoice::Name(String::new()))),
        ..Default::default()
    };
    let updated = update(&db, vid, got.record.id, blank).await.unwrap();
    assert_eq!(updated.record.shop_id, None);
}

#[tokio::test]
async fn a_shop_name_must_match_one_shop_in_the_directory() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let kw = add_shop(&db, "KW Automotive", None).await;
    add_shop(&db, "America's Tire", Some("433 E Shaw Ave")).await;
    add_shop(&db, "America's Tire", Some("833 E Shaw Ave")).await;
    let at = |name: &str| NewServiceRecord {
        service_date: date("2025-03-24"),
        shop: Some(ShopChoice::Name(name.into())),
        ..Default::default()
    };

    let got = create(&db, vid, at(" kw automotive ")).await.unwrap();
    assert_eq!(got.record.shop_id, Some(kw));
    assert_eq!(
        got.shop.as_ref().map(|s| s.name.as_str()),
        Some("KW Automotive")
    );
    assert_eq!(got.record.shop_name, None);
    let listed = list_between(&db, vid, None, None).await.unwrap();
    assert_eq!(
        listed[0].shop.as_ref().map(|s| s.name.as_str()),
        Some("KW Automotive")
    );

    let err = create(&db, vid, at("Clovis VW")).await.unwrap_err();
    assert!(
        matches!(err, DomainError::BadRequest(ref m) if m.starts_with("No shop")),
        "{err}"
    );
    let err = create(&db, vid, at("america's tire")).await.unwrap_err();
    assert!(
        matches!(err, DomainError::BadRequest(ref m) if m.contains("433 E Shaw Ave") && m.contains("833 E Shaw Ave")),
        "{err}"
    );
    assert_eq!(row_count::<service_record::Entity>(&db).await, 1);
}

#[tokio::test]
async fn a_service_serializes_its_shop_as_shop_id_and_shop_name() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let kw = add_shop(&db, "KW Automotive", None).await;
    let on = |day: &str, shop: Option<ShopChoice>| NewServiceRecord {
        service_date: date(day),
        shop,
        ..Default::default()
    };
    let named = create(&db, vid, on("2025-03-24", Some(ShopChoice::Id(kw))))
        .await
        .unwrap();
    let bare = create(&db, vid, on("2025-04-24", None)).await.unwrap();

    crate::test_support::assert_shop_keys(&named, "shop_name", Some((kw, "KW Automotive")));
    crate::test_support::assert_shop_keys(&bare, "shop_name", None);
    let read = get(&db, vid, named.record.id).await.unwrap();
    crate::test_support::assert_shop_keys(&read, "shop_name", Some((kw, "KW Automotive")));
    let listed = list_between(&db, vid, None, None).await.unwrap();
    for service in &listed {
        let shop = (service.record.id == named.record.id).then_some((kw, "KW Automotive"));
        crate::test_support::assert_shop_keys(service, "shop_name", shop);
    }
    assert_eq!(listed.len(), 2);
}

#[tokio::test]
async fn inline_parts_are_installed_by_the_service_and_keep_their_supplier() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let got = create(
        &db,
        vid,
        NewServiceRecord {
            service_date: date("2025-03-24"),
            mileage: Some(52_747),
            parts: Some(vec![
                NewPart {
                    seller: Some("FCP Euro".into()),
                    ..inline_part("Front brake pads", Supplier::Owner)
                },
                inline_part("Brake fluid", Supplier::Shop),
            ]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(got.parts.len(), 2);
    assert_eq!(got.part_ids.len(), 2);
    assert_eq!(got.parts[0].part.supplied_by, Supplier::Owner);
    assert_eq!(got.parts[0].part.installed_service_id, Some(got.record.id));
    assert_eq!(got.parts[0].part.installed_odometer, Some(52_747));
    assert_eq!(got.parts[0].part.installed_date, Some(date("2025-03-24")));
    assert_eq!(got.parts[0].part.status, PartStatus::Installed);
    assert_eq!(got.parts[1].part.supplied_by, Supplier::Shop);
}

#[tokio::test]
async fn installing_a_returned_part_through_a_service_is_refused() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let mut returned = part::ActiveModel {
        vehicle_id: Set(vid),
        name: Set("Rotors".into()),
        supplied_by: Set(Supplier::Owner),
        ..Default::default()
    };
    part_svc::PartState::Returned {
        on: date("2026-11-06"),
        reason: "Wrong size".into(),
        need_id: None,
    }
    .write(&mut returned);
    let returned = returned.insert(&db).await.unwrap();
    let err = create(
        &db,
        vid,
        NewServiceRecord {
            service_date: date("2026-11-10"),
            part_ids: Some(vec![returned.id]),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
    assert_eq!(row_count::<service_record::Entity>(&db).await, 0);
    let after = part::Entity::find_by_id(returned.id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.status, PartStatus::Returned);
}

#[tokio::test]
async fn refiling_an_invoice_number_returns_the_first_record_untouched() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let first = create(
        &db,
        vid,
        NewServiceRecord {
            service_date: date("2025-03-24"),
            invoice_ref: Some("07447".into()),
            line_items: Some(vec![line("Oil change", WorkKind::Maintenance)]),
            parts: Some(vec![inline_part("Oil filter", Supplier::Shop)]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(!first.already_recorded);
    assert!(first.message.is_none());

    let second = create(
        &db,
        vid,
        NewServiceRecord {
            service_date: date("2025-03-24"),
            invoice_ref: Some("07447".into()),
            line_items: Some(vec![
                line("Other", WorkKind::Repair),
                line("Another", WorkKind::Mod),
            ]),
            parts: Some(vec![
                inline_part("A", Supplier::Owner),
                inline_part("B", Supplier::Owner),
            ]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(second.already_recorded);
    assert!(
        second
            .message
            .as_deref()
            .is_some_and(|m| m.contains(&format!("service {}", first.record.id))),
        "got: {:?}",
        second.message
    );
    assert_eq!(second.record.id, first.record.id);
    assert_eq!(row_count::<service_record_line_item::Entity>(&db).await, 1);
    assert_eq!(row_count::<part::Entity>(&db).await, 1);
}

#[tokio::test]
async fn a_refused_inline_part_rolls_back_the_whole_service() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let result = create(
        &db,
        vid,
        NewServiceRecord {
            service_date: date("2025-03-24"),
            mileage: Some(50_000),
            line_items: Some(vec![line("Oil change", WorkKind::Maintenance)]),
            parts: Some(vec![NewPart {
                need_id: Some(99_999),
                ..inline_part("Filter", Supplier::Owner)
            }]),
            ..Default::default()
        },
    )
    .await;
    assert!(matches!(result, Err(DomainError::NotFound(_))));
    assert_eq!(row_count::<service_record::Entity>(&db).await, 0);
    assert_eq!(row_count::<service_record_line_item::Entity>(&db).await, 0);
    assert_eq!(row_count::<part::Entity>(&db).await, 0);
    assert_eq!(row_count::<mileage_log::Entity>(&db).await, 0);
}

#[tokio::test]
async fn a_self_performed_service_records_an_ecu_flash() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let got = create(
        &db,
        vid,
        NewServiceRecord {
            service_date: date("2025-03-24"),
            performed_by: Some(Performer::Owner),
            line_items: Some(vec![line("IE Stage 1 tune flash", WorkKind::Mod)]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(got.record.performed_by, Performer::Owner);
    assert!(got.record.shop_name.is_none());
}

#[tokio::test]
async fn update_changes_performer_and_end_date_and_validates_them() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let made = create(&db, vid, minimal_record(None)).await.unwrap();

    let updated = update(
        &db,
        vid,
        made.record.id,
        UpdateServiceRecord {
            performed_by: Some(Performer::Owner),
            end_date: Some(Some(date("2024-03-02"))),
            line_items: Some(vec![line("Flash", WorkKind::Mod)]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.record.performed_by, Performer::Owner);
    assert_eq!(updated.record.end_date, Some(date("2024-03-02")));
    assert_eq!(kinds(&updated), vec![WorkKind::Mod]);

    let err = update(
        &db,
        vid,
        made.record.id,
        UpdateServiceRecord {
            end_date: Some(Some(date("2024-02-01"))),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "end_date"));

    let cleared = update(
        &db,
        vid,
        made.record.id,
        UpdateServiceRecord {
            end_date: Some(None),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(cleared.record.end_date.is_none());
}
// --- incident links ---------------------------------------------------

async fn service_dated(db: &DatabaseConnection, vehicle_id: i32, date: &str) -> i32 {
    create(
        db,
        vehicle_id,
        NewServiceRecord {
            service_date: crate::test_support::date(date),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .record
    .id
}

async fn observation(db: &DatabaseConnection, vehicle_id: i32, noticed_during: Option<i32>) -> i32 {
    incident::create(
        db,
        vehicle_id,
        NewIncident {
            category: IncidentCategory::ShopObservation,
            title: "N80 purge valve failing".into(),
            noticed_during_service_id: noticed_during,
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .incident
    .id
}

#[tokio::test]
async fn a_shop_observation_links_the_service_that_noticed_it_and_the_one_that_fixed_it() {
    let db = test_db().await;
    let v = seed_vehicle(&db).await;
    let svc_2023 = service_dated(&db, v, "2023-05-03").await;
    let incident_id = observation(&db, v, Some(svc_2023)).await;
    let svc_2024 = create(
        &db,
        v,
        NewServiceRecord {
            service_date: date("2024-06-12"),
            resolved_incident_ids: Some(vec![incident_id]),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .record
    .id;

    let s23 = get(&db, v, svc_2023).await.unwrap();
    assert_eq!(s23.noticed_incidents[0].title, "N80 purge valve failing");
    assert!(s23.noticed_incidents[0].resolved);
    assert_eq!(
        s23.noticed_incidents[0]
            .other_service
            .as_ref()
            .unwrap()
            .service_date,
        date("2024-06-12")
    );
    let s24 = get(&db, v, svc_2024).await.unwrap();
    assert_eq!(
        s24.resolved_incidents[0]
            .other_service
            .as_ref()
            .unwrap()
            .service_date,
        date("2023-05-03")
    );
    let inc = incident::get(&db, v, incident_id).await.unwrap();
    assert_eq!(inc.noticed_during.unwrap().id, svc_2023);
    assert_eq!(inc.resolved_by[0].id, svc_2024);

    // The list path carries the same links.
    let listed = list(&db, v).await.unwrap();
    let listed_23 = listed.iter().find(|s| s.record.id == svc_2023).unwrap();
    assert_eq!(listed_23.noticed_incidents.len(), 1);
    let listed_24 = listed.iter().find(|s| s.record.id == svc_2024).unwrap();
    assert_eq!(listed_24.resolved_incidents.len(), 1);
}

#[tokio::test]
async fn an_unresolved_observation_shows_as_open_on_the_service_that_noticed_it() {
    let db = test_db().await;
    let v = seed_vehicle(&db).await;
    let svc_2023 = service_dated(&db, v, "2023-05-03").await;
    observation(&db, v, Some(svc_2023)).await;

    let s23 = get(&db, v, svc_2023).await.unwrap();
    assert!(!s23.noticed_incidents[0].resolved);
    assert!(s23.noticed_incidents[0].other_service.is_none());
}

#[tokio::test]
async fn recording_a_service_can_note_what_it_noticed() {
    let db = test_db().await;
    let v = seed_vehicle(&db).await;
    let incident_id = observation(&db, v, None).await;
    let svc = create(
        &db,
        v,
        NewServiceRecord {
            service_date: date("2023-05-03"),
            noticed_incident_ids: Some(vec![incident_id]),
            recall_finding_ids: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(svc.noticed_incidents[0].id, incident_id);
    let inc = incident::get(&db, v, incident_id).await.unwrap();
    assert_eq!(inc.noticed_during.unwrap().id, svc.record.id);
    assert!(!inc.incident.resolved);

    // An incident is noticed during at most one service.
    let err = create(
        &db,
        v,
        NewServiceRecord {
            service_date: date("2023-06-01"),
            noticed_incident_ids: Some(vec![incident_id]),
            recall_finding_ids: None,
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DomainError::BadRequest(_)), "got {err:?}");
    assert_eq!(row_count::<service_record::Entity>(&db).await, 1);
}

#[tokio::test]
async fn completing_an_incident_sourced_work_item_resolves_the_incident() {
    let db = test_db().await;
    let v = seed_vehicle(&db).await;
    let svc_2023 = service_dated(&db, v, "2023-05-03").await;
    let incident_id = observation(&db, v, Some(svc_2023)).await;
    let planned = work_item::create(
        &db,
        v,
        crate::inputs::work_item::NewWorkItem {
            title: "Replace purge valve".into(),
            incident_id: Some(incident_id),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Naming the incident as well as the work item must not double-link.
    let svc_2024 = create(
        &db,
        v,
        NewServiceRecord {
            service_date: date("2024-06-12"),
            work_item_ids: Some(vec![planned.id]),
            resolved_incident_ids: Some(vec![incident_id]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(svc_2024.resolved_incidents.len(), 1);
    let inc = incident::get(&db, v, incident_id).await.unwrap();
    assert!(inc.incident.resolved);
    assert_eq!(inc.resolved_by.len(), 1);
    assert_eq!(inc.resolved_by[0].id, svc_2024.record.id);

    // The service names the to-do it completed, when created and when read
    // back alone or in a list.
    let listed = list(&db, v).await.unwrap();
    let read = get(&db, v, svc_2024.record.id).await.unwrap();
    for got in [&svc_2024, &read, &listed[0]] {
        assert_eq!(got.completed_work_items.len(), 1);
        assert_eq!(got.completed_work_items[0].id, planned.id);
        assert_eq!(got.completed_work_items[0].title, "Replace purge valve");
    }
    assert!(listed[1].completed_work_items.is_empty());
}

#[tokio::test]
async fn incident_and_service_ids_from_another_vehicle_are_not_found() {
    let db = test_db().await;
    let v1 = seed_vehicle(&db).await;
    let v2 = seed_vehicle(&db).await;
    let foreign_incident = observation(&db, v2, None).await;
    let foreign_service = service_dated(&db, v2, "2023-05-03").await;
    let services_before = row_count::<service_record::Entity>(&db).await;

    // A foreign noticed_during_service_id on create reads like a missing one.
    let wrong = incident::create(
        &db,
        v1,
        NewIncident {
            noticed_during_service_id: Some(foreign_service),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    let missing = incident::create(
        &db,
        v1,
        NewIncident {
            noticed_during_service_id: Some(9999),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(wrong, DomainError::NotFound(_)));
    assert_eq!(
        wrong.to_string().replace(&foreign_service.to_string(), "N"),
        missing.to_string().replace("9999", "N")
    );
    crate::test_support::assert_empty(
        &incident::list_filtered(&db, v1, crate::inputs::incident::IncidentFilter::default())
            .await
            .unwrap(),
    );

    // Foreign incident ids on record_service leave no footprint at all.
    let sched = seed_schedule_item(&db, v1, "Oil change").await;
    let full = |as_resolved: bool, id: i32| {
        let ids = Some(vec![id]);
        NewServiceRecord {
            service_date: date("2024-06-12"),
            mileage: Some(50_000),
            line_items: Some(vec![line("Oil", WorkKind::Maintenance)]),
            parts: Some(vec![NewPart {
                name: "Filter".into(),
                ..Default::default()
            }]),
            schedule_item_ids: Some(vec![sched]),
            resolved_incident_ids: if as_resolved { ids.clone() } else { None },
            noticed_incident_ids: if as_resolved { None } else { ids },
            recall_finding_ids: None,
            ..Default::default()
        }
    };
    for as_resolved in [true, false] {
        let err = create(&db, v1, full(as_resolved, foreign_incident))
            .await
            .unwrap_err();
        let gone = create(&db, v1, full(as_resolved, 9999)).await.unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)));
        assert_eq!(
            err.to_string().replace(&foreign_incident.to_string(), "N"),
            gone.to_string().replace("9999", "N")
        );
        assert_eq!(
            row_count::<service_record::Entity>(&db).await,
            services_before
        );
        assert_eq!(row_count::<service_record_line_item::Entity>(&db).await, 0);
        assert_eq!(row_count::<part::Entity>(&db).await, 0);
        assert_eq!(row_count::<mileage_log::Entity>(&db).await, 0);
        assert_eq!(row_count::<service_schedule_link::Entity>(&db).await, 0);
        assert_eq!(row_count::<incident_service_link::Entity>(&db).await, 0);
    }
    assert!(
        !incident::get(&db, v2, foreign_incident)
            .await
            .unwrap()
            .incident
            .resolved
    );
}

// --- deleting a service reopens what it closed --------------------------

#[tokio::test]
async fn deleting_a_service_reopens_what_it_closed() {
    use crate::{
        inputs::{service_record::NewServiceRecord, work_item::NewWorkItem},
        services::{recall, research, work_item as work_item_svc},
    };

    let db = test_db().await;
    let v = seed_vehicle(&db).await;

    let earlier = service_dated(&db, v, "2023-01-10").await;
    let incident_a = observation(&db, v, None).await;
    let incident_b = observation(&db, v, None).await;
    incident::update(
        &db,
        v,
        incident_b,
        crate::inputs::incident::UpdateIncident {
            resolved_by_service_ids: Some(vec![earlier]),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let finding_id = research::record_recalls(
        &db,
        v,
        &[crate::services::nhtsa::RecallInfo {
            campaign_number: "24V123".into(),
            manufacturer: None,
            title: "Pump".into(),
            summary: None,
            consequence: None,
            remedy: None,
            report_date: None,
            component: None,
            action_number: None,
        }],
    )
    .await
    .unwrap()
    .recalls[0]
        .finding_id;
    let work_item = work_item_svc::create(
        &db,
        v,
        NewWorkItem {
            title: "Do the thing".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let closing = create(
        &db,
        v,
        NewServiceRecord {
            service_date: date("2024-06-01"),
            work_item_ids: Some(vec![work_item.id]),
            resolved_incident_ids: Some(vec![incident_a, incident_b]),
            recall_finding_ids: Some(vec![finding_id]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let work_item_before = work_item_svc::get(&db, v, work_item.id).await.unwrap();
    assert_eq!(work_item_before.status, JobStatus::Done);
    assert_eq!(
        recall::list(&db, v).await.unwrap()[0].status,
        recall::ShownRecallStatus::Done
    );

    delete(&db, v, closing.record.id, UNDO).await.unwrap();

    let work_item_after = work_item_svc::get(&db, v, work_item.id).await.unwrap();
    assert_eq!(work_item_after.status, JobStatus::Planned);
    assert_eq!(work_item_after.completed_service_id, None);
    let reopened_incident = incident::get(&db, v, incident_a).await.unwrap();
    let kept_incident = incident::get(&db, v, incident_b).await.unwrap();
    assert!(!reopened_incident.incident.resolved);
    assert!(kept_incident.incident.resolved);
    assert_eq!(
        recall::list(&db, v).await.unwrap()[0].status,
        recall::ShownRecallStatus::Open
    );
}

async fn seed_recall(db: &DatabaseConnection, v: i32, campaign: &str) -> i32 {
    crate::test_support::seed_recall(db, v, campaign, "Pump").await
}

async fn service_with_recall(db: &DatabaseConnection, v: i32, finding_id: i32) -> i32 {
    create(
        db,
        v,
        NewServiceRecord {
            service_date: date("2024-06-01"),
            recall_finding_ids: Some(vec![finding_id]),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .record
    .id
}

async fn recall_status(
    db: &DatabaseConnection,
    v: i32,
) -> Vec<(recall::ShownRecallStatus, Option<i32>)> {
    crate::services::recall::list(db, v)
        .await
        .unwrap()
        .into_iter()
        .map(|r| (r.status, r.done_service.map(|s| s.id)))
        .collect()
}

#[tokio::test]
async fn deleting_a_service_with_recalls_and_no_choice_changes_nothing() {
    use crate::{inputs::work_item::NewWorkItem, services::work_item as work_item_svc};

    let db = test_db().await;
    let v = seed_vehicle(&db).await;
    let finding_id = seed_recall(&db, v, "24V123").await;
    let incident_id = observation(&db, v, None).await;
    let work_item = work_item_svc::create(
        &db,
        v,
        NewWorkItem {
            title: "Do the thing".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let svc = create(
        &db,
        v,
        NewServiceRecord {
            service_date: date("2024-06-01"),
            recall_finding_ids: Some(vec![finding_id]),
            work_item_ids: Some(vec![work_item.id]),
            resolved_incident_ids: Some(vec![incident_id]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let id = svc.record.id;

    let err = delete(&db, v, id, None).await.unwrap_err();

    let DomainError::BadRequest(message) = err else {
        panic!("expected BadRequest, got {err:?}");
    };
    assert!(message.contains(&finding_id.to_string()), "{message}");
    assert!(message.contains("24V123"), "{message}");
    assert!(message.contains("Pump"), "{message}");
    assert!(
        message.contains(&format!("job {} ", work_item.id))
            && message.contains(&format!("incident {incident_id} ")),
        "{message}"
    );
    assert!(
        message.contains("did not happen or was done another way"),
        "{message}"
    );
    assert!(get(&db, v, id).await.is_ok());
    assert_eq!(
        recall_status(&db, v).await,
        vec![(recall::ShownRecallStatus::Done, Some(id))]
    );
    assert_eq!(
        work_item_svc::get(&db, v, work_item.id)
            .await
            .unwrap()
            .status,
        JobStatus::Done
    );
    assert!(
        incident::get(&db, v, incident_id)
            .await
            .unwrap()
            .incident
            .resolved
    );
}

#[tokio::test]
async fn reopen_makes_a_performed_recall_open_even_if_the_owner_marked_it_done() {
    let db = test_db().await;
    let v = seed_vehicle(&db).await;
    let finding_id = seed_recall(&db, v, "24V999").await;
    let svc = service_with_recall(&db, v, finding_id).await;
    recall::set_status(&db, v, finding_id, recall::RecallStatus::Done)
        .await
        .unwrap();

    let out = delete(&db, v, svc, UNDO).await.unwrap();

    assert_eq!(out.reopened_recall_ids, vec![finding_id]);
    crate::test_support::assert_empty(&out.kept_done_recall_ids);
    assert_eq!(
        recall_status(&db, v).await,
        vec![(recall::ShownRecallStatus::Open, None)]
    );
}

#[tokio::test]
async fn keep_done_leaves_a_performed_recall_done_with_no_service() {
    let db = test_db().await;
    let v = seed_vehicle(&db).await;
    let finding_id = seed_recall(&db, v, "24V999").await;
    let svc = service_with_recall(&db, v, finding_id).await;

    let out = delete(&db, v, svc, Some(CompletedWork::DoneAnotherWay))
        .await
        .unwrap();

    assert_eq!(out.kept_done_recall_ids, vec![finding_id]);
    crate::test_support::assert_empty(&out.reopened_recall_ids);
    assert_eq!(
        recall_status(&db, v).await,
        vec![(recall::ShownRecallStatus::Done, None)]
    );
}

#[tokio::test]
async fn a_service_without_recalls_deletes_with_no_choice() {
    let db = test_db().await;
    let v = seed_vehicle(&db).await;
    let svc = service_dated(&db, v, "2024-06-01").await;

    let out = delete(&db, v, svc, None).await.unwrap();

    assert!(out.reopened_recall_ids.is_empty() && out.kept_done_recall_ids.is_empty());
    assert!(get(&db, v, svc).await.is_err());
}

#[tokio::test]
async fn reopen_leaves_a_recall_done_through_another_service() {
    let db = test_db().await;
    let v = seed_vehicle(&db).await;
    let finding_id = seed_recall(&db, v, "24V999").await;
    let first = service_with_recall(&db, v, finding_id).await;
    let second = service_with_recall(&db, v, finding_id).await;

    let out = delete(&db, v, first, UNDO).await.unwrap();

    crate::test_support::assert_empty(&out.reopened_recall_ids);
    assert_eq!(out.still_done_elsewhere_recall_ids, vec![finding_id]);
    assert_eq!(
        recall_status(&db, v).await,
        vec![(recall::ShownRecallStatus::Done, Some(second))]
    );
}

#[tokio::test]
async fn deleting_a_service_keeps_its_inline_parts_as_purchased() {
    let db = test_db().await;
    let v = seed_vehicle(&db).await;
    let svc = create(
        &db,
        v,
        NewServiceRecord {
            service_date: date("2024-06-01"),
            mileage: Some(40_000),
            parts: Some(vec![NewPart {
                name: "Brake pads".into(),
                ..Default::default()
            }]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let part_id = svc.parts[0].part.id;

    delete(&db, v, svc.record.id, UNDO).await.unwrap();

    let kept = part_svc::get(&db, v, part_id).await.unwrap();
    assert_eq!(kept.status, PartStatus::Purchased);
    assert_eq!(kept.installed_service_id, None);
}

#[tokio::test]
async fn correcting_the_date_or_mileage_moves_the_install_of_its_parts() {
    let db = test_db().await;
    let v = seed_vehicle(&db).await;
    let svc = create(
        &db,
        v,
        NewServiceRecord {
            service_date: date("2024-03-01"),
            mileage: Some(50_000),
            parts: Some(vec![inline_part("Pads", Supplier::Shop)]),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let corrected = update(
        &db,
        v,
        svc.record.id,
        UpdateServiceRecord {
            service_date: Some(date("2024-03-10")),
            mileage: Some(Some(50_500)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        corrected.parts[0].part.installed_date,
        Some(date("2024-03-10"))
    );
    assert_eq!(corrected.parts[0].part.installed_odometer, Some(50_500));

    // Clearing the service's mileage leaves the part its own reading.
    let cleared = update(
        &db,
        v,
        svc.record.id,
        UpdateServiceRecord {
            mileage: Some(None),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(cleared.parts[0].part.installed_odometer, Some(50_500));
    assert_eq!(
        cleared.parts[0].part.installed_date,
        Some(date("2024-03-10"))
    );

    // So does listing the part again on the service with no mileage.
    let part_id = cleared.parts[0].part.id;
    let relisted = update(
        &db,
        v,
        svc.record.id,
        UpdateServiceRecord {
            part_ids: Some(vec![part_id]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(relisted.parts[0].part.installed_odometer, Some(50_500));
}

#[tokio::test]
async fn listing_a_part_a_later_service_replaced_keeps_its_replacement() {
    use crate::inputs::part::{Fitting, InstallPatch, ReplacePatch, UpdatePart};
    let db = test_db().await;
    let v = seed_vehicle(&db).await;
    let svc = create(
        &db,
        v,
        NewServiceRecord {
            service_date: date("2024-03-01"),
            parts: Some(vec![inline_part("Pads", Supplier::Shop)]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let part_id = svc.parts[0].part.id;
    let replaced = ReplacePatch {
        date: Some(Some(date("2025-06-01"))),
        odometer: Some(Some(60_000)),
    };
    let off = UpdatePart {
        fitting: Fitting::Replaced(InstallPatch::default(), replaced),
        ..Default::default()
    };
    part_svc::update(&db, v, part_id, off).await.unwrap();

    let corrected = update(
        &db,
        v,
        svc.record.id,
        UpdateServiceRecord {
            part_ids: Some(vec![part_id]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let part = &corrected.parts[0].part;
    assert_eq!(part.status, PartStatus::Replaced);
    assert_eq!(part.replaced_date, Some(date("2025-06-01")));
    assert_eq!(part.replaced_odometer, Some(60_000));
}

#[tokio::test]
async fn list_between_keeps_only_services_inside_the_bounds() {
    let db = test_db().await;
    let v = seed_vehicle(&db).await;
    for date in ["2023-01-05", "2024-03-01", "2025-07-09"] {
        service_dated(&db, v, date).await;
    }
    let dates = |rows: Vec<ServiceRecordWithLinks>| {
        rows.into_iter()
            .map(|r| r.record.service_date.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        dates(
            list_between(&db, v, Some(date("2024-01-01")), None)
                .await
                .unwrap()
        ),
        vec!["2025-07-09", "2024-03-01"]
    );
    assert_eq!(
        dates(
            list_between(&db, v, None, Some(date("2024-03-01")))
                .await
                .unwrap()
        ),
        vec!["2024-03-01", "2023-01-05"]
    );
    assert_eq!(list_between(&db, v, None, None).await.unwrap().len(), 3);
}

// ─── invoices against appointments ───

// Invoices filed at once from several pools are all recorded: each opens
// with the write lock before it reads the schedule it links.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_services_on_one_vehicle_are_all_recorded() {
    let (_dir, pools) = crate::test_support::race_pools().await;
    let first = &pools[0];
    let v = seed_vehicle(first).await;

    let results = crate::test_support::race(&pools, |i, db| async move {
        let input = NewServiceRecord {
            service_date: date("2024-06-01"),
            invoice_ref: Some(format!("INV-{i}")),
            ..Default::default()
        };
        create(&db, v, input).await.map(|_| ())
    })
    .await;

    assert!(results.iter().all(Result::is_ok), "{results:?}");
    assert_eq!(
        row_count::<service_record::Entity>(first).await,
        results.len() as u64
    );
}

// One invoice filed at once from several pools is stored once, and every
// other call gets that record back as already recorded.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_invoice_filed_at_once_is_recorded_once() {
    let (_dir, pools) = crate::test_support::race_pools().await;
    let first = &pools[0];
    let v = seed_vehicle(first).await;

    let results = crate::test_support::race(&pools, |_, db| async move {
        let input = NewServiceRecord {
            service_date: date("2024-06-01"),
            invoice_ref: Some("INV-1".into()),
            ..Default::default()
        };
        create(&db, v, input).await.map(|r| r.already_recorded)
    })
    .await;

    let repeats: Vec<bool> = results.into_iter().map(Result::unwrap).collect();
    assert_eq!(repeats.iter().filter(|r| !**r).count(), 1, "{repeats:?}");
    assert_eq!(row_count::<service_record::Entity>(first).await, 1);
}

// Deletes racing on one file from several pools delete the service once,
// and the rest find it gone: each takes the write lock before it reads.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_deletes_of_one_service_delete_it_once() {
    let (_dir, pools) = crate::test_support::race_pools().await;
    let first = &pools[0];
    let v = seed_vehicle(first).await;
    let part_id = seed_part(first, v, "Pads").await;
    let id = create(
        first,
        v,
        NewServiceRecord {
            service_date: date("2024-06-01"),
            part_ids: Some(vec![part_id]),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .record
    .id;

    let results = crate::test_support::race(&pools, |_, db| async move {
        delete(&db, v, id, UNDO).await.map(|_| ())
    })
    .await;

    assert_eq!(
        results.iter().filter(|r| r.is_ok()).count(),
        1,
        "{results:?}"
    );
    for err in results.iter().filter_map(|r| r.as_ref().err()) {
        assert!(matches!(err, DomainError::NotFound(_)), "{err:?}");
    }
}

mod appointments {
    use super::*;
    use crate::{
        entities::{job_need, work_item},
        error::DomainError,
        inputs::{appointment::NewAppointment, need::NewNeed, work_item::NewWorkItem},
        services::{appointment, need, part as part_svc, work_item as job},
    };

    struct Fixture {
        db: DatabaseConnection,
        vehicle: i32,
        appointment: i32,
    }

    async fn fixture() -> Fixture {
        let db = test_db().await;
        let vehicle = seed_vehicle(&db).await;
        let appointment = appointment::create(
            &db,
            vehicle,
            NewAppointment {
                purpose: "Brakes".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        Fixture {
            db,
            vehicle,
            appointment,
        }
    }

    async fn add_job(f: &Fixture, title: &str) -> i32 {
        job::create(
            &f.db,
            f.vehicle,
            NewWorkItem {
                title: title.into(),
                appointment_id: Some(f.appointment),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    async fn add_need(f: &Fixture, job_id: i32, name: &str, supplied_by: Option<Supplier>) -> i32 {
        need::add(
            &f.db,
            f.vehicle,
            job_id,
            NewNeed {
                name: name.into(),
                supplied_by,
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    // A recorded part linked to the need, as the owner's own purchase.
    async fn link_new_part(f: &Fixture, need_id: i32, received: bool) -> i32 {
        let part_id = seed_part(&f.db, f.vehicle, "Pads").await;
        let received_on = received.then(|| date("2026-11-01"));
        need::link_part(&f.db, f.vehicle, need_id, part_id, received_on)
            .await
            .unwrap();
        part_id
    }

    async fn stage_of(f: &Fixture, need_id: i32) -> need::Stage {
        let n = job_need::Entity::find_by_id(need_id)
            .one(&f.db)
            .await
            .unwrap()
            .unwrap();
        let p = match n.part_id {
            Some(id) => part::Entity::find_by_id(id).one(&f.db).await.unwrap(),
            None => None,
        };
        need::stage(&n, p.as_ref())
    }

    async fn job_status(f: &Fixture, job_id: i32) -> JobStatus {
        work_item::Entity::find_by_id(job_id)
            .one(&f.db)
            .await
            .unwrap()
            .unwrap()
            .status
    }

    async fn appointment_status(f: &Fixture) -> appointment::AppointmentStatus {
        appointment::summary(&f.db, f.vehicle, f.appointment, crate::services::today())
            .await
            .unwrap()
            .status
    }

    fn invoice(f: &Fixture) -> NewServiceRecord {
        NewServiceRecord {
            service_date: date("2026-11-10"),
            appointment_id: Some(f.appointment),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn naming_an_appointment_installs_and_completes_nothing_by_itself() {
        let f = fixture().await;
        let job_id = add_job(&f, "Rear brakes").await;
        let need_id = add_need(&f, job_id, "Pads", None).await;
        let part_id = link_new_part(&f, need_id, false).await;

        let created = create(&f.db, f.vehicle, invoice(&f)).await.unwrap();

        assert_eq!(created.record.appointment_id, Some(f.appointment));
        assert_eq!(job_status(&f, job_id).await, JobStatus::Planned);
        let part = part_svc::get(&f.db, f.vehicle, part_id).await.unwrap();
        assert_eq!(part.status, PartStatus::Purchased);
        assert_eq!(stage_of(&f, need_id).await, need::Stage::Ordered);
        assert!(created.unfitted_needs.is_empty());
        assert!(created.completed_work_items.is_empty());
    }

    #[tokio::test]
    async fn named_parts_install_and_their_needs_read_installed() {
        let f = fixture().await;
        let job_id = add_job(&f, "Rear brakes").await;
        let need_id = add_need(&f, job_id, "Pads", None).await;
        let part_id = link_new_part(&f, need_id, true).await;

        let created = create(
            &f.db,
            f.vehicle,
            NewServiceRecord {
                part_ids: Some(vec![part_id]),
                work_item_ids: Some(vec![job_id]),
                ..invoice(&f)
            },
        )
        .await
        .unwrap();

        assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);
        assert_eq!(job_status(&f, job_id).await, JobStatus::Done);
        assert!(created.unfitted_needs.is_empty());
    }

    #[tokio::test]
    async fn inline_shop_part_fills_its_need() {
        let f = fixture().await;
        let job_id = add_job(&f, "Rear brakes").await;
        let need_id = add_need(&f, job_id, "Pads", Some(Supplier::Shop)).await;

        let created = create(
            &f.db,
            f.vehicle,
            NewServiceRecord {
                work_item_ids: Some(vec![job_id]),
                parts: Some(vec![NewPart {
                    name: "Shop pads".into(),
                    supplied_by: Some(Supplier::Shop),
                    need_id: Some(need_id),
                    ..Default::default()
                }]),
                ..invoice(&f)
            },
        )
        .await
        .unwrap();

        let new_part_id = created.part_ids[0];
        let n = job_need::Entity::find_by_id(need_id)
            .one(&f.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(n.part_id, Some(new_part_id));
        assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);
        assert!(created.unfitted_needs.is_empty());
    }

    #[tokio::test]
    async fn unfitted_needs_lists_linked_parts_not_named() {
        let f = fixture().await;
        let job_id = add_job(&f, "Rear brakes").await;
        let fitted = add_need(&f, job_id, "Pads", None).await;
        let fitted_part = link_new_part(&f, fitted, true).await;
        let left_over = add_need(&f, job_id, "Sensor", None).await;
        let left_over_part = link_new_part(&f, left_over, false).await;
        // A need with no part and a need on a job this service does not
        // complete are not listed.
        add_need(&f, job_id, "Grease", None).await;
        let other_job = add_job(&f, "Wipers").await;
        let other_need = add_need(&f, other_job, "Blades", None).await;
        link_new_part(&f, other_need, false).await;

        let created = create(
            &f.db,
            f.vehicle,
            NewServiceRecord {
                part_ids: Some(vec![fitted_part]),
                work_item_ids: Some(vec![job_id]),
                ..invoice(&f)
            },
        )
        .await
        .unwrap();

        assert_eq!(created.unfitted_needs.len(), 1);
        let listed = &created.unfitted_needs[0];
        assert_eq!(listed.need_id, left_over);
        assert_eq!(listed.name, "Sensor");
        assert_eq!(listed.job_title, "Rear brakes");
        assert_eq!(listed.part_id, left_over_part);
        let json = serde_json::to_value(&created).unwrap();
        assert_eq!(json["unfitted_needs"].as_array().unwrap().len(), 1);

        // Plain reads leave the list out; an update recomputes it.
        let read = get(&f.db, f.vehicle, created.record.id).await.unwrap();
        assert!(read.unfitted_needs.is_empty());
        assert!(
            serde_json::to_value(&read)
                .unwrap()
                .get("unfitted_needs")
                .is_none()
        );
        let updated = update(
            &f.db,
            f.vehicle,
            created.record.id,
            UpdateServiceRecord {
                part_ids: Some(vec![fitted_part, left_over_part]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(updated.unfitted_needs.is_empty());
        let again = update(
            &f.db,
            f.vehicle,
            created.record.id,
            UpdateServiceRecord {
                part_ids: Some(vec![fitted_part]),
                completed_work: Some(CompletedWork::DidNotHappen),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(again.unfitted_needs.len(), 1);
    }

    async fn set_part_status(f: &Fixture, part_id: i32, status: PartStatus) {
        part::ActiveModel {
            id: Unchanged(part_id),
            status: Set(status),
            ..Default::default()
        }
        .update(&f.db)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_replaced_part_is_not_listed_as_unfitted() {
        let f = fixture().await;
        let job_id = add_job(&f, "Rear brakes").await;
        let need_id = add_need(&f, job_id, "Pads", None).await;
        let part_id = link_new_part(&f, need_id, true).await;
        set_part_status(&f, part_id, PartStatus::Replaced).await;

        let created = create(
            &f.db,
            f.vehicle,
            NewServiceRecord {
                work_item_ids: Some(vec![job_id]),
                ..invoice(&f)
            },
        )
        .await
        .unwrap();

        assert!(created.unfitted_needs.is_empty());
    }

    // Dropping a replaced part from the service's `part_ids` keeps its
    // status, so the need stays linked and still reads installed.
    #[tokio::test]
    async fn a_replaced_part_dropped_from_its_service_still_reads_installed() {
        let f = fixture().await;
        let job_id = add_job(&f, "Rear brakes").await;
        let need_id = add_need(&f, job_id, "Pads", None).await;
        let part_id = link_new_part(&f, need_id, true).await;
        let created = create(
            &f.db,
            f.vehicle,
            NewServiceRecord {
                part_ids: Some(vec![part_id]),
                work_item_ids: Some(vec![job_id]),
                ..invoice(&f)
            },
        )
        .await
        .unwrap();
        set_part_status(&f, part_id, PartStatus::Replaced).await;

        update(
            &f.db,
            f.vehicle,
            created.record.id,
            UpdateServiceRecord {
                part_ids: Some(vec![]),
                completed_work: Some(CompletedWork::DidNotHappen),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let n = job_need::Entity::find_by_id(need_id)
            .one(&f.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(n.part_id, Some(part_id));
        assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);
    }

    #[tokio::test]
    async fn appointment_reads_done_when_its_last_job_completes() {
        let f = fixture().await;
        let first = add_job(&f, "Rear brakes").await;
        let second = add_job(&f, "Wipers").await;
        assert_eq!(
            appointment_status(&f).await,
            appointment::AppointmentStatus::Planning
        );

        create(
            &f.db,
            f.vehicle,
            NewServiceRecord {
                work_item_ids: Some(vec![first]),
                ..invoice(&f)
            },
        )
        .await
        .unwrap();
        assert_eq!(
            appointment_status(&f).await,
            appointment::AppointmentStatus::Planning
        );

        create(
            &f.db,
            f.vehicle,
            NewServiceRecord {
                work_item_ids: Some(vec![second]),
                service_date: date("2026-11-11"),
                ..invoice(&f)
            },
        )
        .await
        .unwrap();
        assert_eq!(
            appointment_status(&f).await,
            appointment::AppointmentStatus::Done
        );
    }

    #[tokio::test]
    async fn deleting_the_service_reopens_the_appointment_and_needs_fall_back() {
        let f = fixture().await;
        let job_id = add_job(&f, "Rear brakes").await;
        let need_id = add_need(&f, job_id, "Pads", None).await;
        let part_id = link_new_part(&f, need_id, true).await;
        let created = create(
            &f.db,
            f.vehicle,
            NewServiceRecord {
                part_ids: Some(vec![part_id]),
                work_item_ids: Some(vec![job_id]),
                ..invoice(&f)
            },
        )
        .await
        .unwrap();
        assert_eq!(
            appointment_status(&f).await,
            appointment::AppointmentStatus::Done
        );
        assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);

        let deleted = delete(&f.db, f.vehicle, created.record.id, UNDO)
            .await
            .unwrap();

        // An appointment that read done from its jobs had no date to clear.
        crate::test_support::assert_empty(&deleted.reopened_appointment_ids);
        crate::test_support::assert_empty(&deleted.unassigned_job_ids);
        assert_eq!(
            appointment_status(&f).await,
            appointment::AppointmentStatus::Planning
        );
        assert_eq!(job_status(&f, job_id).await, JobStatus::Planned);
        assert_eq!(stage_of(&f, need_id).await, need::Stage::Received);
    }

    #[tokio::test]
    async fn deleting_the_service_of_a_hand_closed_appointment_opens_it_again() {
        let f = fixture().await;
        let job_id = add_job(&f, "Rear brakes").await;
        let need_id = add_need(&f, job_id, "Pads", None).await;
        let part_id = link_new_part(&f, need_id, true).await;
        let filed = || NewServiceRecord {
            part_ids: Some(vec![part_id]),
            work_item_ids: Some(vec![job_id]),
            ..invoice(&f)
        };
        let created = create(&f.db, f.vehicle, filed()).await.unwrap();
        appointment::close(&f.db, f.vehicle, f.appointment)
            .await
            .unwrap();

        let deleted = delete(&f.db, f.vehicle, created.record.id, UNDO)
            .await
            .unwrap();
        assert_eq!(deleted.reopened_appointment_ids, vec![f.appointment]);
        crate::test_support::assert_empty(&deleted.unassigned_job_ids);

        let summary =
            appointment::summary(&f.db, f.vehicle, f.appointment, crate::services::today())
                .await
                .unwrap();
        assert_eq!(summary.status, appointment::AppointmentStatus::Planning);
        assert_eq!(summary.appointment.closed_on, None);
        let open = appointment::list(&f.db, f.vehicle, false, crate::services::today())
            .await
            .unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(job_status(&f, job_id).await, JobStatus::Planned);
        // Re-recording the corrected invoice against the appointment works.
        create(&f.db, f.vehicle, filed()).await.unwrap();
        assert_eq!(
            appointment_status(&f).await,
            appointment::AppointmentStatus::Done
        );
    }

    #[tokio::test]
    async fn deleting_the_service_of_a_cancelled_appointment_unschedules_its_jobs() {
        let f = fixture().await;
        let job_id = add_job(&f, "Rear brakes").await;
        add_need(&f, job_id, "Pads", None).await;
        let created = create(
            &f.db,
            f.vehicle,
            NewServiceRecord {
                work_item_ids: Some(vec![job_id]),
                ..invoice(&f)
            },
        )
        .await
        .unwrap();
        appointment::cancel(&f.db, f.vehicle, f.appointment)
            .await
            .unwrap();

        let deleted = delete(&f.db, f.vehicle, created.record.id, UNDO)
            .await
            .unwrap();
        crate::test_support::assert_empty(&deleted.reopened_appointment_ids);
        assert_eq!(deleted.unassigned_job_ids, vec![job_id]);

        assert_eq!(
            appointment_status(&f).await,
            appointment::AppointmentStatus::Cancelled
        );
        let job = work_item::Entity::find_by_id(job_id)
            .one(&f.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(job.status, JobStatus::Planned);
        assert_eq!(job.appointment_id, None);
    }

    #[tokio::test]
    async fn deleting_the_service_unlinks_its_inline_shop_part() {
        let f = fixture().await;
        let job_id = add_job(&f, "Rear brakes").await;
        let need_id = add_need(&f, job_id, "Brake fluid", Some(Supplier::Shop)).await;
        let filed = || NewServiceRecord {
            work_item_ids: Some(vec![job_id]),
            parts: Some(vec![NewPart {
                name: "DOT4 fluid".into(),
                supplied_by: Some(Supplier::Shop),
                need_id: Some(need_id),
                ..Default::default()
            }]),
            ..invoice(&f)
        };
        let created = create(&f.db, f.vehicle, filed()).await.unwrap();

        let deleted = delete(&f.db, f.vehicle, created.record.id, UNDO)
            .await
            .unwrap();

        assert_eq!(deleted.kept_part_ids, created.part_ids);
        assert_eq!(stage_of(&f, need_id).await, need::Stage::ShopSupplies);
        let again = create(&f.db, f.vehicle, filed()).await.unwrap();
        let n = job_need::Entity::find_by_id(need_id)
            .one(&f.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(n.part_id, Some(again.part_ids[0]));
        assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);
    }

    // A correction that takes the shop's part off the invoice leaves the
    // need as the delete does, and the owner's own part stays linked.
    #[tokio::test]
    async fn correcting_the_parts_unlinks_a_removed_shop_part_only() {
        let f = fixture().await;
        let job_id = add_job(&f, "Rear brakes").await;
        let fluid = add_need(&f, job_id, "Brake fluid", Some(Supplier::Shop)).await;
        let pads = add_need(&f, job_id, "Pads", None).await;
        let own_part = link_new_part(&f, pads, true).await;
        let created = create(
            &f.db,
            f.vehicle,
            NewServiceRecord {
                part_ids: Some(vec![own_part]),
                parts: Some(vec![NewPart {
                    name: "DOT4 fluid".into(),
                    supplied_by: Some(Supplier::Shop),
                    need_id: Some(fluid),
                    ..Default::default()
                }]),
                ..invoice(&f)
            },
        )
        .await
        .unwrap();
        assert_eq!(stage_of(&f, fluid).await, need::Stage::Installed);

        update(
            &f.db,
            f.vehicle,
            created.record.id,
            UpdateServiceRecord {
                part_ids: Some(Vec::new()),
                completed_work: Some(CompletedWork::DidNotHappen),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(stage_of(&f, fluid).await, need::Stage::ShopSupplies);
        assert_eq!(stage_of(&f, pads).await, need::Stage::Received);
    }

    // A "Rear brakes" job whose shop-supplied fluid an invoice installed.
    struct Filed {
        job_id: i32,
        need_id: i32,
        service_id: i32,
        part_id: i32,
    }

    async fn file_shop_part(f: &Fixture) -> Filed {
        let job_id = add_job(f, "Rear brakes").await;
        let need_id = add_need(f, job_id, "Brake fluid", Some(Supplier::Shop)).await;
        let created = create(
            &f.db,
            f.vehicle,
            NewServiceRecord {
                work_item_ids: Some(vec![job_id]),
                parts: Some(vec![NewPart {
                    name: "DOT4 fluid".into(),
                    supplied_by: Some(Supplier::Shop),
                    need_id: Some(need_id),
                    ..Default::default()
                }]),
                ..invoice(f)
            },
        )
        .await
        .unwrap();
        Filed {
            job_id,
            need_id,
            service_id: created.record.id,
            part_id: created.part_ids[0],
        }
    }

    #[tokio::test]
    async fn work_that_did_not_happen_keeps_a_part_a_later_service_replaced() {
        let f = fixture().await;
        let Filed {
            job_id,
            need_id,
            service_id,
            part_id,
        } = file_shop_part(&f).await;
        set_part_status(&f, part_id, PartStatus::Replaced).await;

        delete(&f.db, f.vehicle, service_id, UNDO).await.unwrap();

        let p = part_svc::get(&f.db, f.vehicle, part_id).await.unwrap();
        assert_eq!(
            (p.status, p.installed_service_id),
            (PartStatus::Replaced, None)
        );
        assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);
        let j = job::get(&f.db, f.vehicle, job_id).await.unwrap();
        assert_eq!(j.status, work_item::JobStatus::Planned);
    }

    #[tokio::test]
    async fn work_done_another_way_stays_done_without_the_invoice() {
        let f = fixture().await;
        let Filed {
            job_id,
            need_id,
            service_id,
            part_id,
        } = file_shop_part(&f).await;
        let installed_on = part_svc::get(&f.db, f.vehicle, part_id)
            .await
            .unwrap()
            .installed_date;

        let deleted = delete(
            &f.db,
            f.vehicle,
            service_id,
            Some(CompletedWork::DoneAnotherWay),
        )
        .await
        .unwrap();

        assert_eq!(deleted.kept_part_ids, vec![part_id]);
        crate::test_support::assert_empty(&deleted.reopened_appointment_ids);
        let p = part_svc::get(&f.db, f.vehicle, part_id).await.unwrap();
        assert_eq!(
            (p.status, p.installed_service_id, p.installed_date),
            (PartStatus::Installed, None, installed_on)
        );
        assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);
        let j = job::get(&f.db, f.vehicle, job_id).await.unwrap();
        assert_eq!(
            (j.status, j.completed_service_id),
            (work_item::JobStatus::Done, None)
        );
    }

    #[tokio::test]
    async fn a_service_with_completed_work_refuses_a_delete_with_no_choice() {
        let f = fixture().await;
        let Filed {
            job_id,
            need_id,
            service_id,
            part_id,
        } = file_shop_part(&f).await;

        let err = delete(&f.db, f.vehicle, service_id, None)
            .await
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            format!(
                "Service {service_id} recorded this work as done: job {job_id} \"Rear brakes\", \
                 part {part_id} \"DOT4 fluid\". Deleting it needs to know whether that work did \
                 not happen or was done another way. Nothing was deleted."
            )
        );
        assert!(get(&f.db, f.vehicle, service_id).await.is_ok());
        assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);
    }

    #[tokio::test]
    async fn correcting_the_parts_keeps_a_replaced_shop_part_on_its_need() {
        let f = fixture().await;
        let Filed {
            need_id,
            service_id,
            part_id,
            ..
        } = file_shop_part(&f).await;
        set_part_status(&f, part_id, PartStatus::Replaced).await;

        update(
            &f.db,
            f.vehicle,
            service_id,
            UpdateServiceRecord {
                part_ids: Some(Vec::new()),
                completed_work: Some(CompletedWork::DidNotHappen),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let p = part_svc::get(&f.db, f.vehicle, part_id).await.unwrap();
        assert_eq!(p.status, PartStatus::Replaced);
        assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);
    }

    #[tokio::test]
    async fn appointment_from_another_vehicle_reads_as_missing() {
        let f = fixture().await;
        let other = VehicleFixture::new().name("Other").insert_id(&f.db).await;
        let missing = create(
            &f.db,
            other,
            NewServiceRecord {
                appointment_id: Some(99_999),
                ..invoice(&f)
            },
        )
        .await
        .unwrap_err();
        let wrong = create(&f.db, other, invoice(&f)).await.unwrap_err();
        assert!(matches!(wrong, DomainError::NotFound(_)));
        assert_eq!(
            wrong.to_string().replace(&f.appointment.to_string(), "N"),
            missing.to_string().replace("99999", "N")
        );
        assert_eq!(row_count::<service_record::Entity>(&f.db).await, 0);

        // The same guard covers a correction.
        let own = create(
            &f.db,
            other,
            NewServiceRecord {
                appointment_id: None,
                ..invoice(&f)
            },
        )
        .await
        .unwrap();
        let err = update(
            &f.db,
            other,
            own.record.id,
            UpdateServiceRecord {
                appointment_id: Some(Some(f.appointment)),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)));
    }

    #[tokio::test]
    async fn cancelled_or_closed_appointment_is_refused() {
        let f = fixture().await;
        appointment::cancel(&f.db, f.vehicle, f.appointment)
            .await
            .unwrap();
        let err = create(&f.db, f.vehicle, invoice(&f)).await.unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)));
        assert_eq!(row_count::<service_record::Entity>(&f.db).await, 0);
    }

    #[tokio::test]
    async fn correcting_a_service_sets_changes_and_clears_its_appointment() {
        let f = fixture().await;
        let second = appointment::create(
            &f.db,
            f.vehicle,
            NewAppointment {
                purpose: "Oil".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        let created = create(
            &f.db,
            f.vehicle,
            NewServiceRecord {
                appointment_id: None,
                ..invoice(&f)
            },
        )
        .await
        .unwrap();
        assert_eq!(created.record.appointment_id, None);
        let set = |value| UpdateServiceRecord {
            appointment_id: Some(value),
            ..Default::default()
        };
        let id = created.record.id;
        let r = update(&f.db, f.vehicle, id, set(Some(f.appointment)))
            .await
            .unwrap();
        assert_eq!(r.record.appointment_id, Some(f.appointment));
        let r = update(&f.db, f.vehicle, id, set(Some(second)))
            .await
            .unwrap();
        assert_eq!(r.record.appointment_id, Some(second));
        // An absent key leaves it alone.
        let r = update(&f.db, f.vehicle, id, UpdateServiceRecord::default())
            .await
            .unwrap();
        assert_eq!(r.record.appointment_id, Some(second));
        let r = update(&f.db, f.vehicle, id, set(None)).await.unwrap();
        assert_eq!(r.record.appointment_id, None);
    }

    #[tokio::test]
    async fn inline_part_need_from_another_vehicle_reads_as_missing() {
        let f = fixture().await;
        let other = VehicleFixture::new().name("Other").insert_id(&f.db).await;
        let job_id = add_job(&f, "Rear brakes").await;
        let need_id = add_need(&f, job_id, "Pads", Some(Supplier::Shop)).await;
        let inline = |need_id| NewServiceRecord {
            appointment_id: None,
            parts: Some(vec![NewPart {
                name: "Pads".into(),
                need_id: Some(need_id),
                ..Default::default()
            }]),
            ..invoice(&f)
        };
        let missing = create(&f.db, other, inline(99_999)).await.unwrap_err();
        let wrong = create(&f.db, other, inline(need_id)).await.unwrap_err();
        assert!(matches!(wrong, DomainError::NotFound(_)));
        assert_eq!(
            wrong.to_string().replace(&need_id.to_string(), "N"),
            missing.to_string().replace("99999", "N")
        );
        assert_eq!(row_count::<service_record::Entity>(&f.db).await, 0);
        assert_eq!(row_count::<part::Entity>(&f.db).await, 0);
    }

    #[tokio::test]
    async fn one_bad_need_id_commits_nothing() {
        let f = fixture().await;
        let job_id = add_job(&f, "Rear brakes").await;
        let good = add_need(&f, job_id, "Pads", Some(Supplier::Shop)).await;
        let linked = add_need(&f, job_id, "Sensor", None).await;
        link_new_part(&f, linked, false).await;
        let parts_before = row_count::<part::Entity>(&f.db).await;

        for (bad, expect_missing) in [(99_999, true), (linked, false)] {
            let err = create(
                &f.db,
                f.vehicle,
                NewServiceRecord {
                    work_item_ids: Some(vec![job_id]),
                    parts: Some(vec![
                        NewPart {
                            name: "Pads".into(),
                            need_id: Some(good),
                            ..Default::default()
                        },
                        NewPart {
                            name: "Other".into(),
                            need_id: Some(bad),
                            ..Default::default()
                        },
                    ]),
                    ..invoice(&f)
                },
            )
            .await
            .unwrap_err();
            // A nonexistent need reads as missing; a need that already has a
            // part is a malformed request.
            if expect_missing {
                assert!(matches!(err, DomainError::NotFound(_)), "got: {err:?}");
            } else {
                assert!(matches!(err, DomainError::BadRequest(_)), "got: {err:?}");
            }
            assert_eq!(row_count::<service_record::Entity>(&f.db).await, 0);
            assert_eq!(row_count::<part::Entity>(&f.db).await, parts_before);
            assert_eq!(job_status(&f, job_id).await, JobStatus::Planned);
            let n = job_need::Entity::find_by_id(good)
                .one(&f.db)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(n.part_id, None);
        }
    }

    #[tokio::test]
    async fn re_recording_an_invoice_ref_with_another_appointment_writes_nothing() {
        let f = fixture().await;
        let second = appointment::create(
            &f.db,
            f.vehicle,
            NewAppointment {
                purpose: "Wipers".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        let job_id = add_job(&f, "Rear brakes").await;
        let need_id = add_need(&f, job_id, "Pads", Some(Supplier::Shop)).await;
        let first = create(
            &f.db,
            f.vehicle,
            NewServiceRecord {
                invoice_ref: Some("RO-1".into()),
                ..invoice(&f)
            },
        )
        .await
        .unwrap();
        let parts_before = row_count::<part::Entity>(&f.db).await;

        let again = create(
            &f.db,
            f.vehicle,
            NewServiceRecord {
                invoice_ref: Some("RO-1".into()),
                appointment_id: Some(second),
                parts: Some(vec![NewPart {
                    name: "Pads".into(),
                    need_id: Some(need_id),
                    ..Default::default()
                }]),
                ..invoice(&f)
            },
        )
        .await
        .unwrap();

        assert!(again.already_recorded);
        assert_eq!(again.record.id, first.record.id);
        assert_eq!(again.record.appointment_id, Some(f.appointment));
        assert_eq!(row_count::<service_record::Entity>(&f.db).await, 1);
        assert_eq!(row_count::<part::Entity>(&f.db).await, parts_before);
        assert_eq!(stage_of(&f, need_id).await, need::Stage::ShopSupplies);
        let n = job_need::Entity::find_by_id(need_id)
            .one(&f.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(n.part_id, None);
    }

    #[tokio::test]
    async fn deleting_a_service_leaves_an_ordered_need_ordered() {
        let f = fixture().await;
        let job_id = add_job(&f, "Rear brakes").await;
        let need_id = add_need(&f, job_id, "Pads", None).await;
        // Ordered but never received.
        let part_id = link_new_part(&f, need_id, false).await;
        let created = create(
            &f.db,
            f.vehicle,
            NewServiceRecord {
                part_ids: Some(vec![part_id]),
                work_item_ids: Some(vec![job_id]),
                ..invoice(&f)
            },
        )
        .await
        .unwrap();
        assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);

        delete(&f.db, f.vehicle, created.record.id, UNDO)
            .await
            .unwrap();

        assert_eq!(stage_of(&f, need_id).await, need::Stage::Ordered);
        assert_eq!(job_status(&f, job_id).await, JobStatus::Planned);
    }

    #[tokio::test]
    async fn filing_an_invoice_on_a_closed_appointment_gives_a_neutral_message() {
        let f = fixture().await;
        appointment::cancel(&f.db, f.vehicle, f.appointment)
            .await
            .unwrap();

        let err = create(&f.db, f.vehicle, invoice(&f)).await.unwrap_err();

        assert!(matches!(err, DomainError::BadRequest(_)), "got: {err:?}");
        assert_eq!(
            err.to_string(),
            format!(
                "Appointment {} is cancelled, so it takes no new work",
                f.appointment
            )
        );
        assert_eq!(row_count::<service_record::Entity>(&f.db).await, 0);
    }
}

// `update` taking recorded work off a service: the jobs, parts and
// recalls its replacement lists leave out.
mod corrections {
    use super::*;
    use crate::{
        inputs::{appointment::NewAppointment, work_item::NewWorkItem},
        services::{appointment, work_item as job},
    };

    async fn planned(db: &DatabaseConnection, v: i32, title: &str, appt: Option<i32>) -> i32 {
        job::create(
            db,
            v,
            NewWorkItem {
                title: title.into(),
                appointment_id: appt,
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    async fn completing(db: &DatabaseConnection, v: i32, jobs: Vec<i32>) -> i32 {
        create(
            db,
            v,
            NewServiceRecord {
                service_date: date("2026-11-01"),
                work_item_ids: Some(jobs),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .record
        .id
    }

    fn jobs(ids: Vec<i32>, completed: Option<CompletedWork>) -> UpdateServiceRecord {
        UpdateServiceRecord {
            work_item_ids: Some(ids),
            completed_work: completed,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn dropping_a_completed_job_needs_an_answer_and_changes_nothing_without_one() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let j = planned(&db, v, "Flush brake fluid", None).await;
        let svc = completing(&db, v, vec![j]).await;

        let err = update(
            &db,
            v,
            svc,
            UpdateServiceRecord {
                notes: Some(Some("edited".into())),
                ..jobs(vec![], None)
            },
        )
        .await
        .unwrap_err();
        let DomainError::BadRequest(message) = err else {
            panic!("{err:?}");
        };
        assert!(
            message.contains(&format!("job {j} \"Flush brake fluid\"")),
            "{message}"
        );
        assert!(message.contains("Taking it off the service"), "{message}");
        assert_eq!(job::get(&db, v, j).await.unwrap().status, JobStatus::Done);
        assert_eq!(get(&db, v, svc).await.unwrap().record.notes, None);
    }

    #[tokio::test]
    async fn did_not_happen_puts_the_job_back_and_reopens_its_closed_appointment() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let appt = appointment::create(
            &db,
            v,
            NewAppointment {
                purpose: "Brakes".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        let j = planned(&db, v, "Pads", Some(appt)).await;
        let svc = completing(&db, v, vec![j]).await;
        appointment::close(&db, v, appt).await.unwrap();

        let out = update(&db, v, svc, jobs(vec![], Some(CompletedWork::DidNotHappen)))
            .await
            .unwrap();
        let back = job::get(&db, v, j).await.unwrap();
        assert_eq!(back.status, JobStatus::Planned);
        assert_eq!(back.completed_service_id, None);
        assert_eq!(out.reopened_appointment_ids, vec![appt]);
        crate::test_support::assert_empty(&out.completed_work_items);
    }

    #[tokio::test]
    async fn done_another_way_keeps_the_job_done_without_the_service() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let j = planned(&db, v, "Pads", None).await;
        let svc = completing(&db, v, vec![j]).await;

        update(
            &db,
            v,
            svc,
            jobs(vec![], Some(CompletedWork::DoneAnotherWay)),
        )
        .await
        .unwrap();
        let kept = job::get(&db, v, j).await.unwrap();
        assert_eq!(kept.status, JobStatus::Done);
        assert_eq!(kept.completed_service_id, None);
    }

    #[tokio::test]
    async fn a_job_added_to_the_list_is_completed_and_a_kept_one_is_left_alone() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let first = planned(&db, v, "Pads", None).await;
        let second = planned(&db, v, "Rotors", None).await;
        let svc = completing(&db, v, vec![first]).await;

        let out = update(&db, v, svc, jobs(vec![first, second], None))
            .await
            .unwrap();
        let ids: Vec<i32> = out.completed_work_items.iter().map(|w| w.id).collect();
        assert_eq!(ids, vec![first, second]);
        assert_eq!(
            job::get(&db, v, second).await.unwrap().completed_service_id,
            Some(svc)
        );

        // A job another service completed is not planned, so it is refused.
        let elsewhere = planned(&db, v, "Wipers", None).await;
        completing(&db, v, vec![elsewhere]).await;
        let err = update(&db, v, svc, jobs(vec![first, second, elsewhere], None))
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
    }

    #[tokio::test]
    async fn a_job_of_another_vehicle_reads_as_missing() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let other = VehicleFixture::new().name("Other").insert_id(&db).await;
        let theirs = planned(&db, other, "Pads", None).await;
        let svc = completing(&db, v, vec![]).await;

        let wrong = update(&db, v, svc, jobs(vec![theirs], None))
            .await
            .unwrap_err();
        let missing = update(&db, v, svc, jobs(vec![99_999], None))
            .await
            .unwrap_err();
        assert_eq!(
            wrong.to_string().replace(&theirs.to_string(), "N"),
            missing.to_string().replace("99999", "N")
        );
    }

    #[tokio::test]
    async fn a_part_and_a_recall_done_another_way_keep_their_state() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let finding = seed_recall(&db, v, "24V555").await;
        let p = part_svc::create(
            &db,
            v,
            crate::inputs::part::NewPart {
                name: "Pads".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        let svc = create(
            &db,
            v,
            NewServiceRecord {
                service_date: date("2026-11-01"),
                part_ids: Some(vec![p]),
                recall_finding_ids: Some(vec![finding]),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .record
        .id;

        let drop_both = || UpdateServiceRecord {
            part_ids: Some(vec![]),
            recall_finding_ids: Some(vec![]),
            ..Default::default()
        };
        let err = update(&db, v, svc, drop_both()).await.unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");

        update(
            &db,
            v,
            svc,
            UpdateServiceRecord {
                completed_work: Some(CompletedWork::DoneAnotherWay),
                ..drop_both()
            },
        )
        .await
        .unwrap();
        let part = part_svc::get(&db, v, p).await.unwrap();
        assert_eq!(part.status, PartStatus::Installed);
        assert_eq!(part.installed_service_id, None);
        assert_eq!(
            recall_status(&db, v).await,
            vec![(recall::ShownRecallStatus::Done, None)]
        );
    }

    async fn job_from(db: &DatabaseConnection, v: i32, incident: i32, finding: i32) -> i32 {
        job::create(
            db,
            v,
            NewWorkItem {
                title: "Replace N80 valve".into(),
                incident_id: Some(incident),
                research_finding_id: Some(finding),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    async fn resolved(db: &DatabaseConnection, v: i32, id: i32) -> bool {
        incident::get(db, v, id).await.unwrap().incident.resolved
    }

    #[tokio::test]
    async fn a_job_added_resolves_its_incident_and_performs_its_recall() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let finding = seed_recall(&db, v, "24V777").await;
        let j = job_from(&db, v, incident, finding).await;
        let svc = completing(&db, v, vec![]).await;

        let out = update(&db, v, svc, jobs(vec![j], None)).await.unwrap();
        assert!(resolved(&db, v, incident).await);
        let performed: Vec<i32> = out.performed_recalls.iter().map(|r| r.finding_id).collect();
        assert_eq!(performed, vec![finding]);
    }

    async fn job_from_recall(db: &DatabaseConnection, v: i32, finding: i32) -> i32 {
        job::create(
            db,
            v,
            NewWorkItem {
                title: "Recall repair".into(),
                research_finding_id: Some(finding),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    fn no_recalls_but(ids: Vec<i32>) -> UpdateServiceRecord {
        UpdateServiceRecord {
            recall_finding_ids: Some(vec![]),
            ..jobs(ids, None)
        }
    }

    #[tokio::test]
    async fn an_empty_recall_list_keeps_the_recall_of_a_job_added_in_the_same_call() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let finding = seed_recall(&db, v, "24V888").await;
        let j = job_from_recall(&db, v, finding).await;
        let svc = create(
            &db,
            v,
            NewServiceRecord {
                service_date: date("2026-11-01"),
                recall_finding_ids: Some(vec![finding]),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .record
        .id;

        // The job brings the recall back, so nothing is released.
        let out = update(&db, v, svc, no_recalls_but(vec![j])).await.unwrap();
        assert_eq!(job::get(&db, v, j).await.unwrap().status, JobStatus::Done);
        let performed: Vec<i32> = out.performed_recalls.iter().map(|r| r.finding_id).collect();
        assert_eq!(performed, vec![finding]);
        assert_eq!(
            recall_status(&db, v).await,
            vec![(recall::ShownRecallStatus::Done, Some(svc))]
        );
    }

    #[tokio::test]
    async fn an_empty_recall_list_keeps_the_recall_of_a_job_the_service_already_completed() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let finding = seed_recall(&db, v, "24V889").await;
        let j = job_from_recall(&db, v, finding).await;
        let svc = completing(&db, v, vec![j]).await;

        let out = update(&db, v, svc, no_recalls_but(vec![j])).await.unwrap();
        assert_eq!(job::get(&db, v, j).await.unwrap().status, JobStatus::Done);
        let performed: Vec<i32> = out.performed_recalls.iter().map(|r| r.finding_id).collect();
        assert_eq!(performed, vec![finding]);
    }

    #[tokio::test]
    async fn an_empty_recall_list_without_a_job_list_keeps_the_recall_of_a_completed_job() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let finding = seed_recall(&db, v, "24V893").await;
        let j = job_from_recall(&db, v, finding).await;
        let svc = completing(&db, v, vec![j]).await;

        // The service keeps the job, so its recall stays performed and the
        // call needs no `completed_work`.
        let out = update(
            &db,
            v,
            svc,
            UpdateServiceRecord {
                recall_finding_ids: Some(vec![]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(job::get(&db, v, j).await.unwrap().status, JobStatus::Done);
        let performed: Vec<i32> = out.performed_recalls.iter().map(|r| r.finding_id).collect();
        assert_eq!(performed, vec![finding]);
        assert_eq!(
            recall_status(&db, v).await,
            vec![(recall::ShownRecallStatus::Done, Some(svc))]
        );
    }

    #[tokio::test]
    async fn an_empty_recall_list_without_a_job_list_takes_off_only_a_recall_no_job_brings() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let named = seed_recall(&db, v, "24V894").await;
        // `seed_recall` returns the vehicle's first recall, so the second
        // one is read back from the list.
        seed_recall(&db, v, "24V895").await;
        let from_job = recall::list(&db, v)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.finding_id)
            .find(|id| *id != named)
            .unwrap();
        let j = job_from_recall(&db, v, from_job).await;
        let svc = create(
            &db,
            v,
            NewServiceRecord {
                service_date: date("2026-11-01"),
                recall_finding_ids: Some(vec![named]),
                work_item_ids: Some(vec![j]),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .record
        .id;
        let no_recalls = |completed_work| UpdateServiceRecord {
            recall_finding_ids: Some(vec![]),
            completed_work,
            ..Default::default()
        };

        let err = update(&db, v, svc, no_recalls(None)).await.unwrap_err();
        let DomainError::BadRequest(message) = err else {
            panic!("{err:?}");
        };
        assert!(message.contains(&format!("recall {named}")), "{message}");
        assert!(
            !message.contains(&format!("recall {from_job}")),
            "{message}"
        );

        let out = update(&db, v, svc, no_recalls(Some(CompletedWork::DidNotHappen)))
            .await
            .unwrap();
        let performed: Vec<i32> = out.performed_recalls.iter().map(|r| r.finding_id).collect();
        assert_eq!(performed, vec![from_job]);
    }

    #[tokio::test]
    async fn a_recall_list_gives_a_kept_job_no_recall_link_it_did_not_have() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let finding = seed_recall(&db, v, "24V896").await;
        let j = job_from_recall(&db, v, finding).await;
        let svc = completing(&db, v, vec![j]).await;
        // The service keeps the job without a link to its recall, as it
        // does after the owner says the recall was done another way.
        recall::unlink_from_service(&db, svc, None).await.unwrap();

        let out = update(
            &db,
            v,
            svc,
            UpdateServiceRecord {
                recall_finding_ids: Some(vec![]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        crate::test_support::assert_empty(&out.performed_recalls);
    }

    #[tokio::test]
    async fn listing_a_kept_job_without_a_recall_list_adds_no_recall_link() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let finding = seed_recall(&db, v, "24V892").await;
        let j = job_from_recall(&db, v, finding).await;
        let svc = completing(&db, v, vec![j]).await;
        recall::unlink_from_service(&db, svc, None).await.unwrap();

        let out = update(&db, v, svc, jobs(vec![j], None)).await.unwrap();
        crate::test_support::assert_empty(&out.performed_recalls);
    }

    #[tokio::test]
    async fn a_recall_left_out_of_a_sent_list_with_no_job_behind_it_asks_for_an_answer() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let finding = seed_recall(&db, v, "24V890").await;
        // `seed_recall` returns the vehicle's first recall, so the second
        // one is read back from the list.
        seed_recall(&db, v, "24V891").await;
        let other = recall::list(&db, v)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.finding_id)
            .find(|id| *id != finding)
            .unwrap();
        let j = job_from_recall(&db, v, other).await;
        let svc = create(
            &db,
            v,
            NewServiceRecord {
                service_date: date("2026-11-01"),
                recall_finding_ids: Some(vec![finding]),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .record
        .id;

        let err = update(&db, v, svc, no_recalls_but(vec![j]))
            .await
            .unwrap_err();
        let DomainError::BadRequest(message) = err else {
            panic!("{err:?}");
        };
        assert!(message.contains(&format!("recall {finding}")), "{message}");
    }

    #[tokio::test]
    async fn a_job_taken_off_as_not_done_takes_its_incident_recall_and_parts_with_it() {
        use crate::entities::job_need;
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let finding = seed_recall(&db, v, "24V777").await;
        let j = job_from(&db, v, incident, finding).await;
        let p = part_svc::create(
            &db,
            v,
            crate::inputs::part::NewPart {
                name: "N80 valve".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        job_need::ActiveModel {
            work_item_id: Set(j),
            name: Set("Valve".into()),
            quantity: Set(1),
            part_id: Set(Some(p)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let svc = create(
            &db,
            v,
            NewServiceRecord {
                service_date: date("2026-11-01"),
                work_item_ids: Some(vec![j]),
                part_ids: Some(vec![p]),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .record
        .id;
        assert!(resolved(&db, v, incident).await);

        let err = update(&db, v, svc, jobs(vec![], None)).await.unwrap_err();
        let DomainError::BadRequest(message) = err else {
            panic!("{err:?}");
        };
        for named in [
            format!("job {j}"),
            format!("part {p}"),
            format!("incident {incident}"),
            format!("recall {finding}"),
        ] {
            assert!(message.contains(&named), "{named} in {message}");
        }

        let out = update(&db, v, svc, jobs(vec![], Some(CompletedWork::DidNotHappen)))
            .await
            .unwrap();
        assert!(!resolved(&db, v, incident).await);
        crate::test_support::assert_empty(&out.performed_recalls);
        crate::test_support::assert_empty(&out.part_ids);
        assert_eq!(
            part_svc::get(&db, v, p).await.unwrap().status,
            PartStatus::Purchased
        );
    }

    // Files a job against a new "Brake fluid" schedule item and returns
    // the item's id and the job's id.
    async fn fluid_job(db: &DatabaseConnection, v: i32) -> (i32, i32) {
        let item = crate::services::schedule::create_for_vehicle(
            db,
            v,
            crate::inputs::schedule::NewScheduleItem {
                name: "Brake fluid".into(),
                interval_months: Some(24),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        let j = job::create(
            db,
            v,
            NewWorkItem {
                title: "Flush brake fluid".into(),
                schedule_item_id: Some(item),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        (item, j)
    }

    // Returns the service that the "Brake fluid" reminder counts from.
    async fn fluid_baseline(db: &DatabaseConnection, v: i32) -> Option<i32> {
        let reminders = crate::services::reminders::calculate_reminders(db, v)
            .await
            .unwrap()
            .reminders;
        reminders[0].last_service.as_ref().map(|s| s.id)
    }

    #[tokio::test]
    async fn a_jobs_schedule_item_counts_while_the_service_completes_the_job() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let (item, j) = fluid_job(&db, v).await;
        let svc = completing(&db, v, vec![]).await;

        // The list names the items the invoice itself satisfies, and the
        // job brings its own item whatever the list holds.
        let out = update(
            &db,
            v,
            svc,
            UpdateServiceRecord {
                schedule_item_ids: Some(vec![]),
                ..jobs(vec![j], None)
            },
        )
        .await
        .unwrap();
        assert_eq!(out.schedule_item_ids, vec![item]);
        assert_eq!(list(&db, v).await.unwrap()[0].schedule_item_ids, vec![item]);
        assert_eq!(fluid_baseline(&db, v).await, Some(svc));
        assert_eq!(row_count::<service_schedule_link::Entity>(&db).await, 0);
    }

    #[tokio::test]
    async fn deleting_a_completed_job_keeps_its_schedule_items_baseline() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let (item, j) = fluid_job(&db, v).await;
        let svc = completing(&db, v, vec![j]).await;

        job::delete(&db, v, j).await.unwrap();

        assert_eq!(
            get(&db, v, svc).await.unwrap().schedule_item_ids,
            vec![item]
        );
        assert_eq!(fluid_baseline(&db, v).await, Some(svc));
    }

    #[tokio::test]
    async fn a_job_taken_off_takes_its_schedule_items_baseline_with_it() {
        for choice in [CompletedWork::DidNotHappen, CompletedWork::DoneAnotherWay] {
            let db = test_db().await;
            let v = seed_vehicle(&db).await;
            let (item, j) = fluid_job(&db, v).await;
            let svc = completing(&db, v, vec![j]).await;
            assert_eq!(
                get(&db, v, svc).await.unwrap().schedule_item_ids,
                vec![item]
            );
            assert_eq!(fluid_baseline(&db, v).await, Some(svc));

            let out = update(&db, v, svc, jobs(vec![], Some(choice)))
                .await
                .unwrap();
            crate::test_support::assert_empty(&out.schedule_item_ids);
            assert_eq!(fluid_baseline(&db, v).await, None, "{choice:?}");
        }
    }

    #[tokio::test]
    async fn an_item_named_beside_its_job_goes_with_the_job() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let (item, j) = fluid_job(&db, v).await;
        let svc = create(
            &db,
            v,
            NewServiceRecord {
                service_date: date("2026-11-01"),
                work_item_ids: Some(vec![j]),
                schedule_item_ids: Some(vec![item]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(svc.schedule_item_ids, vec![item]);
        assert_eq!(row_count::<service_schedule_link::Entity>(&db).await, 0);

        let out = update(
            &db,
            v,
            svc.record.id,
            jobs(vec![], Some(CompletedWork::DidNotHappen)),
        )
        .await
        .unwrap();
        crate::test_support::assert_empty(&out.schedule_item_ids);
        assert_eq!(fluid_baseline(&db, v).await, None);
    }

    #[tokio::test]
    async fn sending_back_the_list_a_read_gave_stores_no_link_for_a_jobs_item() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let (item, j) = fluid_job(&db, v).await;
        let other = seed_schedule_item(&db, v, "Cabin filter").await;
        let svc = completing(&db, v, vec![j]).await;

        // The read lists the job's item, and the call sends it back with
        // one more.
        let read = get(&db, v, svc).await.unwrap().schedule_item_ids;
        assert_eq!(read, vec![item]);
        for mode in [LinkMode::Replace, LinkMode::Add] {
            link_schedule_items(&db, v, svc, &[item, other], mode)
                .await
                .unwrap();
        }

        let out = update(&db, v, svc, jobs(vec![], Some(CompletedWork::DidNotHappen)))
            .await
            .unwrap();
        assert_eq!(out.schedule_item_ids, vec![other]);
    }

    #[tokio::test]
    async fn a_list_sent_with_the_call_that_takes_a_job_off_does_not_keep_its_item() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let (item, j) = fluid_job(&db, v).await;
        let other = seed_schedule_item(&db, v, "Cabin filter").await;
        let svc = completing(&db, v, vec![j]).await;

        let out = update(
            &db,
            v,
            svc,
            UpdateServiceRecord {
                schedule_item_ids: Some(vec![item, other]),
                ..jobs(vec![], Some(CompletedWork::DidNotHappen))
            },
        )
        .await
        .unwrap();
        assert_eq!(out.schedule_item_ids, vec![other]);
    }

    #[tokio::test]
    async fn taking_a_job_off_names_the_schedule_item_that_stopped_counting() {
        for choice in [CompletedWork::DidNotHappen, CompletedWork::DoneAnotherWay] {
            let db = test_db().await;
            let v = seed_vehicle(&db).await;
            let (item, j) = fluid_job(&db, v).await;
            let svc = completing(&db, v, vec![j]).await;

            let out = update(&db, v, svc, jobs(vec![], Some(choice)))
                .await
                .unwrap();
            assert_eq!(out.released_schedule_item_ids, vec![item], "{choice:?}");
        }
    }

    #[tokio::test]
    async fn a_correction_that_changes_no_item_names_none() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let (_, j) = fluid_job(&db, v).await;
        let svc = completing(&db, v, vec![j]).await;

        let out = update(
            &db,
            v,
            svc,
            UpdateServiceRecord {
                notes: Some(Some("Fine".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        crate::test_support::assert_empty(&out.released_schedule_item_ids);
    }

    #[tokio::test]
    async fn an_item_a_stored_link_or_another_kept_job_still_satisfies_is_not_named() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let (item, j) = fluid_job(&db, v).await;
        let twin = job::create(
            &db,
            v,
            NewWorkItem {
                title: "Flush again".into(),
                schedule_item_id: Some(item),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        let svc = completing(&db, v, vec![j, twin]).await;
        let out = update(
            &db,
            v,
            svc,
            jobs(vec![twin], Some(CompletedWork::DidNotHappen)),
        )
        .await
        .unwrap();
        crate::test_support::assert_empty(&out.released_schedule_item_ids);
        assert_eq!(out.schedule_item_ids, vec![item]);

        let other = seed_schedule_item(&db, v, "Cabin filter").await;
        link_schedule_items(&db, v, svc, &[other], LinkMode::Add)
            .await
            .unwrap();
        let out = update(&db, v, svc, jobs(vec![], Some(CompletedWork::DidNotHappen)))
            .await
            .unwrap();
        assert_eq!(out.released_schedule_item_ids, vec![item]);
        assert_eq!(out.schedule_item_ids, vec![other]);
    }

    #[tokio::test]
    async fn deleting_a_service_names_its_stored_and_job_items() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let (item, j) = fluid_job(&db, v).await;
        let other = seed_schedule_item(&db, v, "Cabin filter").await;
        let svc = completing(&db, v, vec![j]).await;
        link_schedule_items(&db, v, svc, &[other], LinkMode::Add)
            .await
            .unwrap();

        let deleted = delete(&db, v, svc, UNDO).await.unwrap();
        let mut expected = vec![item, other];
        expected.sort_unstable();
        assert_eq!(deleted.released_schedule_item_ids, expected);
    }

    #[tokio::test]
    async fn a_job_added_for_an_item_the_service_names_takes_the_link_over() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let (item, j) = fluid_job(&db, v).await;
        let svc = create(
            &db,
            v,
            NewServiceRecord {
                service_date: date("2026-11-01"),
                schedule_item_ids: Some(vec![item]),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .record
        .id;
        assert_eq!(row_count::<service_schedule_link::Entity>(&db).await, 1);

        let out = update(&db, v, svc, jobs(vec![j], None)).await.unwrap();
        assert_eq!(out.schedule_item_ids, vec![item]);
        assert_eq!(row_count::<service_schedule_link::Entity>(&db).await, 0);
    }

    #[tokio::test]
    async fn a_completed_job_moved_to_an_item_the_service_names_takes_the_link_over() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let (item, j) = fluid_job(&db, v).await;
        let other = seed_schedule_item(&db, v, "Cabin filter").await;
        let svc = create(
            &db,
            v,
            NewServiceRecord {
                service_date: date("2026-11-01"),
                work_item_ids: Some(vec![j]),
                schedule_item_ids: Some(vec![other]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(svc.schedule_item_ids, vec![item, other]);

        job::update(
            &db,
            v,
            j,
            crate::inputs::work_item::UpdateWorkItem {
                schedule_item_id: Some(Some(other)),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let read = get(&db, v, svc.record.id).await.unwrap();
        assert_eq!(read.schedule_item_ids, vec![other]);
        assert_eq!(row_count::<service_schedule_link::Entity>(&db).await, 0);
    }

    #[tokio::test]
    async fn an_incident_a_kept_job_shares_stays_resolved() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let finding = seed_recall(&db, v, "24V777").await;
        let a = job_from(&db, v, incident, finding).await;
        let b = job_from(&db, v, incident, finding).await;
        let svc = completing(&db, v, vec![a, b]).await;

        let out = update(
            &db,
            v,
            svc,
            jobs(vec![b], Some(CompletedWork::DidNotHappen)),
        )
        .await
        .unwrap();
        assert!(resolved(&db, v, incident).await);
        let performed: Vec<i32> = out.performed_recalls.iter().map(|r| r.finding_id).collect();
        assert_eq!(performed, vec![finding]);
    }

    async fn job_for(db: &DatabaseConnection, v: i32, incident: i32) -> i32 {
        job::create(
            db,
            v,
            NewWorkItem {
                title: "Replace N80 valve".into(),
                incident_id: Some(incident),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    // Returns the ids of the services that read as having resolved the
    // incident.
    async fn resolvers(db: &DatabaseConnection, v: i32, incident: i32) -> Vec<i32> {
        let details = incident::get(db, v, incident).await.unwrap();
        details.resolved_by.iter().map(|s| s.id).collect()
    }

    async fn stored_incident_links(db: &DatabaseConnection) -> u64 {
        crate::test_support::row_count::<incident_service_link::Entity>(db).await
    }

    #[tokio::test]
    async fn a_jobs_incident_is_read_from_the_job_and_reopens_with_it() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let j = job_for(&db, v, incident).await;
        let svc = completing(&db, v, vec![j]).await;

        assert_eq!(stored_incident_links(&db).await, 0);
        assert!(resolved(&db, v, incident).await);
        assert_eq!(resolvers(&db, v, incident).await, vec![svc]);
        let read = get(&db, v, svc).await.unwrap();
        let fixed: Vec<i32> = read.resolved_incidents.iter().map(|i| i.id).collect();
        assert_eq!(fixed, vec![incident]);

        update(&db, v, svc, jobs(vec![], Some(CompletedWork::DidNotHappen)))
            .await
            .unwrap();
        assert!(!resolved(&db, v, incident).await);
        crate::test_support::assert_empty(&resolvers(&db, v, incident).await);
    }

    #[tokio::test]
    async fn an_incident_the_invoice_named_stays_resolved_when_its_job_is_taken_off() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let j = job_for(&db, v, incident).await;
        let svc = create(
            &db,
            v,
            NewServiceRecord {
                service_date: date("2026-11-01"),
                work_item_ids: Some(vec![j]),
                resolved_incident_ids: Some(vec![incident]),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .record
        .id;
        assert_eq!(resolvers(&db, v, incident).await, vec![svc]);

        // The job still needs an answer, and the refusal names no incident.
        let err = update(&db, v, svc, jobs(vec![], None)).await.unwrap_err();
        assert!(!err.to_string().contains("incident"), "{err}");

        let out = update(&db, v, svc, jobs(vec![], Some(CompletedWork::DidNotHappen)))
            .await
            .unwrap();
        assert_eq!(
            job::get(&db, v, j).await.unwrap().status,
            JobStatus::Planned
        );
        assert!(resolved(&db, v, incident).await);
        assert_eq!(resolvers(&db, v, incident).await, vec![svc]);
        let fixed: Vec<i32> = out.resolved_incidents.iter().map(|i| i.id).collect();
        assert_eq!(fixed, vec![incident]);
    }

    #[tokio::test]
    async fn a_job_done_another_way_leaves_its_incident_resolved_on_the_owners_word() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let j = job_for(&db, v, incident).await;
        let svc = completing(&db, v, vec![j]).await;

        update(
            &db,
            v,
            svc,
            jobs(vec![], Some(CompletedWork::DoneAnotherWay)),
        )
        .await
        .unwrap();
        assert!(resolved(&db, v, incident).await);
        crate::test_support::assert_empty(&resolvers(&db, v, incident).await);
        // No service resolves it, so a later job undone as not done
        // leaves it resolved.
        assert!(by_hand(&db, v, incident).await);
    }

    #[tokio::test]
    async fn resolved_sent_by_the_owner_marks_an_incident_by_hand() {
        use crate::inputs::incident::UpdateIncident;
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let j = job_for(&db, v, incident).await;
        completing(&db, v, vec![j]).await;
        // The job goes back to planned, which leaves the incident resolved
        // with no service behind it.
        job::update(
            &db,
            v,
            j,
            crate::inputs::work_item::UpdateWorkItem {
                status: Some(JobStatus::Planned),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(resolved(&db, v, incident).await);
        assert!(!by_hand(&db, v, incident).await);
        // The owner confirms it is fixed, and that word marks it.
        resolve_by_hand(&db, v, incident).await;
        assert!(by_hand(&db, v, incident).await);

        // The owner reopens it and closes it again while a job's service
        // resolves it, and that word marks it too.
        let svc = completing(&db, v, vec![j]).await;
        let flag = |resolved| UpdateIncident {
            resolved: Some(resolved),
            ..Default::default()
        };
        incident::update(&db, v, incident, flag(false))
            .await
            .unwrap();
        incident::update(&db, v, incident, flag(true))
            .await
            .unwrap();
        assert!(by_hand(&db, v, incident).await);

        // Sent beside the service a job brings, the same word resolves an
        // open incident through that service.
        let sent = |resolved| UpdateIncident {
            resolved: Some(resolved),
            resolved_by_service_ids: Some(vec![svc]),
            ..Default::default()
        };
        incident::update(&db, v, incident, sent(false))
            .await
            .unwrap();
        incident::update(&db, v, incident, sent(true))
            .await
            .unwrap();
        assert!(resolved(&db, v, incident).await);
        assert!(!by_hand(&db, v, incident).await);
    }

    #[tokio::test]
    async fn a_resolver_list_sent_back_stores_nothing_for_a_jobs_service() {
        use crate::inputs::incident::UpdateIncident;
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let j = job_for(&db, v, incident).await;
        let svc = completing(&db, v, vec![j]).await;
        let resolved_by = |ids: Vec<i32>| UpdateIncident {
            resolved_by_service_ids: Some(ids),
            ..Default::default()
        };

        let echoed = incident::update(&db, v, incident, resolved_by(vec![svc]))
            .await
            .unwrap();
        assert_eq!(stored_incident_links(&db).await, 0);
        assert_eq!(echoed.resolved_by.len(), 1);

        // An empty list cannot take off the service a job brings, so the
        // incident stays resolved.
        let cleared = incident::update(&db, v, incident, resolved_by(vec![]))
            .await
            .unwrap();
        assert!(cleared.incident.resolved);
        assert_eq!(resolvers(&db, v, incident).await, vec![svc]);

        // A service the invoice named keeps its stored link when the
        // list is sent back.
        let named = service_dated(&db, v, "2026-11-02").await;
        incident::update(&db, v, incident, resolved_by(vec![svc, named]))
            .await
            .unwrap();
        incident::update(&db, v, incident, resolved_by(vec![svc, named]))
            .await
            .unwrap();
        assert_eq!(stored_incident_links(&db).await, 1);
        assert_eq!(resolvers(&db, v, incident).await, vec![svc, named]);
    }

    #[tokio::test]
    async fn a_resolver_list_leaves_the_invoices_link_of_a_jobs_service_alone() {
        use crate::inputs::incident::UpdateIncident;
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let j = job_for(&db, v, incident).await;
        let svc = create(
            &db,
            v,
            NewServiceRecord {
                service_date: date("2026-11-01"),
                work_item_ids: Some(vec![j]),
                resolved_incident_ids: Some(vec![incident]),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .record
        .id;

        // The list leaves the job's service out and cannot remove it.
        let other = service_dated(&db, v, "2026-11-02").await;
        let only_other = UpdateIncident {
            resolved_by_service_ids: Some(vec![other]),
            ..Default::default()
        };
        incident::update(&db, v, incident, only_other)
            .await
            .unwrap();
        assert_eq!(resolvers(&db, v, incident).await, vec![svc, other]);

        // The invoice's naming holds when the job is taken off.
        update(&db, v, svc, jobs(vec![], Some(CompletedWork::DidNotHappen)))
            .await
            .unwrap();
        assert_eq!(resolvers(&db, v, incident).await, vec![svc, other]);
    }

    #[tokio::test]
    async fn a_resolver_list_sent_back_leaves_a_reopened_incident_unresolved() {
        use crate::inputs::incident::UpdateIncident;
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let j = job_for(&db, v, incident).await;
        let svc = completing(&db, v, vec![j]).await;
        let reopened = UpdateIncident {
            resolved: Some(false),
            ..Default::default()
        };
        incident::update(&db, v, incident, reopened).await.unwrap();

        let echoed = UpdateIncident {
            resolved_by_service_ids: Some(vec![svc]),
            ..Default::default()
        };
        let out = incident::update(&db, v, incident, echoed).await.unwrap();
        assert!(!out.incident.resolved);
        assert_eq!(stored_incident_links(&db).await, 0);
    }

    async fn resolve_by_hand(db: &DatabaseConnection, v: i32, incident: i32) {
        incident::update(
            db,
            v,
            incident,
            crate::inputs::incident::UpdateIncident {
                resolved: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    async fn by_hand(db: &DatabaseConnection, v: i32, incident: i32) -> bool {
        incident::get(db, v, incident)
            .await
            .unwrap()
            .incident
            .resolved_by_hand
    }

    #[tokio::test]
    async fn an_incident_resolved_by_hand_stays_resolved_when_a_correction_undoes_its_job() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        resolve_by_hand(&db, v, incident).await;
        let j = job_for(&db, v, incident).await;
        let svc = completing(&db, v, vec![j]).await;
        assert!(by_hand(&db, v, incident).await);

        // The job needs an answer, and the refusal names no incident.
        let err = update(&db, v, svc, jobs(vec![], None)).await.unwrap_err();
        assert!(!err.to_string().contains("incident"), "{err}");

        update(&db, v, svc, jobs(vec![], Some(CompletedWork::DidNotHappen)))
            .await
            .unwrap();
        assert_eq!(
            job::get(&db, v, j).await.unwrap().status,
            JobStatus::Planned
        );
        assert!(resolved(&db, v, incident).await);
        assert!(by_hand(&db, v, incident).await);
    }

    #[tokio::test]
    async fn an_incident_resolved_by_hand_stays_resolved_when_its_jobs_service_is_deleted() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        resolve_by_hand(&db, v, incident).await;
        let j = job_for(&db, v, incident).await;
        let svc = completing(&db, v, vec![j]).await;

        let err = delete(&db, v, svc, None).await.unwrap_err();
        assert!(!err.to_string().contains("incident"), "{err}");

        delete(&db, v, svc, Some(CompletedWork::DidNotHappen))
            .await
            .unwrap();
        assert!(resolved(&db, v, incident).await);
        assert!(by_hand(&db, v, incident).await);
    }

    #[tokio::test]
    async fn an_incident_a_job_resolved_still_reopens_and_carries_no_marker() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let j = job_for(&db, v, incident).await;
        let svc = completing(&db, v, vec![j]).await;
        assert!(!by_hand(&db, v, incident).await);

        // Sending `resolved: true` back changes nothing about who resolved it.
        resolve_by_hand(&db, v, incident).await;
        assert!(!by_hand(&db, v, incident).await);

        update(&db, v, svc, jobs(vec![], Some(CompletedWork::DidNotHappen)))
            .await
            .unwrap();
        assert!(!resolved(&db, v, incident).await);
    }

    #[tokio::test]
    async fn deleting_one_of_two_jobs_from_an_incident_stores_no_link() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let a = job_for(&db, v, incident).await;
        let b = job_for(&db, v, incident).await;
        let svc = completing(&db, v, vec![a, b]).await;

        job::delete(&db, v, a).await.unwrap();
        assert_eq!(stored_incident_links(&db).await, 0);
        assert_eq!(resolvers(&db, v, incident).await, vec![svc]);
    }

    #[tokio::test]
    async fn giving_a_completed_job_an_incident_resolves_that_incident() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let j = planned(&db, v, "Replace N80 valve", None).await;
        let svc = completing(&db, v, vec![j]).await;
        let incident = observation(&db, v, None).await;

        job::update(
            &db,
            v,
            j,
            crate::inputs::work_item::UpdateWorkItem {
                incident_id: Some(Some(incident)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(resolved(&db, v, incident).await);
        assert_eq!(resolvers(&db, v, incident).await, vec![svc]);
    }

    #[tokio::test]
    async fn deleting_a_completed_job_keeps_its_service_as_the_incidents_resolver() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let j = job_for(&db, v, incident).await;
        let svc = completing(&db, v, vec![j]).await;

        job::delete(&db, v, j).await.unwrap();
        assert!(resolved(&db, v, incident).await);
        assert_eq!(resolvers(&db, v, incident).await, vec![svc]);
    }

    #[tokio::test]
    async fn a_done_job_set_back_to_planned_stops_naming_its_service_as_resolver() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let j = job_for(&db, v, incident).await;
        let svc = completing(&db, v, vec![j]).await;

        let back = job::update(
            &db,
            v,
            j,
            crate::inputs::work_item::UpdateWorkItem {
                status: Some(JobStatus::Planned),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        // Only a correction of the service says whether the work happened.
        assert_eq!(back.released_incident_id, Some(incident));
        assert_eq!(back.released_from_service_id, Some(svc));
        assert!(resolved(&db, v, incident).await);
        crate::test_support::assert_empty(&resolvers(&db, v, incident).await);
    }

    #[tokio::test]
    async fn deleting_a_service_reopens_the_incident_only_its_job_resolved() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let j = job_for(&db, v, incident).await;
        let svc = completing(&db, v, vec![j]).await;

        delete(&db, v, svc, Some(CompletedWork::DidNotHappen))
            .await
            .unwrap();
        assert!(!resolved(&db, v, incident).await);
    }

    #[tokio::test]
    async fn a_wrong_id_is_not_found_before_any_answer_is_asked_for() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let j = planned(&db, v, "Pads", None).await;
        let svc = completing(&db, v, vec![j]).await;

        let err = update(&db, v, svc, jobs(vec![99_999], None))
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)), "{err:?}");
    }

    fn naming(ids: Vec<i32>, completed: Option<CompletedWork>) -> UpdateServiceRecord {
        UpdateServiceRecord {
            resolved_incident_ids: Some(ids),
            completed_work: completed,
            ..Default::default()
        }
    }

    // Each resolved incident of the service with whether its invoice
    // names it.
    async fn named(db: &DatabaseConnection, v: i32, svc: i32) -> Vec<(i32, Option<bool>)> {
        let read = get(db, v, svc).await.unwrap();
        read.resolved_incidents
            .iter()
            .map(|i| (i.id, i.named_by_invoice))
            .collect()
    }

    // An open shop observation dated before the 2026-11-01 services
    // these tests record.
    async fn observed(db: &DatabaseConnection, v: i32, on: &str) -> i32 {
        incident::create(
            db,
            v,
            NewIncident {
                category: IncidentCategory::ShopObservation,
                title: "Rear wiper streaks".into(),
                occurred_at: Some(DayOrTime::Day(date(on))),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .incident
        .id
    }

    fn open_ids(out: &ServiceRecordWithLinks) -> Vec<(i32, Vec<i32>)> {
        out.open_incidents
            .as_ref()
            .expect("the call lists open incidents")
            .iter()
            .map(|i| (i.id, i.planned_work_item_ids.clone()))
            .collect()
    }

    #[tokio::test]
    async fn a_recorded_service_lists_the_incidents_it_left_open() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let fixed = observed(&db, v, "2026-10-01").await;
        let fixed_job = job_for(&db, v, fixed).await;
        let waiting = observed(&db, v, "2026-10-02").await;
        let waiting_job = job_for(&db, v, waiting).await;
        let unplanned = observed(&db, v, "2026-11-02").await;
        // Left out: a note, an incident noticed during the service, one
        // that happened after its last day and another vehicle's.
        incident::create(
            &db,
            v,
            NewIncident {
                category: IncidentCategory::Note,
                title: "Prefers the dealer on Main".into(),
                occurred_at: Some(DayOrTime::Day(date("2026-10-01"))),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let noticed = observed(&db, v, "2026-11-01").await;
        observed(&db, v, "2026-11-03").await;
        let other = seed_vehicle(&db).await;
        observed(&db, other, "2026-10-01").await;

        let out = create(
            &db,
            v,
            NewServiceRecord {
                service_date: date("2026-11-01"),
                end_date: Some(date("2026-11-02")),
                work_item_ids: Some(vec![fixed_job]),
                noticed_incident_ids: Some(vec![noticed]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            open_ids(&out),
            vec![(unplanned, vec![]), (waiting, vec![waiting_job])]
        );

        // A read does not look, and the key is left out of its JSON.
        let read = get(&db, v, out.record.id).await.unwrap();
        assert!(read.open_incidents.is_none());
        assert!(
            serde_json::to_value(&read)
                .unwrap()
                .get("open_incidents")
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_correction_lists_open_incidents_only_when_it_changes_the_resolved_work() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observed(&db, v, "2026-10-01").await;
        let j = job_for(&db, v, incident).await;
        let svc = completing(&db, v, vec![]).await;

        let noted = update(
            &db,
            v,
            svc,
            UpdateServiceRecord {
                notes: Some(Some("Paid by card".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(noted.open_incidents.is_none());

        // A correction that sends a job list reports what is still open
        // after it, which here is nothing, as an empty list.
        let completed = update(&db, v, svc, jobs(vec![j], None)).await.unwrap();
        crate::test_support::assert_empty(&open_ids(&completed));
        assert_eq!(
            serde_json::to_value(&completed).unwrap()["open_incidents"],
            serde_json::json!([])
        );

        // An incident the owner reopened while its job stays completed
        // reads under `resolved_incidents` only.
        incident::update(
            &db,
            v,
            incident,
            crate::inputs::incident::UpdateIncident {
                resolved: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let reopened = update(&db, v, svc, jobs(vec![j], None)).await.unwrap();
        crate::test_support::assert_empty(&open_ids(&reopened));

        let undone = update(&db, v, svc, jobs(vec![], Some(CompletedWork::DidNotHappen)))
            .await
            .unwrap();
        assert_eq!(open_ids(&undone), vec![(incident, vec![j])]);
    }

    #[tokio::test]
    async fn a_repeated_invoice_number_does_not_look_for_open_incidents() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        observed(&db, v, "2026-10-01").await;
        let invoice = || NewServiceRecord {
            service_date: date("2026-11-01"),
            invoice_ref: Some("RO-1".into()),
            ..Default::default()
        };
        let first = create(&db, v, invoice()).await.unwrap();
        assert_eq!(open_ids(&first).len(), 1);
        let again = create(&db, v, invoice()).await.unwrap();
        assert!(again.already_recorded && again.open_incidents.is_none());
    }

    #[tokio::test]
    async fn naming_a_jobs_incident_keeps_it_resolved_when_the_job_is_taken_off() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let j = job_for(&db, v, incident).await;
        let svc = completing(&db, v, vec![j]).await;
        assert_eq!(named(&db, v, svc).await, vec![(incident, Some(false))]);

        // The service keeps the job, so the name needs no answer.
        update(&db, v, svc, naming(vec![incident], None))
            .await
            .unwrap();
        assert_eq!(stored_incident_links(&db).await, 1);
        assert_eq!(named(&db, v, svc).await, vec![(incident, Some(true))]);

        let out = update(&db, v, svc, jobs(vec![], Some(CompletedWork::DidNotHappen)))
            .await
            .unwrap();
        assert_eq!(out.incidents_kept_resolved, vec![incident]);
        assert!(resolved(&db, v, incident).await);
        assert_eq!(resolvers(&db, v, incident).await, vec![svc]);
    }

    #[tokio::test]
    async fn naming_an_open_incident_resolves_it() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let svc = completing(&db, v, vec![]).await;

        let out = update(&db, v, svc, naming(vec![incident], None))
            .await
            .unwrap();
        assert!(resolved(&db, v, incident).await);
        assert!(!by_hand(&db, v, incident).await);
        let fixed: Vec<i32> = out.resolved_incidents.iter().map(|i| i.id).collect();
        assert_eq!(fixed, vec![incident]);
        crate::test_support::assert_empty(&out.incidents_kept_resolved);
    }

    #[tokio::test]
    async fn a_name_taken_off_asks_for_an_answer_unless_a_job_still_brings_the_incident() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let svc = completing(&db, v, vec![]).await;
        update(&db, v, svc, naming(vec![incident], None))
            .await
            .unwrap();

        // No job of the service brings the incident, so taking the name
        // off takes the incident off.
        let err = update(&db, v, svc, naming(vec![], None)).await.unwrap_err();
        let DomainError::BadRequest(message) = err else {
            panic!("{err:?}");
        };
        assert!(
            message.contains(&format!("incident {incident}")),
            "{message}"
        );
        assert_eq!(stored_incident_links(&db).await, 1);

        update(
            &db,
            v,
            svc,
            naming(vec![], Some(CompletedWork::DidNotHappen)),
        )
        .await
        .unwrap();
        assert!(!resolved(&db, v, incident).await);
        assert_eq!(stored_incident_links(&db).await, 0);

        // With a job filed from the incident on the service, the name
        // comes off without an answer and the job still resolves it.
        let j = job_for(&db, v, incident).await;
        update(&db, v, svc, jobs(vec![j], None)).await.unwrap();
        update(&db, v, svc, naming(vec![incident], None))
            .await
            .unwrap();
        update(&db, v, svc, naming(vec![], None)).await.unwrap();
        assert_eq!(stored_incident_links(&db).await, 0);
        assert!(resolved(&db, v, incident).await);
        assert_eq!(named(&db, v, svc).await, vec![(incident, Some(false))]);
    }

    #[tokio::test]
    async fn a_jobs_incident_sent_back_with_the_call_that_takes_the_job_off_is_not_named() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let j = job_for(&db, v, incident).await;
        let svc = completing(&db, v, vec![j]).await;

        // The list repeats what a read of the service shows.
        let out = update(
            &db,
            v,
            svc,
            UpdateServiceRecord {
                work_item_ids: Some(vec![]),
                resolved_incident_ids: Some(vec![incident]),
                completed_work: Some(CompletedWork::DidNotHappen),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(stored_incident_links(&db).await, 0);
        assert!(!resolved(&db, v, incident).await);
        crate::test_support::assert_empty(&out.incidents_kept_resolved);
        assert_eq!(out.incident_names_ignored, vec![incident]);
    }

    #[tokio::test]
    async fn a_name_and_its_job_taken_off_together_release_the_incident() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let j = job_for(&db, v, incident).await;
        let svc = completing(&db, v, vec![j]).await;
        update(&db, v, svc, naming(vec![incident], None))
            .await
            .unwrap();

        let both = |completed_work| UpdateServiceRecord {
            work_item_ids: Some(vec![]),
            resolved_incident_ids: Some(vec![]),
            completed_work,
            ..Default::default()
        };
        let err = update(&db, v, svc, both(None)).await.unwrap_err();
        assert!(
            err.to_string().contains(&format!("incident {incident}")),
            "{err}"
        );

        let out = update(&db, v, svc, both(Some(CompletedWork::DidNotHappen)))
            .await
            .unwrap();
        crate::test_support::assert_empty(&out.incidents_kept_resolved);
        assert!(!resolved(&db, v, incident).await);
        crate::test_support::assert_empty(&resolvers(&db, v, incident).await);
    }

    #[tokio::test]
    async fn a_name_taken_off_as_done_another_way_leaves_the_incident_resolved_by_hand() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let svc = completing(&db, v, vec![]).await;
        update(&db, v, svc, naming(vec![incident], None))
            .await
            .unwrap();

        update(
            &db,
            v,
            svc,
            naming(vec![], Some(CompletedWork::DoneAnotherWay)),
        )
        .await
        .unwrap();
        assert!(resolved(&db, v, incident).await);
        assert!(by_hand(&db, v, incident).await);
        crate::test_support::assert_empty(&resolvers(&db, v, incident).await);
    }

    #[tokio::test]
    async fn a_name_taken_off_an_incident_resolved_by_hand_asks_nothing() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        resolve_by_hand(&db, v, incident).await;
        let svc = completing(&db, v, vec![]).await;
        update(&db, v, svc, naming(vec![incident], None))
            .await
            .unwrap();

        update(&db, v, svc, naming(vec![], None)).await.unwrap();
        assert!(resolved(&db, v, incident).await);
        assert!(by_hand(&db, v, incident).await);
    }

    // The incident still reads resolved, by `by` alone and not on the
    // owner's word.
    async fn assert_resolved_only_by(db: &DatabaseConnection, v: i32, incident: i32, by: i32) {
        assert!(resolved(db, v, incident).await);
        assert!(!by_hand(db, v, incident).await);
        assert_eq!(resolvers(db, v, incident).await, vec![by]);
    }

    #[tokio::test]
    async fn a_name_taken_off_an_incident_another_service_resolves_asks_nothing() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let j = job_for(&db, v, incident).await;
        let first = completing(&db, v, vec![j]).await;
        let second = completing(&db, v, vec![]).await;
        update(&db, v, second, naming(vec![incident], None))
            .await
            .unwrap();

        update(&db, v, second, naming(vec![], None)).await.unwrap();
        assert_resolved_only_by(&db, v, incident, first).await;
    }

    #[tokio::test]
    async fn a_job_taken_off_whose_incident_another_service_resolves_asks_only_about_the_job() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let kept = job_for(&db, v, incident).await;
        let first = completing(&db, v, vec![kept]).await;
        let taken_off = job_for(&db, v, incident).await;
        let second = completing(&db, v, vec![taken_off]).await;

        let err = update(&db, v, second, jobs(vec![], None))
            .await
            .unwrap_err();
        let message = err.to_string();
        assert!(message.contains(&format!("job {taken_off}")), "{message}");
        assert!(
            !message.contains(&format!("incident {incident}")),
            "{message}"
        );

        update(
            &db,
            v,
            second,
            jobs(vec![], Some(CompletedWork::DidNotHappen)),
        )
        .await
        .unwrap();
        assert_resolved_only_by(&db, v, incident, first).await;
    }

    #[tokio::test]
    async fn deleting_a_service_that_names_an_incident_another_service_resolves_asks_nothing() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident = observation(&db, v, None).await;
        let j = job_for(&db, v, incident).await;
        let first = completing(&db, v, vec![j]).await;
        let second = completing(&db, v, vec![]).await;
        update(&db, v, second, naming(vec![incident], None))
            .await
            .unwrap();

        delete(&db, v, second, None).await.unwrap();
        assert_resolved_only_by(&db, v, incident, first).await;
    }

    #[tokio::test]
    async fn naming_another_vehicles_incident_is_not_found_and_stores_nothing() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let theirs = observation(&db, other, None).await;
        let svc = completing(&db, v, vec![]).await;

        let wrong = update(&db, v, svc, naming(vec![theirs], None))
            .await
            .unwrap_err();
        let missing = update(&db, v, svc, naming(vec![theirs + 1000], None))
            .await
            .unwrap_err();
        assert!(matches!(wrong, DomainError::NotFound(_)), "{wrong:?}");
        assert_eq!(
            wrong.to_string().replace(&theirs.to_string(), "N"),
            missing
                .to_string()
                .replace(&(theirs + 1000).to_string(), "N")
        );
        assert_eq!(stored_incident_links(&db).await, 0);
        assert!(!resolved(&db, other, theirs).await);
    }
}

async fn listed_descriptions(
    db: &DatabaseConnection,
    vehicle_id: i32,
    filter: ServiceFilter,
) -> Vec<String> {
    list_filtered(db, vehicle_id, filter)
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.record.description.unwrap_or_default())
        .collect()
}

// The fixture holds four services, two at each shop, and the second and
// third by date carry a repair line.
async fn seed_filterable_services(db: &DatabaseConnection) -> (i32, i32, i32) {
    let v = seed_vehicle(db).await;
    let dealer = crate::test_support::add_shop(db, "Dealer").await;
    let indie = crate::test_support::add_shop(db, "Indie").await;
    for (day, description, shop, lines) in [
        (
            "2026-01-10",
            "Oil change",
            dealer,
            vec![line("Oil", WorkKind::Maintenance)],
        ),
        (
            "2026-03-10",
            "Coil pack",
            indie,
            vec![line("Coil", WorkKind::Repair)],
        ),
        (
            "2026-05-10",
            "Brakes and a hose",
            dealer,
            vec![
                line("Pads", WorkKind::Maintenance),
                line("Hose", WorkKind::Repair),
            ],
        ),
        ("2026-07-10", "Inspection", indie, Vec::new()),
    ] {
        let input = NewServiceRecord {
            service_date: date(day),
            description: Some(description.into()),
            shop: Some(ShopChoice::Id(shop)),
            line_items: Some(lines),
            ..minimal_record(None)
        };
        create(db, v, input).await.unwrap();
    }
    (v, dealer, indie)
}

#[tokio::test]
async fn list_filtered_keeps_a_shop_a_work_kind_and_a_date_range() {
    let db = test_db().await;
    let (v, dealer, _) = seed_filterable_services(&db).await;

    let all = listed_descriptions(&db, v, ServiceFilter::default()).await;
    assert_eq!(
        all,
        ["Inspection", "Brakes and a hose", "Coil pack", "Oil change"]
    );

    let at_dealer = ServiceFilter {
        shop_id: Some(dealer),
        ..Default::default()
    };
    assert_eq!(
        listed_descriptions(&db, v, at_dealer).await,
        ["Brakes and a hose", "Oil change"]
    );

    // A service matches when any one of its lines has the kind.
    let repairs = ServiceFilter {
        work_kind: Some(WorkKind::Repair),
        ..Default::default()
    };
    assert_eq!(
        listed_descriptions(&db, v, repairs).await,
        ["Brakes and a hose", "Coil pack"]
    );

    let dealer_repairs_in_spring = ServiceFilter {
        from: Some(date("2026-03-10")),
        to: Some(date("2026-05-10")),
        shop_id: Some(dealer),
        work_kind: Some(WorkKind::Repair),
        limit: None,
    };
    assert_eq!(
        listed_descriptions(&db, v, dealer_repairs_in_spring).await,
        ["Brakes and a hose"]
    );

    let recalls = ServiceFilter {
        work_kind: Some(WorkKind::Recall),
        ..Default::default()
    };
    crate::test_support::assert_empty(&listed_descriptions(&db, v, recalls).await);
}

#[tokio::test]
async fn list_filtered_limit_keeps_the_newest_and_each_keeps_its_own_lines() {
    let db = test_db().await;
    let (v, ..) = seed_filterable_services(&db).await;
    let newest = ServiceFilter {
        limit: Some(2),
        ..Default::default()
    };
    let listed = list_filtered(&db, v, newest).await.unwrap();
    let described: Vec<_> = listed
        .iter()
        .map(|s| (s.record.description.as_deref().unwrap(), s.line_items.len()))
        .collect();
    assert_eq!(described, [("Inspection", 0), ("Brakes and a hose", 2)]);

    // The limit counts the services the other filters keep.
    let newest_repair = ServiceFilter {
        work_kind: Some(WorkKind::Repair),
        limit: Some(1),
        ..Default::default()
    };
    assert_eq!(
        listed_descriptions(&db, v, newest_repair).await,
        ["Brakes and a hose"]
    );
}

#[tokio::test]
async fn list_filtered_by_a_missing_shop_is_not_found() {
    let db = test_db().await;
    let (v, ..) = seed_filterable_services(&db).await;
    let filter = ServiceFilter {
        shop_id: Some(9999),
        ..Default::default()
    };
    let err = list_filtered(&db, v, filter).await.err().unwrap();
    assert_eq!(err.to_string(), "Shop 9999 not found");
}

#[tokio::test]
async fn list_filtered_refuses_a_limit_of_zero() {
    let db = test_db().await;
    let (v, ..) = seed_filterable_services(&db).await;
    let filter = ServiceFilter {
        limit: Some(0),
        ..Default::default()
    };
    let err = list_filtered(&db, v, filter).await.err().unwrap();
    assert_eq!(err.to_string(), "limit: must be at least 1");
}
