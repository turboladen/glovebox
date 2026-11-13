mod common;

use common::{connect, new_vehicle, schedule_item_every_12_months};
use glovebox_shared::{
    inputs::vehicle::NewVehicle,
    services::{schedule as schedule_svc, vehicle},
};
use serde_json::json;

#[tokio::test]
async fn whats_coming_returns_every_section_for_a_bare_vehicle() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();

    let due = t.call("whats_coming", json!({ "vehicle_id": v.id })).await;
    for key in [
        "reminders",
        "budget",
        "planned_work",
        "recalls",
        "open_incidents",
    ] {
        assert!(due.get(key).is_some(), "missing {key}; got: {due}");
    }
    assert!(due.get("estimated_mileage").is_some(), "got: {due}");
}

#[tokio::test]
async fn whats_coming_flags_missing_history_and_missing_estimates() {
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
    for name in ["Brake fluid", "Cabin filter"] {
        schedule_svc::create(&t.db, schedule_item_every_12_months(v.id, name))
            .await
            .unwrap();
    }
    t.call(
        "plan_work",
        json!({ "vehicle_id": v.id, "title": "Alignment" }),
    )
    .await;

    let due = t.call("whats_coming", json!({ "vehicle_id": v.id })).await;
    assert_eq!(due["reminders"][0]["baseline"], "purchase", "got: {due}");
    assert_eq!(due["bundle_suggestions"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        due["bundle_suggestions"][0]["items"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(due["budget"]["unestimated_item_count"], 2);
    assert_eq!(due["budget"]["unestimated_planned_work_count"], 1);
    assert_eq!(
        due["budget"]["unestimated_planned_work"][0]["title"],
        "Alignment"
    );
}

// Plans work from a recall finding and from an overdue schedule item, then
// reads the to-do list and the forecast.
#[tokio::test]
async fn recall_and_schedule_item_plan_into_the_todo_list_and_forecast() {
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
    let overdue = schedule_svc::create(
        &t.db,
        schedule_item_every_12_months(v.id, "Water pump replacement"),
    )
    .await
    .unwrap();

    let finding = t
        .call(
            "file_research_finding",
            json!({
                "vehicle_id": v.id,
                "category": "recall",
                "title": "Recall: coolant pump may seize",
                "severity": "critical"
            }),
        )
        .await;
    let finding_id = finding["id"].as_i64().expect("finding id");

    let planned = t
        .call(
            "plan_work",
            json!({
                "vehicle_id": v.id,
                "title": "Coolant pump recall fix",
                "research_finding_id": finding_id,
                "est_cost_cents": 0
            }),
        )
        .await;
    assert_eq!(planned["status"], "planned");

    t.call(
        "plan_work",
        json!({
            "vehicle_id": v.id,
            "title": "Water pump replacement",
            "schedule_item_id": overdue.id,
            "est_cost_cents": 65_000
        }),
    )
    .await;

    let list = t
        .call("list_planned_work", json!({ "vehicle_id": v.id }))
        .await;
    assert_eq!(list["items"].as_array().map(Vec::len), Some(2));
    assert!(
        list.get("visits").is_none(),
        "planning state no longer carries visits"
    );

    // The forecast counts the planned estimates once.
    let due = t.call("whats_coming", json!({ "vehicle_id": v.id })).await;
    assert_eq!(due["budget"]["planned_work_cents"], 65_000);
    assert!(due["budget"].get("planned_visits_cents").is_none());
}

#[tokio::test]
async fn planning_tools_reject_wrong_vehicle_and_malformed_args() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Mine")).await.unwrap();
    let other = vehicle::create(&t.db, new_vehicle("Other")).await.unwrap();
    let foreign_item = schedule_svc::create(&t.db, schedule_item_every_12_months(v.id, "Oil"))
        .await
        .unwrap();

    // Another vehicle's schedule item reads as missing.
    let err = t
        .call_err(
            "plan_work",
            json!({
                "vehicle_id": other.id,
                "title": "Not my item",
                "schedule_item_id": foreign_item.id
            }),
        )
        .await;
    assert!(err.contains("not found"), "got: {err}");

    t.call_err("list_planned_work", json!({ "vehicle_id": 999 }))
        .await;

    // A missing required title points at the schema.
    let err = t.call_err("plan_work", json!({ "vehicle_id": v.id })).await;
    assert!(
        err.contains("title") && err.contains("input schema"),
        "must name the missing field and point at the schema; got: {err}"
    );
}

// A to-do item with a revisit-by reports its status, and a service that names
// it completes it.
#[tokio::test]
async fn revisit_by_work_reports_its_status_and_a_service_completes_it() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    t.call(
        "log_mileage",
        json!({ "vehicle_id": v.id, "mileage": 64_000 }),
    )
    .await;

    let planned = t
        .call(
            "plan_work",
            json!({
                "vehicle_id": v.id,
                "title": "Re-check the valve cover seep",
                "revisit_by_date": "2999-05-01",
                "revisit_by_miles": 200_000
            }),
        )
        .await;
    let item_id = planned["id"].as_i64().expect("item id");

    let list = t
        .call("list_planned_work", json!({ "vehicle_id": v.id }))
        .await;
    let item = &list["items"][0];
    assert_eq!(item["revisit_by_date"], "2999-05-01");
    assert_eq!(item["revisit_by_miles"], 200_000);
    assert_eq!(item["revisit_status"], "ok");

    let service = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-11-12",
                "work_item_ids": [item_id]
            }),
        )
        .await;
    assert_eq!(
        service["completed_work_items"],
        json!([{ "id": item_id, "title": "Re-check the valve cover seep" }])
    );
    let open = t
        .call("list_planned_work", json!({ "vehicle_id": v.id }))
        .await;
    assert_eq!(open["items"].as_array().map(Vec::len), Some(0));
    let all = t
        .call(
            "list_planned_work",
            json!({ "vehicle_id": v.id, "include_done": true }),
        )
        .await;
    assert_eq!(all["items"][0]["status"], "done");
}

