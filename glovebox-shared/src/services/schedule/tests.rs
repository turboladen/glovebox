use super::*;
use crate::{
    services::reminders::DueStatus,
    test_support::{VehicleFixture, days_ago, test_db},
};

// Reads the schedule items the vehicle itself owns, overrides included.
async fn own_items(db: &impl ConnectionTrait, vid: i32) -> Vec<maintenance_schedule_item::Model> {
    maintenance_schedule_item::Entity::find()
        .filter(maintenance_schedule_item::Column::VehicleId.eq(vid))
        .all(db)
        .await
        .unwrap()
}

async fn seed_vehicle(db: &impl ConnectionTrait, model_template_id: Option<i32>) -> i32 {
    let mut fixture = VehicleFixture::new();
    if let Some(id) = model_template_id {
        fixture = fixture.model_template_id(id);
    }
    fixture.insert_id(db).await
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
            est_cost_cents: None,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DomainError::Invalid { .. }));
}

#[tokio::test]
async fn only_a_vehicle_item_overrides_another() {
    let db = test_db().await;
    let err = create(
        &db,
        NewScheduleItem {
            platform_id: Some(1),
            overrides_item_id: Some(1),
            name: "Oil change".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "overrides_item_id"));
}

#[tokio::test]
async fn a_new_item_without_warning_values_gets_the_default_window() {
    use crate::{
        entities::{service_record, service_schedule_link},
        services::reminders::calculate_reminders,
    };
    let db = test_db().await;
    let vid = seed_vehicle(&db, None).await;
    let created = create(
        &db,
        NewScheduleItem {
            vehicle_id: Some(vid),
            interval_miles: None,
            interval_months: Some(24),
            ..item_named("Smog check")
        },
    )
    .await
    .unwrap();
    let read_back = get(&db, created.id).await.unwrap();
    assert_eq!(read_back.warning_miles, Some(1000));
    assert_eq!(read_back.warning_days, Some(30));

    // Done 23.5 months (715 days) ago: due in about 15 days, inside the window.
    let svc = service_record::ActiveModel {
        vehicle_id: Set(vid),
        service_date: Set(days_ago(715)),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    service_schedule_link::ActiveModel {
        service_record_id: Set(svc.id),
        schedule_item_id: Set(created.id),
    }
    .insert(&db)
    .await
    .unwrap();
    let resp = calculate_reminders(&db, vid).await.unwrap();
    assert_eq!(resp.reminders[0].status, DueStatus::DueSoon);
}

#[tokio::test]
async fn explicit_warning_values_are_kept_and_clearing_restores_the_default() {
    let db = test_db().await;
    let vid = seed_vehicle(&db, None).await;
    let created = create(
        &db,
        NewScheduleItem {
            vehicle_id: Some(vid),
            warning_miles: Some(500),
            warning_days: Some(14),
            ..item_named("Brake fluid")
        },
    )
    .await
    .unwrap();
    assert_eq!(created.warning_miles, Some(500));
    assert_eq!(created.warning_days, Some(14));

    let cleared = update(
        &db,
        created.id,
        UpdateScheduleItem {
            warning_miles: Some(None),
            warning_days: Some(None),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(cleared.warning_miles, Some(1000));
    assert_eq!(cleared.warning_days, Some(30));
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
            est_cost_cents: None,
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
            est_cost_cents: None,
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
            est_cost_cents: None,
        },
    )
    .await
    .unwrap();

    let resolved = resolve(&db, vid).await.unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].effective_item.name, "Tire rotation");
    assert_eq!(resolved[0].inherited_from, None);
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
        est_cost_cents: None,
    }
}

// Platform → template → vehicle chain; returns (`platform_id`,
// `template_id`, `vehicle_id`).
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

    let dismissed = dismiss_for_vehicle(&db, vid, item.id, Some("switched to synthetic".into()))
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

    let vehicle_rows = own_items(&db, vid).await;
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

