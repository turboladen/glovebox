mod common;

use common::{connect, new_vehicle, schedule_item_every_12_months};
use glovebox_shared::{
    inputs::vehicle::NewVehicle,
    services::{schedule as schedule_svc, vehicle},
};
use serde_json::json;

#[tokio::test]
async fn check_due_maintenance_returns_without_error() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();

    let due = t
        .call("check_due_maintenance", json!({ "vehicle_id": v.id }))
        .await
        .to_string();
    assert!(
        due.contains("estimated_mileage") && due.contains("reminders"),
        "reminders payload must round-trip; got: {due}"
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
    let due = t
        .call("check_due_maintenance", json!({ "vehicle_id": v.id }))
        .await;
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

    t.call(
        "record_service",
        json!({
            "vehicle_id": v.id,
            "service_date": "2026-11-12",
            "work_item_ids": [item_id]
        }),
    )
    .await;
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
