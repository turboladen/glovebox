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
