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
            purchase_date: Some(glovebox_shared::test_support::date("2020-01-01")),
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
            purchase_date: Some(glovebox_shared::test_support::date("2020-01-01")),
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
async fn update_planned_work_reports_the_appointment_a_reopened_job_moves() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let mut jobs = Vec::new();
    for (purpose, status) in [("Brakes", "done"), ("Wipers", "cancelled")] {
        let appt = t
            .call(
                "plan_appointment",
                json!({ "vehicle_id": v.id, "purpose": purpose }),
            )
            .await["id"]
            .clone();
        let job = t
            .call(
                "plan_work",
                json!({ "vehicle_id": v.id, "title": purpose, "appointment_id": appt }),
            )
            .await["id"]
            .clone();
        t.call(
            "update_planned_work",
            json!({ "vehicle_id": v.id, "work_item_id": job, "status": "done" }),
        )
        .await;
        t.call(
            "update_appointment",
            json!({ "vehicle_id": v.id, "appointment_id": appt, "status": status }),
        )
        .await;
        jobs.push((appt, job));
    }
    let [(closed, on_closed), (cancelled, on_cancelled)] = jobs.try_into().unwrap();
    let back_to_planned =
        |job| json!({ "vehicle_id": v.id, "work_item_id": job, "status": "planned" });

    let reopened = t
        .call("update_planned_work", back_to_planned(on_closed.clone()))
        .await;
    assert_eq!(reopened["reopened_appointment_id"], closed);
    assert_eq!(reopened["appointment_id"], closed);
    assert!(
        reopened.get("left_cancelled_appointment_id").is_none(),
        "{reopened}"
    );

    let left = t
        .call("update_planned_work", back_to_planned(on_cancelled.clone()))
        .await;
    assert_eq!(left["left_cancelled_appointment_id"], cancelled);
    assert!(left["appointment_id"].is_null(), "{left}");
    assert!(left.get("reopened_appointment_id").is_none(), "{left}");

    let edited = t
        .call(
            "update_planned_work",
            json!({ "vehicle_id": v.id, "work_item_id": on_closed, "title": "Rear brakes" }),
        )
        .await;
    assert!(edited.get("reopened_appointment_id").is_none(), "{edited}");
    assert!(
        edited.get("left_cancelled_appointment_id").is_none(),
        "{edited}"
    );
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

#[tokio::test]
async fn a_quote_is_planned_work_with_its_source() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    t.call("add_shop", json!({ "name": "City VW" })).await;
    let service = t
        .call(
            "record_service",
            json!({ "vehicle_id": v.id, "service_date": "2020-06-01", "shop_name": "City VW" }),
        )
        .await;
    let quote = t
        .call(
            "plan_work",
            json!({
                "vehicle_id": v.id, "title": "Repair the driver's mirror", "est_cost_cents": 42_000,
                "quoted_by": "City VW", "quoted_on": "2020-06-01", "quote_service_id": service["id"]
            }),
        )
        .await;
    assert_eq!(quote["quoted_by"], "City VW", "got: {quote}");
    assert_eq!(quote["quoted_on"], "2020-06-01");
    assert_eq!(quote["quote_service_id"], service["id"]);

    let listed = t
        .call("list_planned_work", json!({ "vehicle_id": v.id }))
        .await;
    assert_eq!(listed["items"][0]["quoted_by"], "City VW", "got: {listed}");

    let cleared = t
        .call(
            "update_planned_work",
            json!({ "vehicle_id": v.id, "work_item_id": quote["id"], "quote_service_id": null }),
        )
        .await;
    assert!(cleared["quote_service_id"].is_null(), "got: {cleared}");

    let err = t
        .call_err(
            "plan_work",
            json!({ "vehicle_id": v.id, "title": "Mirror", "quoted_on": "June 2020" }),
        )
        .await;
    assert!(err.contains("quoted_on"), "got: {err}");
}

#[tokio::test]
async fn update_planned_work_names_the_schedule_item_that_stopped_counting() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let item = schedule_svc::create(
        &t.db,
        schedule_item_every_12_months(v.id, "Brake fluid flush"),
    )
    .await
    .unwrap();
    let job = t
        .call(
            "plan_work",
            json!({ "vehicle_id": v.id, "title": "Flush", "schedule_item_id": item.id }),
        )
        .await["id"]
        .clone();
    let service = t
        .call(
            "record_service",
            json!({ "vehicle_id": v.id, "service_date": "2026-06-01", "work_item_ids": [job] }),
        )
        .await;

    let edited = t
        .call(
            "update_planned_work",
            json!({ "vehicle_id": v.id, "work_item_id": job, "title": "Flush fluid" }),
        )
        .await;
    assert!(
        edited.get("released_schedule_item_id").is_none(),
        "{edited}"
    );

    let back = t
        .call(
            "update_planned_work",
            json!({ "vehicle_id": v.id, "work_item_id": job, "status": "planned" }),
        )
        .await;
    assert_eq!(back["released_schedule_item_id"], item.id);
    assert_eq!(back["released_from_service_id"], service["id"]);
}

