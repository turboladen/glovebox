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

#[tokio::test]
async fn list_correct_and_delete_a_part() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let other = vehicle::create(&t.db, new_vehicle("Other")).await.unwrap();
    let part = t
        .call(
            "record_part",
            json!({
                "vehicle_id": v.id,
                "name": "Clutch kit",
                "cost_cents": 89_900,
                "seller": "FCP Euro"
            }),
        )
        .await;
    let pid = part["id"].clone();

    let listed = t.call("list_parts", json!({ "vehicle_id": v.id })).await;
    assert_eq!(listed.as_array().map(Vec::len), Some(1));
    let installed_only = t
        .call(
            "list_parts",
            json!({ "vehicle_id": v.id, "status": "installed" }),
        )
        .await;
    assert_eq!(installed_only, json!([]));

    let corrected = t
        .call(
            "correct_part",
            json!({
                "vehicle_id": v.id,
                "part_id": pid,
                "cost_cents": 79_900,
                "seller": null
            }),
        )
        .await;
    assert_eq!(corrected["cost_cents"], 79_900);
    assert!(
        corrected["seller"].is_null(),
        "null clears seller: {corrected}"
    );
    assert_eq!(corrected["name"], "Clutch kit", "absent keys stay put");

    for tool in ["correct_part", "delete_part"] {
        let wrong = t
            .call_err(tool, json!({ "vehicle_id": other.id, "part_id": pid }))
            .await;
        let missing = t
            .call_err(tool, json!({ "vehicle_id": other.id, "part_id": 99999 }))
            .await;
        assert_eq!(
            wrong.replace(&pid.to_string(), "N"),
            missing.replace("99999", "N"),
            "{tool}: a wrong-vehicle id must read like a nonexistent one"
        );
    }

    t.call("delete_part", json!({ "vehicle_id": v.id, "part_id": pid }))
        .await;
    let after = t.call("list_parts", json!({ "vehicle_id": v.id })).await;
    assert_eq!(after, json!([]));
}