async fn service_on(db: &DatabaseConnection, vehicle_id: i32, item_id: i32) -> i32 {
    crate::services::service_record::create(
        db,
        vehicle_id,
        crate::inputs::service_record::NewServiceRecord {
            service_date: days_ago(30),
            schedule_item_ids: Some(vec![item_id]),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .record
    .id
}

fn reminder_for<'a>(
    rems: &'a crate::services::reminders::RemindersResponse,
    name: &str,
) -> &'a crate::services::reminders::ReminderStatus {
    rems.reminders
        .iter()
        .find(|r| r.schedule_item.name == name)
        .unwrap_or_else(|| panic!("no reminder named {name}"))
}

#[tokio::test]
async fn changing_an_inherited_item_keeps_its_history_and_planned_work() {
    use crate::services::{budget, reminders, work_item};
    let db = test_db().await;
    let (_pid, template_id, vid) = seed_chain(&db).await;
    let inherited = create(
        &db,
        NewScheduleItem {
            model_template_id: Some(template_id),
            interval_miles: None,
            interval_months: Some(12),
            ..item_named("Spark Plugs")
        },
    )
    .await
    .unwrap();
    let svc = service_on(&db, vid, inherited.id).await;
    let planned = work_item::create(
        &db,
        vid,
        crate::inputs::work_item::NewWorkItem {
            title: "Plugs".into(),
            schedule_item_id: Some(inherited.id),
            est_cost_cents: Some(9_000),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let over = update_for_vehicle(
        &db,
        vid,
        inherited.id,
        UpdateScheduleItem {
            est_cost_cents: Some(Some(12_000)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_ne!(over.id, inherited.id, "the shared item is never edited");
    assert_eq!(over.vehicle_id, Some(vid));
    assert_eq!(over.overrides_item_id, Some(inherited.id));
    assert_eq!(over.interval_months, Some(12), "definition copied");
    assert_eq!(over.est_cost_cents, Some(12_000));
    assert_eq!(get(&db, inherited.id).await.unwrap().est_cost_cents, None);

    let rems = reminders::calculate_reminders(&db, vid).await.unwrap();
    let plugs = reminder_for(&rems, "Spark Plugs");
    assert_eq!(plugs.schedule_item.id, over.id);
    assert_eq!(plugs.status, DueStatus::NotDue);
    assert_eq!(plugs.last_service.as_ref().map(|s| s.id), Some(svc));

    // The planned work names the inherited row and counts toward the
    // override, so it still stands in for the first projected occurrence.
    let kept = work_item::get(&db, vid, planned.id).await.unwrap();
    assert_eq!(kept.schedule_item_id, Some(inherited.id));
    let today = crate::services::today();
    let backlog = crate::services::upcoming::planned_work(&db, vid, today)
        .await
        .unwrap();
    let f = budget::forecast_from(&rems, &backlog, today);
    assert!(
        !f.lines.iter().any(|l| l.label == "Spark Plugs"),
        "first occurrence is already planned: {:?}",
        f.lines
    );

    // Calling again, on either id, edits the same override.
    for id in [inherited.id, over.id] {
        let again = update_for_vehicle(
            &db,
            vid,
            id,
            UpdateScheduleItem {
                notes: Some(Some(format!("edit via {id}"))),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(again.id, over.id);
    }
    let own_rows = own_items(&db, vid).await;
    assert_eq!(own_rows.len(), 1, "one override, never two");
}

#[tokio::test]
async fn overriding_an_item_on_one_car_leaves_the_other_cars_links_alone() {
    use crate::services::reminders;
    let db = test_db().await;
    let (_pid, template_id, first) = seed_chain(&db).await;
    let second = seed_vehicle(&db, Some(template_id)).await;
    let inherited = create(
        &db,
        NewScheduleItem {
            model_template_id: Some(template_id),
            interval_miles: None,
            interval_months: Some(12),
            ..item_named("Spark Plugs")
        },
    )
    .await
    .unwrap();
    service_on(&db, first, inherited.id).await;
    let second_svc = service_on(&db, second, inherited.id).await;

    update_for_vehicle(
        &db,
        first,
        inherited.id,
        UpdateScheduleItem {
            est_cost_cents: Some(Some(12_000)),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let rems = reminders::calculate_reminders(&db, second).await.unwrap();
    let plugs = reminder_for(&rems, "Spark Plugs");
    assert_eq!(plugs.schedule_item.id, inherited.id);
    assert_eq!(plugs.last_service.as_ref().map(|s| s.id), Some(second_svc));
}

#[tokio::test]
async fn adding_an_item_that_shadows_an_inherited_one_keeps_its_history() {
    use crate::services::reminders;
    let db = test_db().await;
    let (_pid, template_id, vid) = seed_chain(&db).await;
    let inherited = create(
        &db,
        NewScheduleItem {
            model_template_id: Some(template_id),
            interval_miles: None,
            interval_months: Some(12),
            ..item_named("Spark Plugs")
        },
    )
    .await
    .unwrap();
    let svc = service_on(&db, vid, inherited.id).await;

    let own = create_for_vehicle(
        &db,
        vid,
        NewScheduleItem {
            interval_months: Some(24),
            ..item_named("Spark Plugs")
        },
    )
    .await
    .unwrap();
    assert_eq!(own.overrides_item_id, Some(inherited.id));

    let rems = reminders::calculate_reminders(&db, vid).await.unwrap();
    let plugs = reminder_for(&rems, "Spark Plugs");
    assert_eq!(plugs.schedule_item.id, own.id);
    assert_eq!(plugs.last_service.as_ref().map(|s| s.id), Some(svc));

    // A second item under the same name would make the schedule ambiguous.
    let err = create_for_vehicle(&db, vid, item_named("Spark Plugs"))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, DomainError::BadRequest(m) if m.contains("update_schedule_item")),
        "got: {err}"
    );
}

#[tokio::test]
async fn an_inherited_item_keeps_its_name() {
    let db = test_db().await;
    let (_pid, template_id, vid) = seed_chain(&db).await;
    let inherited = create(
        &db,
        NewScheduleItem {
            model_template_id: Some(template_id),
            ..item_named("Spark Plugs")
        },
    )
    .await
    .unwrap();
    let err = update_for_vehicle(
        &db,
        vid,
        inherited.id,
        UpdateScheduleItem {
            name: Some("Plugs".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DomainError::BadRequest(_)), "got: {err}");
    // A refused change leaves no override behind.
    crate::test_support::assert_empty(&own_items(&db, vid).await);
}

#[tokio::test]
async fn dismissing_and_restoring_an_inherited_item_keeps_its_history() {
    use crate::services::reminders;
    let db = test_db().await;
    let (_pid, template_id, vid) = seed_chain(&db).await;
    let inherited = create(
        &db,
        NewScheduleItem {
            model_template_id: Some(template_id),
            interval_miles: None,
            interval_months: Some(12),
            ..item_named("Spark Plugs")
        },
    )
    .await
    .unwrap();
    let svc = service_on(&db, vid, inherited.id).await;
    let shadow = dismiss_for_vehicle(&db, vid, inherited.id, None)
        .await
        .unwrap();
    undismiss_for_vehicle(&db, vid, shadow.id).await.unwrap();

    let rems = reminders::calculate_reminders(&db, vid).await.unwrap();
    let plugs = reminder_for(&rems, "Spark Plugs");
    assert_eq!(plugs.last_service.as_ref().map(|s| s.id), Some(svc));
}

#[tokio::test]
async fn history_follows_a_vehicle_to_another_template_with_an_item_of_the_same_name() {
    use crate::services::reminders;
    let db = test_db().await;
    let (pid, template_id, vid) = seed_chain(&db).await;
    let old = create(
        &db,
        NewScheduleItem {
            model_template_id: Some(template_id),
            interval_months: Some(12),
            ..item_named("Spark Plugs")
        },
    )
    .await
    .unwrap();
    let svc = service_on(&db, vid, old.id).await;

    let other_template = model_template::ActiveModel {
        platform_id: Set(Some(pid)),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;
    let new = create(
        &db,
        NewScheduleItem {
            model_template_id: Some(other_template),
            interval_months: Some(12),
            ..item_named("spark plugs")
        },
    )
    .await
    .unwrap();
    crate::services::vehicle::update(
        &db,
        vid,
        crate::inputs::vehicle::UpdateVehicle {
            model_template_id: Some(Some(other_template)),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // The link names the first template's row, and the service counts
    // toward the item the vehicle tracks under that name.
    let rems = reminders::calculate_reminders(&db, vid).await.unwrap();
    let plugs = reminder_for(&rems, "spark plugs");
    assert_eq!(plugs.schedule_item.id, new.id);
    assert_eq!(plugs.last_service.as_ref().map(|s| s.id), Some(svc));
    let read = crate::services::service_record::get(&db, vid, svc)
        .await
        .unwrap();
    assert_eq!(read.schedule_item_ids, vec![new.id]);
}

#[tokio::test]
async fn a_link_to_the_inherited_row_follows_a_job_on_the_cars_own_item() {
    use crate::{
        inputs::service_record::UpdateServiceRecord,
        services::{service_record, work_item},
    };
    let db = test_db().await;
    let (_pid, template_id, vid) = seed_chain(&db).await;
    let inherited = create(
        &db,
        NewScheduleItem {
            model_template_id: Some(template_id),
            ..item_named("Spark Plugs")
        },
    )
    .await
    .unwrap();
    // The invoice names the item while the vehicle still tracks the
    // inherited row, and the vehicle gets its own copy afterwards.
    let svc = service_on(&db, vid, inherited.id).await;
    let over = update_for_vehicle(
        &db,
        vid,
        inherited.id,
        UpdateScheduleItem {
            est_cost_cents: Some(Some(1)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let job = work_item::create(
        &db,
        vid,
        crate::inputs::work_item::NewWorkItem {
            title: "Plugs".into(),
            schedule_item_id: Some(over.id),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Completing the job drops the stored link that counts toward the
    // same item, so the item goes with the job when it is taken off.
    let done = service_record::update(
        &db,
        vid,
        svc,
        UpdateServiceRecord {
            work_item_ids: Some(vec![job.id]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(done.schedule_item_ids, vec![over.id]);
    let off = service_record::update(
        &db,
        vid,
        svc,
        UpdateServiceRecord {
            work_item_ids: Some(vec![]),
            completed_work: Some(service_record::CompletedWork::DidNotHappen),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    crate::test_support::assert_empty(&off.schedule_item_ids);
    assert_eq!(off.released_schedule_item_ids, vec![over.id]);
}

// An inherited "Spark Plugs", a job planned on it, and then the vehicle's
// own copy, so the job names a row the vehicle does not track.
async fn job_on_a_row_the_copy_replaced(db: &DatabaseConnection) -> (i32, i32, i32, i32) {
    let (_pid, template_id, vid) = seed_chain(db).await;
    let inherited = create(
        db,
        NewScheduleItem {
            model_template_id: Some(template_id),
            ..item_named("Spark Plugs")
        },
    )
    .await
    .unwrap();
    let job = crate::services::work_item::create(
        db,
        vid,
        crate::inputs::work_item::NewWorkItem {
            title: "Plugs".into(),
            schedule_item_id: Some(inherited.id),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let over = update_for_vehicle(
        db,
        vid,
        inherited.id,
        UpdateScheduleItem {
            est_cost_cents: Some(Some(1)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    (vid, inherited.id, over.id, job.id)
}

#[tokio::test]
async fn the_planned_work_list_names_the_item_each_job_counts_toward() {
    use crate::services::work_item;
    let db = test_db().await;
    let (vid, inherited, over, job) = job_on_a_row_the_copy_replaced(&db).await;

    let stored = work_item::get(&db, vid, job).await.unwrap();
    assert_eq!(stored.schedule_item_id, Some(inherited));
    let listed = work_item::list_with_status(&db, vid, false, crate::services::today())
        .await
        .unwrap();
    assert_eq!(listed[0].item.schedule_item_id, Some(over));
}

#[tokio::test]
async fn a_job_on_a_replaced_row_releases_and_shields_the_tracked_item() {
    use crate::{
        inputs::service_record::{NewServiceRecord, UpdateServiceRecord},
        services::service_record,
    };
    let db = test_db().await;
    let (vid, inherited, over, job) = job_on_a_row_the_copy_replaced(&db).await;
    let svc = service_record::create(
        &db,
        vid,
        NewServiceRecord {
            service_date: days_ago(1),
            work_item_ids: Some(vec![job]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(svc.schedule_item_ids, vec![over]);

    // A list that names the item by its inherited id, sent with the call
    // that takes the job off, stores nothing for it.
    let off = service_record::update(
        &db,
        vid,
        svc.record.id,
        UpdateServiceRecord {
            work_item_ids: Some(vec![]),
            schedule_item_ids: Some(vec![inherited]),
            completed_work: Some(service_record::CompletedWork::DoneAnotherWay),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    crate::test_support::assert_empty(&off.schedule_item_ids);
    assert_eq!(off.released_schedule_item_ids, vec![over]);
}

#[tokio::test]
async fn a_job_edit_names_the_tracked_item_it_released() {
    use crate::{
        entities::work_item::JobStatus,
        inputs::{service_record::NewServiceRecord, work_item::UpdateWorkItem},
        services::{service_record, work_item},
    };
    let db = test_db().await;
    let (vid, _inherited, over, job) = job_on_a_row_the_copy_replaced(&db).await;
    service_record::create(
        &db,
        vid,
        NewServiceRecord {
            service_date: days_ago(1),
            work_item_ids: Some(vec![job]),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let back = work_item::update(
        &db,
        vid,
        job,
        UpdateWorkItem {
            status: Some(JobStatus::Planned),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(back.released_schedule_item_id, Some(over));
}

#[tokio::test]
async fn an_item_renamed_onto_an_inherited_name_drops_the_link_its_job_repeats() {
    use crate::{
        inputs::service_record::UpdateServiceRecord,
        services::{service_record, work_item},
    };
    let db = test_db().await;
    let (_pid, template_id, vid) = seed_chain(&db).await;
    let inherited = create(
        &db,
        NewScheduleItem {
            model_template_id: Some(template_id),
            ..item_named("Spark Plugs")
        },
    )
    .await
    .unwrap();
    let own = create_for_vehicle(&db, vid, item_named("Plugs"))
        .await
        .unwrap();
    let svc = service_on(&db, vid, inherited.id).await;
    let job = work_item::create(
        &db,
        vid,
        crate::inputs::work_item::NewWorkItem {
            title: "Plugs".into(),
            schedule_item_id: Some(own.id),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let complete = |ids: Vec<i32>, completed_work| UpdateServiceRecord {
        work_item_ids: Some(ids),
        completed_work,
        ..Default::default()
    };
    service_record::update(&db, vid, svc, complete(vec![job.id], None))
        .await
        .unwrap();

    update_for_vehicle(
        &db,
        vid,
        own.id,
        UpdateScheduleItem {
            name: Some("Spark Plugs".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // The link and the job count toward one item, so the item goes with
    // the job.
    let off = service_record::update(
        &db,
        vid,
        svc,
        complete(vec![], Some(service_record::CompletedWork::DidNotHappen)),
    )
    .await
    .unwrap();
    assert_eq!(off.released_schedule_item_ids, vec![own.id]);
    crate::test_support::assert_empty(&off.schedule_item_ids);
}

#[tokio::test]
async fn links_sent_with_a_shadowed_id_land_on_the_cars_own_item() {
    let db = test_db().await;
    let (_pid, template_id, vid) = seed_chain(&db).await;
    let inherited = create(
        &db,
        NewScheduleItem {
            model_template_id: Some(template_id),
            ..item_named("Spark Plugs")
        },
    )
    .await
    .unwrap();
    let over = update_for_vehicle(
        &db,
        vid,
        inherited.id,
        UpdateScheduleItem {
            est_cost_cents: Some(Some(1)),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let record = crate::services::service_record::create(
        &db,
        vid,
        crate::inputs::service_record::NewServiceRecord {
            service_date: days_ago(1),
            schedule_item_ids: Some(vec![inherited.id, over.id]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(record.schedule_item_ids, vec![over.id]);
}

#[tokio::test]
async fn update_for_vehicle_edits_own_items_only() {
    let db = test_db().await;
    let template_id = crate::services::model_template::list(&db).await.unwrap()[0].id;
    let vid = seed_vehicle(&db, Some(template_id)).await;
    let other = seed_vehicle(&db, None).await;

    let own = create_for_vehicle(
        &db,
        vid,
        NewScheduleItem {
            name: "Smog check".into(),
            interval_months: Some(24),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let updated = update_for_vehicle(
        &db,
        vid,
        own.id,
        UpdateScheduleItem {
            est_cost_cents: Some(Some(5800)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.est_cost_cents, Some(5800));

    let shared = create(
        &db,
        NewScheduleItem {
            model_template_id: Some(template_id),
            name: "Shared item".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let over = update_for_vehicle(&db, vid, shared.id, UpdateScheduleItem::default())
        .await
        .unwrap();
    assert_eq!(over.overrides_item_id, Some(shared.id));
    assert_eq!(over.vehicle_id, Some(vid));

    let foreign = update_for_vehicle(&db, other, own.id, UpdateScheduleItem::default())
        .await
        .unwrap_err();
    let missing = update_for_vehicle(&db, other, 99_999, UpdateScheduleItem::default())
        .await
        .unwrap_err();
    assert_eq!(
        foreign.to_string().replace(&own.id.to_string(), "N"),
        missing.to_string().replace("99999", "N")
    );
}

#[tokio::test]
async fn an_interval_of_zero_or_less_is_refused_and_nothing_is_stored() {
    let db = test_db().await;
    let (_pid, template_id, vid) = seed_chain(&db).await;
    for (miles, months, field) in [
        (Some(0), None, "interval_miles"),
        (Some(-5), None, "interval_miles"),
        (None, Some(0), "interval_months"),
    ] {
        let err = create_for_vehicle(
            &db,
            vid,
            NewScheduleItem {
                interval_miles: miles,
                interval_months: months,
                ..item_named("Smog check")
            },
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&err, DomainError::Invalid { field: f, .. } if f == field),
            "got: {err}"
        );
    }

    // A refused change to an inherited item leaves no copy behind.
    let inherited = create(
        &db,
        NewScheduleItem {
            model_template_id: Some(template_id),
            ..item_named("Spark Plugs")
        },
    )
    .await
    .unwrap();
    let err = update_for_vehicle(
        &db,
        vid,
        inherited.id,
        UpdateScheduleItem {
            interval_months: Some(Some(0)),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DomainError::Invalid { .. }), "got: {err}");
    crate::test_support::assert_empty(&own_items(&db, vid).await);
}

#[tokio::test]
async fn a_negative_warning_is_refused_and_zero_is_accepted() {
    let db = test_db().await;
    let (_pid, template_id, vid) = seed_chain(&db).await;
    for (miles, days, field) in [
        (Some(-1), None, "warning_miles"),
        (None, Some(-1), "warning_days"),
    ] {
        let err = create_for_vehicle(
            &db,
            vid,
            NewScheduleItem {
                warning_miles: miles,
                warning_days: days,
                ..item_named("Smog check")
            },
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&err, DomainError::Invalid { field: f, .. } if f == field),
            "got: {err}"
        );
    }

    // A refused change to an inherited item leaves no copy behind.
    let inherited = create(
        &db,
        NewScheduleItem {
            model_template_id: Some(template_id),
            ..item_named("Spark Plugs")
        },
    )
    .await
    .unwrap();
    for (miles, days, field) in [
        (Some(Some(-1)), None, "warning_miles"),
        (None, Some(Some(-1)), "warning_days"),
    ] {
        let err = update_for_vehicle(
            &db,
            vid,
            inherited.id,
            UpdateScheduleItem {
                warning_miles: miles,
                warning_days: days,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&err, DomainError::Invalid { field: f, .. } if f == field),
            "got: {err}"
        );
    }
    crate::test_support::assert_empty(&own_items(&db, vid).await);

    let zero = create_for_vehicle(
        &db,
        vid,
        NewScheduleItem {
            warning_miles: Some(0),
            warning_days: Some(0),
            ..item_named("Smog check")
        },
    )
    .await
    .unwrap();
    assert_eq!((zero.warning_miles, zero.warning_days), (Some(0), Some(0)));
    let defaults = create_for_vehicle(&db, vid, item_named("Wipers"))
        .await
        .unwrap();
    let updated = update_for_vehicle(
        &db,
        vid,
        defaults.id,
        UpdateScheduleItem {
            warning_miles: Some(Some(0)),
            warning_days: Some(Some(0)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        (updated.warning_miles, updated.warning_days),
        (Some(0), Some(0))
    );
}

#[tokio::test]
async fn an_item_renamed_onto_an_inherited_name_takes_its_place_and_its_history() {
    use crate::services::reminders;
    let db = test_db().await;
    let (_pid, template_id, vid) = seed_chain(&db).await;
    let inherited = create(
        &db,
        NewScheduleItem {
            model_template_id: Some(template_id),
            ..item_named("Spark Plugs")
        },
    )
    .await
    .unwrap();
    let svc = service_on(&db, vid, inherited.id).await;
    let own = create_for_vehicle(&db, vid, item_named("Plugs"))
        .await
        .unwrap();

    let renamed = update_for_vehicle(
        &db,
        vid,
        own.id,
        UpdateScheduleItem {
            name: Some("Spark Plugs".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(renamed.overrides_item_id, Some(inherited.id));
    let rems = reminders::calculate_reminders(&db, vid).await.unwrap();
    let plugs = reminder_for(&rems, "Spark Plugs");
    assert_eq!(plugs.schedule_item.id, own.id);
    assert_eq!(plugs.last_service.as_ref().map(|s| s.id), Some(svc));

    // Renaming it away again would strand that history on a name the
    // inherited item takes back, so it keeps the name.
    let err = update_for_vehicle(
        &db,
        vid,
        own.id,
        UpdateScheduleItem {
            name: Some("Plugs".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DomainError::BadRequest(_)), "got: {err}");
}

#[tokio::test]
async fn an_item_added_in_other_capitalization_replaces_the_inherited_one_and_keeps_its_history() {
    use crate::services::reminders;
    let db = test_db().await;
    let (_pid, template_id, vid) = seed_chain(&db).await;
    let inherited = create(
        &db,
        NewScheduleItem {
            model_template_id: Some(template_id),
            interval_miles: None,
            interval_months: Some(12),
            ..item_named("Cabin Air Filter")
        },
    )
    .await
    .unwrap();
    let svc = service_on(&db, vid, inherited.id).await;

    let own = create_for_vehicle(&db, vid, item_named("  cabin air filter "))
        .await
        .unwrap();
    assert_eq!(own.overrides_item_id, Some(inherited.id));
    assert_eq!(
        own.name, "Cabin Air Filter",
        "the stored name takes the inherited item's spelling"
    );

    let resolved = resolve(&db, vid).await.unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].effective_item.id, own.id);
    let rems = reminders::calculate_reminders(&db, vid).await.unwrap();
    assert_eq!(rems.reminders.len(), 1);
    assert_eq!(
        rems.reminders[0].last_service.as_ref().map(|s| s.id),
        Some(svc)
    );

    let err = create_for_vehicle(&db, vid, item_named("CABIN AIR FILTER"))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, DomainError::BadRequest(m) if m.contains("update_schedule_item")),
        "got: {err}"
    );
}

#[tokio::test]
async fn a_rename_that_differs_only_in_case_from_another_item_is_refused() {
    let db = test_db().await;
    let vid = seed_vehicle(&db, None).await;
    for name in ["Oil Change", "Wipers"] {
        create_for_vehicle(&db, vid, item_named(name))
            .await
            .unwrap();
    }
    let wipers = own_item_named(&db, vid, "wipers").await.unwrap().unwrap();
    let err = update_for_vehicle(
        &db,
        vid,
        wipers.id,
        UpdateScheduleItem {
            name: Some("oil change".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DomainError::BadRequest(_)), "got: {err}");

    // Respelling an item's own name is not a rename onto another item.
    let respelled = update_for_vehicle(
        &db,
        vid,
        wipers.id,
        UpdateScheduleItem {
            name: Some("WIPERS".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(respelled.name, "WIPERS");
}

#[tokio::test]
async fn an_inherited_item_still_keeps_its_name_when_the_rename_is_only_a_respelling() {
    let db = test_db().await;
    let (_pid, template_id, vid) = seed_chain(&db).await;
    let inherited = create(
        &db,
        NewScheduleItem {
            model_template_id: Some(template_id),
            ..item_named("Spark Plugs")
        },
    )
    .await
    .unwrap();
    let updated = update_for_vehicle(
        &db,
        vid,
        inherited.id,
        UpdateScheduleItem {
            name: Some("spark plugs".into()),
            interval_miles: Some(Some(20_000)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.name, "Spark Plugs");
    assert_eq!(updated.interval_miles, Some(20_000));
}

#[tokio::test]
async fn two_vehicles_on_one_template_keep_their_own_same_named_items() {
    let db = test_db().await;
    let (_pid, template_id, first) = seed_chain(&db).await;
    let second = seed_vehicle(&db, Some(template_id)).await;
    create(
        &db,
        NewScheduleItem {
            model_template_id: Some(template_id),
            ..item_named("Cabin Air Filter")
        },
    )
    .await
    .unwrap();

    let own = create_for_vehicle(
        &db,
        first,
        NewScheduleItem {
            interval_miles: Some(9_000),
            ..item_named("cabin air filter")
        },
    )
    .await
    .unwrap();

    let seen_by_first = resolve(&db, first).await.unwrap();
    assert_eq!(seen_by_first.len(), 1);
    assert_eq!(seen_by_first[0].effective_item.id, own.id);
    let seen_by_second = resolve(&db, second).await.unwrap();
    assert_eq!(seen_by_second.len(), 1);
    assert_eq!(seen_by_second[0].effective_item.interval_miles, Some(5_000));
    assert_eq!(
        seen_by_second[0].inherited_from.as_deref(),
        Some("model_template")
    );
    // The second car may add its own under yet another spelling.
    create_for_vehicle(&db, second, item_named("CABIN AIR FILTER"))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn racing_restores_of_one_item_all_succeed() {
    let (_dir, pools) = crate::test_support::race_pools().await;
    let first = &pools[0];
    let v = seed_vehicle(first, None).await;
    let item = create_for_vehicle(first, v, item_named("Wipers"))
        .await
        .unwrap()
        .id;
    dismiss_for_vehicle(first, v, item, None).await.unwrap();
    let results = crate::test_support::race(&pools, |_, db| async move {
        undismiss_for_vehicle(&db, v, item).await
    })
    .await;
    assert!(results.iter().all(Result::is_ok), "{results:?}");
}