// Records a service that completes a new job, and logs an incident the job
// was not filed from. Returns the job id, the service id and the incident id.
async fn completed_job_and_an_incident(
    t: &common::TestClient,
    vehicle_id: i32,
) -> (serde_json::Value, serde_json::Value, serde_json::Value) {
    let job = t
        .call(
            "plan_work",
            json!({ "vehicle_id": vehicle_id, "title": "Fix leak" }),
        )
        .await["id"]
        .clone();
    let service = t
        .call(
            "record_service",
            json!({ "vehicle_id": vehicle_id, "service_date": "2026-06-01", "work_item_ids": [job] }),
        )
        .await["id"]
        .clone();
    let incident = t
        .call(
            "log_incident",
            json!({ "vehicle_id": vehicle_id, "title": "Leak" }),
        )
        .await["id"]
        .clone();
    (job, service, incident)
}

#[tokio::test]
async fn giving_a_completed_job_an_incident_resolves_it_and_deleting_the_job_keeps_that() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let (job, service, incident) = completed_job_and_an_incident(&t, v.id).await;

    t.call(
        "update_planned_work",
        json!({ "vehicle_id": v.id, "work_item_id": job, "incident_id": incident }),
    )
    .await;
    let read = || {
        t.call(
            "get_incident",
            json!({ "vehicle_id": v.id, "incident_id": incident }),
        )
    };
    let linked = read().await;
    assert_eq!(linked["resolved"], true, "got: {linked}");
    assert_eq!(linked["resolved_by"][0]["id"], service);

    t.call(
        "delete_planned_work",
        json!({ "vehicle_id": v.id, "work_item_id": job }),
    )
    .await;
    let kept = read().await;
    assert_eq!(kept["resolved"], true, "got: {kept}");
    assert_eq!(kept["resolved_by"][0]["id"], service);
}

#[tokio::test]
async fn update_planned_work_names_the_incident_its_service_stopped_resolving() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let (job, service, incident) = completed_job_and_an_incident(&t, v.id).await;
    let linked = t
        .call(
            "update_planned_work",
            json!({ "vehicle_id": v.id, "work_item_id": job, "incident_id": incident }),
        )
        .await;
    assert!(
        linked.get("released_incident_id").is_none(),
        "got: {linked}"
    );

    let back = t
        .call(
            "update_planned_work",
            json!({ "vehicle_id": v.id, "work_item_id": job, "status": "planned" }),
        )
        .await;
    assert_eq!(back["released_incident_id"], incident);
    assert_eq!(back["released_from_service_id"], service);
}

// The old car is long past its warranty date and still under the mileage
// limit, so the stricter reading says its warranty has ended.
#[tokio::test]
async fn whats_coming_reports_each_warranty_bound() {
    let t = connect().await;
    let add = |name: &'static str, expires_on: serde_json::Value| {
        t.call(
            "add_vehicle",
            json!({
                "name": name,
                "purchase_mileage": 50_000,
                "warranty_expires_on": expires_on,
                "warranty_expires_miles": 60_000
            }),
        )
    };
    let old = add("Old", json!("2016-01-01")).await["id"].clone();
    let new = add("New", json!("2099-01-01")).await["id"].clone();
    let bare = common::add_vehicle(&t, "Bare").await;

    let w = t.call("whats_coming", json!({ "vehicle_id": old })).await["warranty"].clone();
    assert_eq!(w["possibly_covered"], true, "{w}");
    assert_eq!(w["in_force"], false, "{w}");
    assert_eq!(w["date_passed"], true);
    assert_eq!(w["days_left"], 0);
    assert_eq!(w["mileage_passed"], false);
    assert_eq!(w["miles_left"], 10_000);

    let w = t.call("whats_coming", json!({ "vehicle_id": new })).await["warranty"].clone();
    assert_eq!(w["possibly_covered"], true, "{w}");
    assert_eq!(w["in_force"], true, "{w}");
    assert_eq!(w["date_passed"], false);
    assert_eq!(w["miles_left"], 10_000);

    let due = t.call("whats_coming", json!({ "vehicle_id": bare })).await;
    assert!(due["warranty"].is_null(), "{due}");
}
