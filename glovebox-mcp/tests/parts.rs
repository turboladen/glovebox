mod common;

use common::{connect, new_vehicle};
use glovebox_shared::services::vehicle;
use serde_json::json;

#[tokio::test]
async fn record_part_round_trips_and_rejects_a_missing_vehicle() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();

    // Parts have no MCP list surface; the tool result payload is the created
    // part record itself.
    let part = t
        .call(
            "record_part",
            json!({
                "vehicle_id": v.id,
                "name": "Sachs SRE clutch kit",
                "cost_cents": 89_900,
                "location": "Front brakes",
                "seller": "FCP Euro"
            }),
        )
        .await;
    assert_eq!(part["name"], "Sachs SRE clutch kit");
    assert_eq!(part["location"], "Front brakes");
    assert!(
        part["id"].is_i64(),
        "created part must carry an id; got: {part}"
    );
    assert_eq!(
        part["status"], "purchased",
        "status must default to purchased"
    );

    t.call_err(
        "record_part",
        json!({ "vehicle_id": 999, "name": "Oil filter" }),
    )
    .await;
}
