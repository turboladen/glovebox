mod common;

use common::{connect, new_vehicle, reminder, schedule_item_every_12_months};
use glovebox_shared::{
    inputs::vehicle::NewVehicle,
    services::{schedule as schedule_svc, vehicle},
};
use serde_json::json;

#[tokio::test]
async fn dismiss_schedule_item_hides_it_and_rejects_wrong_vehicle() {
    let t = connect().await;
    let v = vehicle::create(
        &t.db,
        NewVehicle {
            purchase_date: Some("2020-01-01".into()),
            ..new_vehicle("Daily")
        },
    )
    .await
    .unwrap();
    let item = schedule_svc::create(
        &t.db,
        schedule_item_every_12_months(v.id, "Dealer inspection"),
    )
    .await
    .unwrap();
    assert_eq!(schedule_svc::resolve(&t.db, v.id).await.unwrap().len(), 1);

    let payload = t
        .call(
            "dismiss_schedule_item",
            json!({
                "vehicle_id": v.id,
                "schedule_item_id": item.id,
                "reason": "independent shop handles this"
            }),
        )
        .await;
    assert_eq!(payload["enabled"], false);
    assert!(
        payload["notes"]
            .as_str()
            .unwrap()
            .contains("independent shop handles this")
    );

    // Gone from the resolved schedule and from the reminders.
    assert!(schedule_svc::resolve(&t.db, v.id).await.unwrap().is_empty());
    let due = t.call("whats_coming", json!({ "vehicle_id": v.id })).await;
    assert_eq!(
        due["reminders"].as_array().map(Vec::len),
        Some(0),
        "dismissed item must not appear in reminders: {due}"
    );

    // Wrong-vehicle and nonexistent probes are the same clean tool error.
    let other = vehicle::create(&t.db, new_vehicle("Other")).await.unwrap();
    for schedule_item_id in [item.id, 9999] {
        let err = t
            .call_err(
                "dismiss_schedule_item",
                json!({ "vehicle_id": other.id, "schedule_item_id": schedule_item_id }),
            )
            .await;
        assert!(err.contains("not found"), "got: {err}");
    }
}

#[tokio::test]
async fn add_and_update_schedule_items_round_trip() {
    let t = connect().await;
    let v = vehicle::create(
        &t.db,
        NewVehicle {
            purchase_date: Some("2020-01-01".into()),
            ..new_vehicle("Daily")
        },
    )
    .await
    .unwrap();

    let added = t
        .call(
            "add_schedule_item",
            json!({
                "vehicle_id": v.id,
                "name": "Smog check",
                "interval_months": 24,
                "est_cost_cents": 4500,
                "notes": "state requirement"
            }),
        )
        .await;
    let item_id = added["id"].clone();
    assert_eq!(added["vehicle_id"], v.id);
    assert_eq!(
        common::reminder_status(&t, v.id, "Smog check").await,
        "overdue"
    );
    let listed = t.call("list_schedule", json!({ "vehicle_id": v.id })).await;
    assert_eq!(listed[0]["effective_item"]["name"], "Smog check");

    let updated = t
        .call(
            "update_schedule_item",
            json!({ "vehicle_id": v.id, "schedule_item_id": item_id, "est_cost_cents": 5800 }),
        )
        .await;
    assert_eq!(updated["est_cost_cents"], 5800);
    assert_eq!(updated["notes"], "state requirement");

    let cleared = t
        .call(
            "update_schedule_item",
            json!({ "vehicle_id": v.id, "schedule_item_id": item_id, "notes": null }),
        )
        .await;
    assert!(cleared["notes"].is_null(), "null clears notes: {cleared}");
    assert_eq!(cleared["est_cost_cents"], 5800, "absent keys stay put");

    // An interval of zero would read as overdue from the day it was serviced.
    for (tool, args) in [
        (
            "add_schedule_item",
            json!({ "vehicle_id": v.id, "name": "Wipers", "interval_months": 0 }),
        ),
        (
            "update_schedule_item",
            json!({ "vehicle_id": v.id, "schedule_item_id": item_id, "interval_months": 0 }),
        ),
    ] {
        let err = t.call_err(tool, args).await;
        assert!(err.starts_with("interval_months: "), "{tool} got: {err}");
    }

    let other = vehicle::create(&t.db, new_vehicle("Other")).await.unwrap();
    let wrong = t
        .call_err(
            "update_schedule_item",
            json!({ "vehicle_id": other.id, "schedule_item_id": item_id, "notes": "x" }),
        )
        .await;
    let missing = t
        .call_err(
            "update_schedule_item",
            json!({ "vehicle_id": other.id, "schedule_item_id": 99999, "notes": "x" }),
        )
        .await;
    assert_eq!(
        wrong.replace(&item_id.to_string(), "N"),
        missing.replace("99999", "N"),
        "a wrong-vehicle id must read like a nonexistent one"
    );
}

