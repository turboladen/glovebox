mod common;

use common::{connect, new_vehicle, seed_recall};
use glovebox_shared::services::vehicle;
use serde_json::json;

#[tokio::test]
async fn a_recall_named_on_a_service_reads_as_done_with_that_service() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let recall = seed_recall(&t.db, v.id, "24V110000").await;
    let other = seed_recall(&t.db, v.id, "24V220000").await;

    let service = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2025-03-25",
                "description": "Recall 24V110",
                "recall_finding_ids": [recall]
            }),
        )
        .await;
    assert_eq!(service["performed_recalls"][0]["finding_id"], recall);
    assert_eq!(
        service["performed_recalls"][0]["campaign_number"],
        "24V110000"
    );

    let read = t
        .call(
            "get_service",
            json!({ "vehicle_id": v.id, "service_id": service["id"] }),
        )
        .await;
    assert_eq!(read["performed_recalls"][0]["finding_id"], recall);

    let done = t
        .call(
            "set_recall_status",
            json!({ "vehicle_id": v.id, "finding_id": recall, "status": "done" }),
        )
        .await;
    assert_eq!(done["status"], "done");
    assert_eq!(done["done_service"]["service_date"], "2025-03-25");
    assert_eq!(done["done_service"]["id"], service["id"]);

    let err = t
        .call_err(
            "set_recall_status",
            json!({ "vehicle_id": v.id, "finding_id": recall, "status": "open" }),
        )
        .await;
    assert!(err.contains("linked to a service"), "got: {err}");

    let skipped = t
        .call(
            "set_recall_status",
            json!({ "vehicle_id": v.id, "finding_id": other, "status": "not_applicable" }),
        )
        .await;
    assert_eq!(skipped["status"], "not_applicable");
    assert_eq!(skipped["done_service"], serde_json::Value::Null);

    t.call_err(
        "set_recall_status",
        json!({ "vehicle_id": v.id, "finding_id": 9999, "status": "done" }),
    )
    .await;
}

#[tokio::test]
async fn list_recalls_shows_every_status_without_a_network_call() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let open = seed_recall(&t.db, v.id, "24V110000").await;
    let done = seed_recall(&t.db, v.id, "24V220000").await;
    let skipped = seed_recall(&t.db, v.id, "24V330000").await;
    let planned = seed_recall(&t.db, v.id, "24V440000").await;
    let item = t
        .call(
            "plan_work",
            json!({ "vehicle_id": v.id, "title": "Recall", "research_finding_id": planned }),
        )
        .await;
    let service = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id, "service_date": "2025-03-25",
                "description": "Recall 24V220", "recall_finding_ids": [done]
            }),
        )
        .await;
    t.call(
        "set_recall_status",
        json!({ "vehicle_id": v.id, "finding_id": skipped, "status": "not_applicable" }),
    )
    .await;

    let out = t.call("list_recalls", json!({ "vehicle_id": v.id })).await;
    let by_id = |id: i32| {
        out["recalls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["finding_id"] == id)
            .unwrap_or_else(|| panic!("recall {id} missing from {out}"))
            .clone()
    };
    assert_eq!(by_id(open)["status"], "open");
    assert_eq!(by_id(open)["campaign_number"], "24V110000");
    assert_eq!(by_id(planned)["status"], "planned");
    assert_eq!(by_id(planned)["work_item_id"], item["id"]);
    assert_eq!(by_id(done)["status"], "done");
    assert_eq!(by_id(done)["done_service"]["id"], service["id"]);
    assert_eq!(by_id(open)["work_item_id"], serde_json::Value::Null);
    assert_eq!(by_id(skipped)["status"], "not_applicable");
    assert!(by_id(open)["title"].is_string());

    t.call_err("list_recalls", json!({ "vehicle_id": 9999 }))
        .await;
}

// `check_recalls` makes a live NHTSA request, so it is not exercised here.
#[tokio::test]
async fn file_research_finding_persists_and_rejects_a_missing_vehicle() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();

    let finding = t
        .call(
            "file_research_finding",
            json!({
                "vehicle_id": v.id,
                "category": "maintenance",
                "title": "DSG interval 40k",
                "source_url": "https://example.com/t"
            }),
        )
        .await;
    assert_eq!(finding["title"], "DSG interval 40k");

    t.call_err(
        "file_research_finding",
        json!({ "vehicle_id": 999, "category": "x", "title": "y" }),
    )
    .await;
}

#[tokio::test]
async fn recalls_tools_reject_unknown_arguments() {
    let t = connect().await;
    common::assert_unknown_field_rejected(
        &t,
        "set_recall_status",
        json!({ "vehicle_id": 1, "finding_id": 1, "status": "done", "campaign": "20UF" }),
        "campaign",
    )
    .await;
}
