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
    let err = NewFitting::from_fields(Some(PartStatus::Returned), Install::default()).unwrap_err();
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
        let err = Fitting::from_fields(Some(PartStatus::Purchased), install, replace).unwrap_err();
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
            "Part {} is installed, so it takes no replacement details; set its status to replaced \
             with them",
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
                "Part {} was replaced; clear its replacement date and odometer in the same change \
                 to set it back to installed",
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
        "received_on: is before the purchase date 2026-11-05; correct the purchase date first if \
         it is wrong"
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
        "received_on: is before the purchase date 2026-11-06; correct the purchase date first if \
         it is wrong"
    );
    assert_eq!(
        get(&db, vid, p.id).await.unwrap().purchase_date,
        Some(date("2026-11-05"))
    );
}
