mod common;

use common::{connect, new_vehicle};
use glovebox_shared::services::vehicle;
use serde_json::json;

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
