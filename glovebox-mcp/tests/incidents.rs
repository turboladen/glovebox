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

#[tokio::test]
async fn a_shop_observation_is_linked_to_the_services_that_noticed_and_fixed_it() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let other = vehicle::create(&t.db, new_vehicle("Other")).await.unwrap();

    let svc_2023 = t
        .call(
            "record_service",
            json!({ "vehicle_id": v.id, "service_date": "2023-05-03", "description": "Inspection" }),
        )
        .await["id"]
        .as_i64()
        .expect("service id");
    let incident = t
        .call(
            "log_incident",
            json!({
                "vehicle_id": v.id,
                "title": "N80 purge valve failing",
                "category": "shop_observation",
                "noticed_during_service_id": svc_2023
            }),
        )
        .await;
    assert_eq!(incident["noticed_during"]["id"], svc_2023);
    let incident_id = incident["id"].as_i64().expect("incident id");

    // Before the fix, the observation is open on the 2023 service.
    let open = t
        .call(
            "get_service",
            json!({ "vehicle_id": v.id, "service_id": svc_2023 }),
        )
        .await;
    assert_eq!(open["noticed_incidents"][0]["resolved"], false);
    assert!(open["noticed_incidents"][0]["other_service"].is_null());

    let svc_2024 = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2024-06-12",
                "resolved_incident_ids": [incident_id]
            }),
        )
        .await["id"]
        .as_i64()
        .expect("service id");

    let s23 = t
        .call(
            "get_service",
            json!({ "vehicle_id": v.id, "service_id": svc_2023 }),
        )
        .await;
    assert_eq!(
        s23["noticed_incidents"][0]["title"],
        "N80 purge valve failing"
    );
    assert_eq!(s23["noticed_incidents"][0]["resolved"], true);
    assert_eq!(
        s23["noticed_incidents"][0]["other_service"]["service_date"],
        "2024-06-12"
    );
    let s24 = t
        .call(
            "get_service",
            json!({ "vehicle_id": v.id, "service_id": svc_2024 }),
        )
        .await;
    assert_eq!(
        s24["resolved_incidents"][0]["other_service"]["service_date"],
        "2023-05-03"
    );
    let inc = t
        .call(
            "get_incident",
            json!({ "vehicle_id": v.id, "incident_id": incident_id }),
        )
        .await;
    assert_eq!(inc["noticed_during"]["id"], svc_2023);
    assert_eq!(inc["resolved_by"][0]["id"], svc_2024);

    // Another vehicle's ids read as missing.
    let err = t
        .call_err(
            "get_service",
            json!({ "vehicle_id": other.id, "service_id": svc_2023 }),
        )
        .await;
    assert!(err.contains("not found"), "got: {err}");
    let err = t
        .call_err(
            "get_incident",
            json!({ "vehicle_id": other.id, "incident_id": incident_id }),
        )
        .await;
    assert!(err.contains("not found"), "got: {err}");
}
