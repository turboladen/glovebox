mod common;

use common::{connect, new_vehicle};
use glovebox_shared::services::vehicle;
use serde_json::json;

#[tokio::test]
async fn log_incident_and_mileage_write_through() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();

    t.call(
        "log_incident",
        json!({
            "vehicle_id": v.id,
            "title": "Squeak from front left",
            "category": "noise"
        }),
    )
    .await;
    t.call(
        "log_mileage",
        json!({ "vehicle_id": v.id, "mileage": 48_250 }),
    )
    .await;

    let feed = t
        .call("summarize_recent_activity", json!({ "vehicle_id": v.id }))
        .await
        .to_string();
    assert!(feed.contains("Squeak from front left") && feed.contains("48250"));
}

#[tokio::test]
async fn log_incident_rejects_unknown_category_and_wrong_links() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let other = vehicle::create(&t.db, new_vehicle("Other")).await.unwrap();

    let err = t
        .call_err(
            "log_incident",
            json!({ "vehicle_id": v.id, "title": "Bad category", "category": "bogus" }),
        )
        .await;
    assert!(
        err.contains("Invalid category") && err.contains("warning_light"),
        "the error must steer toward valid categories; got: {err}"
    );

    let incident = t
        .call(
            "log_incident",
            json!({
                "vehicle_id": v.id,
                "title": "Rear-ended at a stoplight",
                "category": "accident",
                "description": "Other driver ran the light"
            }),
        )
        .await;
    assert_eq!(incident["title"], "Rear-ended at a stoplight");
    let incident_id = incident["id"].as_i64().expect("incident id");

    // A recurrence pointing at another vehicle's incident must be
    // indistinguishable from a nonexistent one.
    let err = t
        .call_err(
            "log_incident",
            json!({
                "vehicle_id": other.id,
                "title": "Not my incident",
                "recurrence_of_id": incident_id
            }),
        )
        .await;
    assert!(
        err.contains("not found"),
        "cross-vehicle recurrence must read as missing; got: {err}"
    );

    let again = t
        .call(
            "log_incident",
            json!({
                "vehicle_id": v.id,
                "title": "Hit again in the same spot",
                "category": "accident",
                "recurrence_of_id": incident_id
            }),
        )
        .await;
    assert_eq!(again["recurrence_of_id"], incident_id);
}

#[tokio::test]
async fn save_note_is_searchable_via_search_records() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();

    let note = t
        .call(
            "save_note",
            json!({
                "vehicle_id": v.id,
                "note": "Steve prefers Liqui Moly 5W-40 for this engine"
            }),
        )
        .await;
    assert_eq!(note["category"], "note");

    // The FTS trigger on incidents must make the note findable immediately.
    let hits = t
        .call(
            "search_records",
            json!({ "query": "Liqui Moly", "scope": "incidents" }),
        )
        .await
        .to_string();
    assert!(
        hits.contains("Liqui Moly"),
        "note must be searchable under the incidents scope; got: {hits}"
    );

    t.call_err("save_note", json!({ "vehicle_id": 999, "note": "orphan" }))
        .await;
}