#[tokio::test]
async fn changing_an_inherited_item_keeps_its_service_history_and_plans() {
    let t = connect().await;
    let template = schedule_svc_template(&t).await;
    let v = vehicle::create(
        &t.db,
        NewVehicle {
            model_template_id: Some(template),
            purchase_date: Some("2017-03-05".into()),
            purchase_mileage: Some(40),
            ..new_vehicle("Golf")
        },
    )
    .await
    .unwrap();
    let coming = t.call("whats_coming", json!({ "vehicle_id": v.id })).await;
    let plugs_id = reminder(&coming, "Spark Plugs")["schedule_item"]["id"].clone();

    let recorded = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-11-10",
                "mileage": 64_000,
                "description": "Spark plugs",
                "schedule_item_ids": [plugs_id]
            }),
        )
        .await;
    let service_id = recorded["id"].clone();
    let planned = t
        .call(
            "plan_work",
            json!({ "vehicle_id": v.id, "title": "Plugs again", "schedule_item_id": plugs_id }),
        )
        .await;
    let before = t.call("whats_coming", json!({ "vehicle_id": v.id })).await;
    assert_eq!(reminder(&before, "Spark Plugs")["status"], "ok");

    let updated = t
        .call(
            "update_schedule_item",
            json!({ "vehicle_id": v.id, "schedule_item_id": plugs_id, "est_cost_cents": 25_000 }),
        )
        .await;
    assert_ne!(updated["id"], plugs_id, "the shared item is never edited");
    assert_eq!(updated["overrides_item_id"], plugs_id);
    assert_eq!(updated["est_cost_cents"], 25_000);

    let after = t.call("whats_coming", json!({ "vehicle_id": v.id })).await;
    let plugs = reminder(&after, "Spark Plugs");
    assert_eq!(plugs["schedule_item"]["id"], updated["id"]);
    assert_eq!(plugs["status"], "ok", "got: {plugs}");
    assert_eq!(plugs["last_service"]["id"], service_id);
    let work = after["planned_work"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == planned["id"])
        .unwrap_or_else(|| panic!("planned work must still be open: {after}"));
    assert_eq!(work["schedule_item_id"], updated["id"]);

    // The same call on the new id edits the same item.
    let again = t
        .call(
            "update_schedule_item",
            json!({ "vehicle_id": v.id, "schedule_item_id": updated["id"], "notes": "NGK" }),
        )
        .await;
    assert_eq!(again["id"], updated["id"]);
}

async fn schedule_svc_template(t: &common::TestClient) -> i32 {
    glovebox_shared::services::model_template::list(&t.db)
        .await
        .unwrap()[0]
        .id
}

#[tokio::test]
async fn a_dismissed_item_can_be_found_and_restored() {
    let t = connect().await;
    let template = schedule_svc_template(&t).await;
    let v = vehicle::create(
        &t.db,
        NewVehicle {
            model_template_id: Some(template),
            ..new_vehicle("Golf")
        },
    )
    .await
    .unwrap();
    let coming = t.call("whats_coming", json!({ "vehicle_id": v.id })).await;
    let wipers = reminder(&coming, "Wiper Blades")["schedule_item"]["id"].clone();
    t.call(
        "dismiss_schedule_item",
        json!({ "vehicle_id": v.id, "schedule_item_id": wipers }),
    )
    .await;

    let named = |listed: &serde_json::Value| -> Option<serde_json::Value> {
        listed
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["effective_item"]["name"] == "Wiper Blades")
            .map(|i| i["effective_item"].clone())
    };
    let shown = t.call("list_schedule", json!({ "vehicle_id": v.id })).await;
    assert!(
        named(&shown).is_none(),
        "dismissed items are hidden by default"
    );
    let all = t
        .call(
            "list_schedule",
            json!({ "vehicle_id": v.id, "include_dismissed": true }),
        )
        .await;
    let dismissed = named(&all).unwrap_or_else(|| panic!("dismissed item listed: {all}"));
    assert_eq!(dismissed["enabled"], false);

    let restored = t
        .call(
            "restore_schedule_item",
            json!({ "vehicle_id": v.id, "schedule_item_id": dismissed["id"] }),
        )
        .await;
    assert_eq!(restored["enabled"], true);
    let coming = t.call("whats_coming", json!({ "vehicle_id": v.id })).await;
    reminder(&coming, "Wiper Blades");

    let other = vehicle::create(&t.db, new_vehicle("Other")).await.unwrap();
    let err = t
        .call_err(
            "restore_schedule_item",
            json!({ "vehicle_id": other.id, "schedule_item_id": dismissed["id"] }),
        )
        .await;
    assert!(err.contains("not found"), "got: {err}");
}

#[tokio::test]
async fn schedule_tools_reject_unknown_arguments() {
    let t = connect().await;
    common::assert_unknown_field_rejected(
        &t,
        "add_schedule_item",
        json!({ "vehicle_id": 1, "name": "Smog", "interval_km": 5 }),
        "interval_km",
    )
    .await;
}