#[tokio::test]
async fn update_and_delete_planned_work() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let other = vehicle::create(&t.db, new_vehicle("Other")).await.unwrap();
    let item = t
        .call(
            "plan_work",
            json!({
                "vehicle_id": v.id,
                "title": "Replace pads",
                "est_cost_cents": 30_000,
                "notes": "front only"
            }),
        )
        .await;
    let wid = item["id"].clone();

    let updated = t
        .call(
            "update_planned_work",
            json!({
                "vehicle_id": v.id,
                "work_item_id": wid,
                "title": "Replace pads and rotors",
                "notes": null
            }),
        )
        .await;
    assert_eq!(updated["title"], "Replace pads and rotors");
    assert!(updated["notes"].is_null(), "null clears notes: {updated}");
    assert_eq!(updated["est_cost_cents"], 30_000, "absent keys stay put");

    let before = t.call("whats_coming", json!({ "vehicle_id": v.id })).await;
    assert_eq!(before["planned_work"].as_array().map(Vec::len), Some(1));
    t.call(
        "update_planned_work",
        json!({ "vehicle_id": v.id, "work_item_id": wid, "status": "dropped" }),
    )
    .await;
    let after = t.call("whats_coming", json!({ "vehicle_id": v.id })).await;
    assert_eq!(
        after["planned_work"].as_array().map(Vec::len),
        Some(0),
        "dropped work leaves the forecast: {after}"
    );

    for tool in ["update_planned_work", "delete_planned_work"] {
        let wrong = t
            .call_err(tool, json!({ "vehicle_id": other.id, "work_item_id": wid }))
            .await;
        let missing = t
            .call_err(
                tool,
                json!({ "vehicle_id": other.id, "work_item_id": 99999 }),
            )
            .await;
        assert_eq!(
            wrong.replace(&wid.to_string(), "N"),
            missing.replace("99999", "N"),
            "{tool}: a wrong-vehicle id must read like a nonexistent one"
        );
    }

    t.call(
        "delete_planned_work",
        json!({ "vehicle_id": v.id, "work_item_id": wid }),
    )
    .await;
    let listed = t
        .call(
            "list_planned_work",
            json!({ "vehicle_id": v.id, "include_done": true }),
        )
        .await;
    assert_eq!(listed["items"], json!([]));
}

#[tokio::test]
async fn planning_tools_reject_unknown_arguments() {
    let t = connect().await;
    common::assert_unknown_field_rejected(
        &t,
        "plan_work",
        json!({ "vehicle_id": 1, "title": "Pads", "priority": "high" }),
        "priority",
    )
    .await;
}
