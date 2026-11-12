mod common;

use common::{connect, new_vehicle};
use glovebox_shared::{
    entities::{research_finding, research_report},
    services::vehicle,
};
use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};
use serde_json::json;

async fn seed_recall(db: &DatabaseConnection, vehicle_id: i32, campaign: &str) -> i32 {
    let report = research_report::ActiveModel {
        vehicle_id: Set(vehicle_id),
        report_type: Set(Some("recalls_only".into())),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
    research_finding::ActiveModel {
        report_id: Set(report.id),
        category: Set("recall".into()),
        title: Set("Suction Jet Pump".into()),
        source_url: Set(Some(format!(
            "https://www.nhtsa.gov/recalls?nhtsaId={campaign}"
        ))),
        status: Set("new".into()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id
}

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
