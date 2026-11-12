mod common;

use common::{connect, new_vehicle, schedule_item_every_12_months};
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
async fn updating_an_inherited_schedule_item_points_at_dismiss() {
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
    let listed = t.call("list_schedule", json!({ "vehicle_id": v.id })).await;
    let inherited = listed
        .as_array()
        .and_then(|items| items.iter().find(|i| !i["inherited_from"].is_null()))
        .unwrap_or_else(|| panic!("a seeded template item must be inherited: {listed}"));

    let err = t
        .call_err(
            "update_schedule_item",
            json!({
                "vehicle_id": v.id,
                "schedule_item_id": inherited["effective_item"]["id"],
                "est_cost_cents": 1
            }),
        )
        .await;
    assert!(err.contains("dismiss_schedule_item"), "got: {err}");
}

async fn schedule_svc_template(t: &common::TestClient) -> i32 {
    glovebox_shared::services::model_template::list(&t.db)
        .await
        .unwrap()[0]
        .id
}
