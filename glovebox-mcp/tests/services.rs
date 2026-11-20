mod common;

use common::{
    connect, minimal_service, new_vehicle, reminder_status, schedule_item_every_12_months,
};
use glovebox_shared::{
    inputs::vehicle::NewVehicle,
    services::{schedule as schedule_svc, service_record as svc_svc, vehicle},
    test_support::days_ago,
};
use serde_json::json;

#[tokio::test]
async fn record_service_then_summarize_recent_activity_round_trip() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();

    let recorded = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-06-01",
                "description": "Oil change + filter",
                "mileage": 48_000,
                "total_cost_cents": 8_999,
                "line_items": [
                    { "description": "5W-30 oil 5qt", "work_kind": "maintenance", "cost_cents": 4_500 }
                ]
            }),
        )
        .await;
    assert!(recorded.to_string().contains("Oil change + filter"));

    let feed = t
        .call("summarize_recent_activity", json!({ "vehicle_id": v.id }))
        .await
        .to_string();
    assert!(
        feed.contains("Oil change + filter") && feed.contains("48000"),
        "activity feed must include the recorded service; got: {feed}"
    );
}

#[tokio::test]
async fn taking_a_job_off_a_service_as_not_done_brings_its_reminder_back() {
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
    let item = schedule_svc::create(
        &t.db,
        schedule_item_every_12_months(v.id, "Brake fluid flush"),
    )
    .await
    .unwrap();
    let job = t
        .call(
            "plan_work",
            json!({ "vehicle_id": v.id, "title": "Flush brake fluid", "schedule_item_id": item.id }),
        )
        .await;

    let service = t
        .call(
            "record_service",
            json!({ "vehicle_id": v.id, "service_date": days_ago(0), "work_item_ids": [job["id"]] }),
        )
        .await;
    assert_eq!(service["schedule_item_ids"], json!([item.id]));
    assert_eq!(reminder_status(&t, v.id, "Brake fluid flush").await, "ok");

    let fixed = t
        .call(
            "correct_service",
            json!({
                "vehicle_id": v.id,
                "service_id": service["id"],
                "work_item_ids": [],
                "completed_work": "did_not_happen"
            }),
        )
        .await;
    assert_eq!(fixed["schedule_item_ids"], json!([]));
    assert_eq!(fixed["released_schedule_item_ids"], json!([item.id]));
    assert_eq!(
        reminder_status(&t, v.id, "Brake fluid flush").await,
        "overdue"
    );

    let again = t
        .call(
            "correct_service",
            json!({ "vehicle_id": v.id, "service_id": service["id"], "notes": "Checked" }),
        )
        .await;
    assert!(again.get("released_schedule_item_ids").is_none(), "{again}");
}

#[tokio::test]
async fn deleting_a_service_lists_the_schedule_items_that_stopped_counting() {
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
            json!({ "vehicle_id": v.id, "title": "Flush brake fluid", "schedule_item_id": item.id }),
        )
        .await;
    let service = t
        .call(
            "record_service",
            json!({ "vehicle_id": v.id, "service_date": days_ago(0), "work_item_ids": [job["id"]] }),
        )
        .await;

    let deleted = t
        .call(
            "delete_service",
            json!({ "vehicle_id": v.id, "service_id": service["id"], "completed_work": "did_not_happen" }),
        )
        .await;
    assert_eq!(deleted["schedule_items_released"], json!([item.id]));
}

#[tokio::test]
async fn record_service_schedule_item_ids_clears_the_overdue_reminder() {
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
    let item = schedule_svc::create(
        &t.db,
        schedule_item_every_12_months(v.id, "Brake fluid flush"),
    )
    .await
    .unwrap();

    // Never serviced since a 2020 purchase, so the 12-month item is overdue.
    assert_eq!(
        reminder_status(&t, v.id, "Brake fluid flush").await,
        "overdue"
    );

    // Dated today, so the 12-month interval has only just restarted on
    // whatever day this runs.
    t.call(
        "record_service",
        json!({
            "vehicle_id": v.id,
            "service_date": days_ago(0),
            "description": "Brake fluid flush",
            "schedule_item_ids": [item.id]
        }),
    )
    .await;
    assert_eq!(reminder_status(&t, v.id, "Brake fluid flush").await, "ok");

    // Linking another vehicle's schedule item is a clean not-found tool
    // error, and nothing is recorded.
    let other = vehicle::create(&t.db, new_vehicle("Other")).await.unwrap();
    let err = t
        .call_err(
            "record_service",
            json!({
                "vehicle_id": other.id,
                "service_date": "2026-06-20",
                "schedule_item_ids": [item.id]
            }),
        )
        .await;
    assert!(err.contains("not found"), "got: {err}");
    assert!(
        svc_svc::list(&t.db, other.id).await.unwrap().is_empty(),
        "rejected link must not create a record"
    );
}

#[tokio::test]
async fn record_service_payer_flows_into_cost_summary_split() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();

    // A $100 self-paid service (payer omitted, so it defaults to self).
    t.call(
        "record_service",
        json!({
            "vehicle_id": v.id,
            "service_date": "2026-06-01",
            "description": "Brakes",
            "total_cost_cents": 10_000
        }),
    )
    .await;

    // A $150 insurance-paid repair.
    let repair = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-06-15",
                "description": "Collision repair",
                "total_cost_cents": 15_000,
                "paid_by": "insurance",
                "payer_note": "Progressive claim #12345"
            }),
        )
        .await
        .to_string();
    assert!(
        repair.contains("insurance") && repair.contains("Progressive claim #12345"),
        "record_service must echo the payer fields; got: {repair}"
    );

    let summary = t.call("cost_summary", json!({ "vehicle_id": v.id })).await;
    assert_eq!(summary["out_of_pocket_cents"], 10_000);
    assert_eq!(summary["covered_cents"], 15_000);
    assert_eq!(summary["total_cost_cents"], 25_000);

    // An unknown payer is a tool-level error, not a protocol failure.
    let err = t
        .call_err(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-06-20",
                "description": "Bogus payer",
                "paid_by": "my neighbor"
            }),
        )
        .await;
    assert!(
        err.contains("paid_by: unknown variant") && err.contains("third_party"),
        "the error must name the argument and list the valid payers; got: {err}"
    );
}

// Re-recording the same invoice_ref returns the same record, so a retry loop
// cannot create duplicate service records.
#[tokio::test]
async fn record_service_same_invoice_ref_is_idempotent() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();

    let args = json!({
        "vehicle_id": v.id,
        "service_date": "2026-06-01",
        "description": "Oil change",
        "total_cost_cents": 8_999,
        "invoice_ref": "FCP-2026-06-1234"
    });

    let first = t.call("record_service", args.clone()).await;
    let second = t.call("record_service", args).await;
    assert_eq!(second["id"], first["id"], "same invoice_ref, same record");
    assert_eq!(
        svc_svc::list(&t.db, v.id).await.unwrap().len(),
        1,
        "no duplicate record was created"
    );
}

#[tokio::test]
async fn link_service_to_maintenance_add_defaults_and_replace_overwrites() {
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
    let oil = schedule_svc::create(&t.db, schedule_item_every_12_months(v.id, "Oil change"))
        .await
        .unwrap();
    let brake = schedule_svc::create(&t.db, schedule_item_every_12_months(v.id, "Brake fluid"))
        .await
        .unwrap();

    // An imported record, created without links, leaves the oil reminder
    // overdue. It is dated today so that linking it reads `ok` on whatever
    // day this runs.
    let imported = svc_svc::create(
        &t.db,
        v.id,
        minimal_service(&days_ago(0), "Oil + brake fluid service"),
    )
    .await
    .unwrap();
    assert_eq!(reminder_status(&t, v.id, "Oil change").await, "overdue");

    // Default mode is add: link the oil item.
    t.call(
        "link_service_to_maintenance",
        json!({
            "vehicle_id": v.id,
            "service_record_id": imported.record.id,
            "schedule_item_ids": [oil.id]
        }),
    )
    .await;
    // Then the brake item in a second pass; the first link survives.
    let payload = t
        .call(
            "link_service_to_maintenance",
            json!({
                "vehicle_id": v.id,
                "service_record_id": imported.record.id,
                "schedule_item_ids": [brake.id],
                "mode": "add"
            }),
        )
        .await;
    let mut ids: Vec<i64> = payload["schedule_item_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_i64().unwrap())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, vec![i64::from(oil.id), i64::from(brake.id)]);

    assert_eq!(reminder_status(&t, v.id, "Oil change").await, "ok");

    // Replace mode overwrites the set.
    let payload = t
        .call(
            "link_service_to_maintenance",
            json!({
                "vehicle_id": v.id,
                "service_record_id": imported.record.id,
                "schedule_item_ids": [brake.id],
                "mode": "replace"
            }),
        )
        .await;
    assert_eq!(
        payload["schedule_item_ids"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(payload["schedule_item_ids"][0], brake.id);

    // An unknown mode is an actionable tool error.
    let err = t
        .call_err(
            "link_service_to_maintenance",
            json!({
                "vehicle_id": v.id,
                "service_record_id": imported.record.id,
                "schedule_item_ids": [oil.id],
                "mode": "merge"
            }),
        )
        .await;
    assert!(
        err.contains("mode: unknown variant `merge`, expected `add` or `replace`"),
        "got: {err}"
    );
}

#[tokio::test]
async fn link_service_to_maintenance_wrong_parent_both_ways() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Mine")).await.unwrap();
    let other = vehicle::create(&t.db, new_vehicle("Other")).await.unwrap();
    let mine_item = schedule_svc::create(&t.db, schedule_item_every_12_months(v.id, "Oil"))
        .await
        .unwrap();
    let mine_svc = svc_svc::create(&t.db, v.id, minimal_service("2026-06-01", "Mine"))
        .await
        .unwrap();
    let foreign_item = schedule_svc::create(&t.db, schedule_item_every_12_months(other.id, "Oil"))
        .await
        .unwrap();
    let foreign_svc = svc_svc::create(&t.db, other.id, minimal_service("2026-06-01", "Theirs"))
        .await
        .unwrap();

    // Another vehicle's service record reads as missing.
    let err = t
        .call_err(
            "link_service_to_maintenance",
            json!({
                "vehicle_id": v.id,
                "service_record_id": foreign_svc.record.id,
                "schedule_item_ids": [mine_item.id]
            }),
        )
        .await;
    assert!(
        err.contains(&format!(
            "Service record {} not found",
            foreign_svc.record.id
        )),
        "got: {err}"
    );

    // Another vehicle's schedule item reads as missing; nothing was linked.
    let err = t
        .call_err(
            "link_service_to_maintenance",
            json!({
                "vehicle_id": v.id,
                "service_record_id": mine_svc.record.id,
                "schedule_item_ids": [foreign_item.id]
            }),
        )
        .await;
    assert!(
        err.contains(&format!("Schedule item {} not found", foreign_item.id)),
        "got: {err}"
    );
    let fetched = svc_svc::get(&t.db, v.id, mine_svc.record.id).await.unwrap();
    glovebox_shared::test_support::assert_empty(&fetched.schedule_item_ids);
}

#[tokio::test]
async fn record_service_files_a_multi_line_invoice_in_one_call() {
    let c = connect().await;
    let v = vehicle::create(&c.db, new_vehicle("Daily"))
        .await
        .unwrap()
        .id;
    let out = c
        .call(
            "record_service",
            json!({
                "vehicle_id": v, "service_date": "2025-03-24", "end_date": "2025-03-25",
                "performed_by": "shop", "shop_name": "Example VW", "mileage": 53010,
                "invoice_ref": "RO-1182", "total_cost_cents": 48250,
                "line_items": [
                    {"description": "Brake fluid flush", "work_kind": "maintenance", "cost_cents": 14900},
                    {"description": "Install owner-supplied pads", "work_kind": "repair", "cost_cents": 33350}],
                "parts": [{"name": "Front brake pads", "seller": "FCP Euro", "supplied_by": "self",
                           "cost_cents": 8900}]
            }),
        )
        .await;
    assert_eq!(out["performed_by"], "shop");
    assert_eq!(out["end_date"], "2025-03-25");
    assert_eq!(out["parts"][0]["supplied_by"], "self");
    assert_eq!(out["parts"][0]["installed_odometer"], 53010);
    assert_eq!(out["line_items"][1]["work_kind"], "repair");
    assert!(out.get("already_recorded").is_none(), "got: {out}");
    assert!(out.get("message").is_none(), "got: {out}");

    let read = c
        .call(
            "get_service",
            json!({"vehicle_id": v, "service_id": out["id"]}),
        )
        .await;
    assert!(read.get("already_recorded").is_none(), "got: {read}");
    assert!(read.get("message").is_none(), "got: {read}");

    let again = c
        .call(
            "record_service",
            json!({"vehicle_id": v, "service_date": "2025-03-24", "invoice_ref": "RO-1182"}),
        )
        .await;
    assert_eq!(again["already_recorded"], true);
    assert_eq!(again["id"], out["id"]);
    let id = out["id"].as_i64().unwrap();
    assert_eq!(
        again["message"],
        format!(
            "This invoice number is already recorded as service {id}; nothing in this call was \
             applied. Use correct_service to change it."
        )
    );

    // Different values sent with the same invoice number are ignored, and the message says so.
    let refiled = c
        .call(
            "record_service",
            json!({"vehicle_id": v, "service_date": "2025-03-24", "invoice_ref": "RO-1182",
                   "total_cost_cents": 99_900}),
        )
        .await;
    assert_eq!(refiled["total_cost_cents"], 48_250);
    assert!(
        refiled["message"]
            .as_str()
            .unwrap()
            .contains("nothing in this call")
    );
}

#[tokio::test]
async fn record_service_rejects_a_bad_work_kind_or_performer_with_the_argument_path() {
    let c = connect().await;
    let v = vehicle::create(&c.db, new_vehicle("Daily"))
        .await
        .unwrap()
        .id;
    // A bad value inside a line names the line and the argument.
    let err = c
        .call_err(
            "record_service",
            json!({"vehicle_id": v, "service_date": "2025-03-24",
                   "line_items": [{"description": "Oil", "work_kind": "maintenance"},
                                  {"description": "Tune", "work_kind": "upgrade"}]}),
        )
        .await;
    assert!(
        err.contains("line_items[1].work_kind") && err.contains("`mod`"),
        "got: {err}"
    );
    let err = c
        .call_err(
            "record_service",
            json!({"vehicle_id": v, "service_date": "2025-03-24", "performed_by": "dealer"}),
        )
        .await;
    assert!(
        err.contains("performed_by: unknown variant") && err.contains("`shop`"),
        "got: {err}"
    );
    let listed = c.call("list_services", json!({ "vehicle_id": v })).await;
    assert_eq!(listed, json!([]), "a refused call records nothing");
}

#[tokio::test]
async fn correct_service_refuses_a_value_outside_a_list_and_changes_nothing() {
    let c = connect().await;
    let v = vehicle::create(&c.db, new_vehicle("Daily"))
        .await
        .unwrap()
        .id;
    let recorded = c
        .call(
            "record_service",
            json!({"vehicle_id": v, "service_date": "2025-03-24", "paid_by": "insurance",
                   "line_items": [{"description": "Bodywork", "work_kind": "repair"}]}),
        )
        .await;
    let sid = recorded["id"].clone();

    for (arguments, path) in [
        (json!({"paid_by": "bogus", "notes": "x"}), "paid_by"),
        (
            json!({"performed_by": "dealer", "notes": "x"}),
            "performed_by",
        ),
        (
            json!({"notes": "x",
                   "line_items": [{"description": "Tune", "work_kind": "upgrade"}]}),
            "line_items[0].work_kind",
        ),
    ] {
        let mut arguments = arguments;
        arguments["vehicle_id"] = json!(v);
        arguments["service_id"] = sid.clone();
        let err = c.call_err("correct_service", arguments).await;
        assert!(
            err.contains(&format!("{path}: unknown variant")),
            "got: {err}"
        );
    }

    let read = c
        .call("get_service", json!({"vehicle_id": v, "service_id": sid}))
        .await;
    assert_eq!(read["paid_by"], "insurance");
    assert_eq!(read["performed_by"], "shop");
    assert!(read["notes"].is_null(), "got: {read}");
    assert_eq!(read["line_items"][0]["work_kind"], "repair");
}

#[tokio::test]
async fn correct_service_changes_the_cost_and_replaces_the_lines() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let recorded = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-06-01",
                "description": "Brakes",
                "notes": "rotors warped",
                "total_cost_cents": 48_250,
                "line_items": [
                    { "description": "Pads", "work_kind": "maintenance", "cost_cents": 20_000 }
                ]
            }),
        )
        .await;
    let sid = recorded["id"].clone();

    let corrected = t
        .call(
            "correct_service",
            json!({
                "vehicle_id": v.id,
                "service_id": sid,
                "total_cost_cents": 48_520,
                "notes": null,
                "line_items": [
                    { "description": "Pads", "work_kind": "maintenance", "cost_cents": 20_000 },
                    { "description": "Rotors", "work_kind": "repair", "cost_cents": 28_520 }
                ]
            }),
        )
        .await;
    assert_eq!(corrected["total_cost_cents"], 48_520);
    assert!(
        corrected["notes"].is_null(),
        "null clears notes: {corrected}"
    );
    assert_eq!(corrected["description"], "Brakes", "absent keys stay put");
    assert_eq!(corrected["line_items"].as_array().map(Vec::len), Some(2));

    let listed = t
        .call(
            "list_services",
            json!({ "vehicle_id": v.id, "from": "2026-01-01", "to": "2026-12-31" }),
        )
        .await;
    assert_eq!(listed.as_array().map(Vec::len), Some(1));
    let outside = t
        .call(
            "list_services",
            json!({ "vehicle_id": v.id, "from": "2027-01-01" }),
        )
        .await;
    assert_eq!(outside, json!([]));

    let other = vehicle::create(&t.db, new_vehicle("Other")).await.unwrap();
    let wrong = t
        .call_err(
            "correct_service",
            json!({ "vehicle_id": other.id, "service_id": sid, "notes": "x" }),
        )
        .await;
    let missing = t
        .call_err(
            "correct_service",
            json!({ "vehicle_id": other.id, "service_id": 99999, "notes": "x" }),
        )
        .await;
    assert_eq!(
        wrong.replace(&sid.to_string(), "N"),
        missing.replace("99999", "N")
    );

    let wrong_delete = t
        .call_err(
            "delete_service",
            json!({ "vehicle_id": other.id, "service_id": sid }),
        )
        .await;
    assert_eq!(
        wrong_delete.replace(&sid.to_string(), "N"),
        t.call_err(
            "delete_service",
            json!({ "vehicle_id": other.id, "service_id": 99999 })
        )
        .await
        .replace("99999", "N")
    );

    t.call(
        "delete_service",
        json!({ "vehicle_id": v.id, "service_id": sid }),
    )
    .await;
    let gone = t
        .call_err(
            "get_service",
            json!({ "vehicle_id": v.id, "service_id": sid }),
        )
        .await;
    assert!(gone.contains("not found"), "got: {gone}");
}

#[tokio::test]
async fn delete_service_done_another_way_keeps_the_incident_and_work_done() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let incident = t
        .call(
            "log_incident",
            json!({ "vehicle_id": v.id, "title": "Squeak", "category": "noise" }),
        )
        .await;
    let work = t
        .call(
            "plan_work",
            json!({ "vehicle_id": v.id, "title": "Fix squeak" }),
        )
        .await;
    let recorded = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-06-01",
                "resolved_incident_ids": [incident["id"]],
                "work_item_ids": [work["id"]]
            }),
        )
        .await;
    let args = json!({ "vehicle_id": v.id, "service_id": recorded["id"] });
    let refused = t.call_err("delete_service", args.clone()).await;
    assert!(
        refused.contains("\"Fix squeak\"") && refused.contains("\"Squeak\""),
        "got: {refused}"
    );

    t.call(
        "delete_service",
        json!({ "vehicle_id": v.id, "service_id": recorded["id"], "completed_work": "done_another_way" }),
    )
    .await;

    let kept = t
        .call(
            "get_incident",
            json!({ "vehicle_id": v.id, "incident_id": incident["id"] }),
        )
        .await;
    assert_eq!(kept["resolved"], true);
    let work = t
        .call(
            "list_planned_work",
            json!({ "vehicle_id": v.id, "include_done": true }),
        )
        .await;
    assert_eq!(work["items"][0]["status"], "done", "got: {work}");
}

#[tokio::test]
async fn delete_service_reopens_the_incident_and_work_it_closed() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let incident = t
        .call(
            "log_incident",
            json!({ "vehicle_id": v.id, "title": "Squeak", "category": "noise" }),
        )
        .await;
    let work = t
        .call(
            "plan_work",
            json!({ "vehicle_id": v.id, "title": "Fix squeak" }),
        )
        .await;
    let recorded = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-06-01",
                "resolved_incident_ids": [incident["id"]],
                "work_item_ids": [work["id"]]
            }),
        )
        .await;
    let fixed = t
        .call(
            "get_incident",
            json!({ "vehicle_id": v.id, "incident_id": incident["id"] }),
        )
        .await;
    assert_eq!(fixed["resolved"], true);

    let deleted = t
        .call(
            "delete_service",
            json!({ "vehicle_id": v.id, "service_id": recorded["id"], "completed_work": "did_not_happen" }),
        )
        .await;
    assert_eq!(deleted["appointments_reopened"], json!([]));
    assert_eq!(deleted["jobs_unassigned"], json!([]));

    let reopened = t
        .call(
            "get_incident",
            json!({ "vehicle_id": v.id, "incident_id": incident["id"] }),
        )
        .await;
    assert_eq!(reopened["resolved"], false);
    let planned = t
        .call("list_planned_work", json!({ "vehicle_id": v.id }))
        .await;
    assert_eq!(planned["items"][0]["status"], "planned");
}

#[tokio::test]
async fn delete_service_names_the_appointments_it_moves() {
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
        jobs.push((appt, job, status));
    }
    let recorded = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-06-01",
                "work_item_ids": [jobs[0].1, jobs[1].1]
            }),
        )
        .await;
    for (appt, _, status) in &jobs {
        t.call(
            "update_appointment",
            json!({ "vehicle_id": v.id, "appointment_id": appt, "status": status }),
        )
        .await;
    }

    let deleted = t
        .call(
            "delete_service",
            json!({ "vehicle_id": v.id, "service_id": recorded["id"], "completed_work": "did_not_happen" }),
        )
        .await;
    assert_eq!(deleted["appointments_reopened"], json!([jobs[0].0]));
    assert_eq!(deleted["jobs_unassigned"], json!([jobs[1].1]));
}

#[tokio::test]
async fn services_tools_reject_unknown_arguments() {
    let t = connect().await;
    common::assert_unknown_field_rejected(
        &t,
        "correct_service",
        json!({ "vehicle_id": 1, "service_id": 1, "recall_ids": [1] }),
        "recall_ids",
    )
    .await;
    common::assert_unknown_field_rejected(&t, "record_service", json!({ "vehicle_id": 1, "service_date": "2026-11-10", "line_items": [{ "description": "Oil", "work_kind": "maintenance", "category": "x" }] }), "category").await;
    common::assert_unknown_field_rejected(&t, "record_service", json!({ "vehicle_id": 1, "service_date": "2026-11-10", "parts": [{ "name": "Filter", "brand": "Mann" }] }), "brand").await;
    common::assert_unknown_field_rejected(&t, "record_service", json!({ "vehicle_id": 1, "service_date": "2026-11-10", "parts": [{ "name": "Kit", "contents": [{ "name": "Filter", "sku": "x" }] }] }), "sku").await;
}

#[tokio::test]
async fn correct_service_sets_changes_and_clears_the_invoice_ref() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let first = t
        .call(
            "record_service",
            json!({ "vehicle_id": v.id, "service_date": "2026-01-10", "invoice_ref": "RO-1" }),
        )
        .await;
    let second = t
        .call(
            "record_service",
            json!({ "vehicle_id": v.id, "service_date": "2026-02-10" }),
        )
        .await;

    let clash = t
        .call_err(
            "correct_service",
            json!({ "vehicle_id": v.id, "service_id": second["id"], "invoice_ref": " RO-1 " }),
        )
        .await;
    assert!(
        clash.contains(&format!("service {}", first["id"])) && clash.contains("RO-1"),
        "a clash names the other service; got: {clash}"
    );

    let set = t
        .call(
            "correct_service",
            json!({ "vehicle_id": v.id, "service_id": second["id"], "invoice_ref": "RO-2" }),
        )
        .await;
    assert_eq!(set["invoice_ref"], "RO-2");
    let cleared = t
        .call(
            "correct_service",
            json!({ "vehicle_id": v.id, "service_id": second["id"], "invoice_ref": null }),
        )
        .await;
    assert!(cleared["invoice_ref"].is_null(), "got: {cleared}");
    // Keeping a service's own ref is not a clash.
    t.call(
        "correct_service",
        json!({ "vehicle_id": v.id, "service_id": first["id"], "invoice_ref": "RO-1" }),
    )
    .await;
}

#[tokio::test]
async fn correct_service_moves_the_jobs_a_service_completed() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let plan = |title: &'static str| {
        let t = &t;
        async move {
            t.call("plan_work", json!({ "vehicle_id": v.id, "title": title }))
                .await["id"]
                .clone()
        }
    };
    let wrong = plan("Rear wiper").await;
    let right = plan("Front wiper").await;
    let service = t
        .call(
            "record_service",
            json!({ "vehicle_id": v.id, "service_date": "2026-11-01", "work_item_ids": [wrong] }),
        )
        .await;

    let err = t
        .call_err(
            "correct_service",
            json!({ "vehicle_id": v.id, "service_id": service["id"], "work_item_ids": [right] }),
        )
        .await;
    assert!(
        err.contains("Rear wiper") && err.contains("Nothing was changed"),
        "got: {err}"
    );

    let fixed = t
        .call(
            "correct_service",
            json!({
                "vehicle_id": v.id, "service_id": service["id"], "work_item_ids": [right],
                "completed_work": "did_not_happen"
            }),
        )
        .await;
    assert_eq!(
        fixed["completed_work_items"][0]["id"], right,
        "got: {fixed}"
    );
    let planned = t
        .call("list_planned_work", json!({ "vehicle_id": v.id }))
        .await;
    let titles: Vec<&serde_json::Value> = planned["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| &j["title"])
        .collect();
    assert_eq!(titles, [&json!("Rear wiper")], "got: {planned}");
}

#[tokio::test]
async fn correct_service_replaces_the_recalls_it_performed() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let wrong = common::seed_recall(&t.db, v.id, "24V110000").await;
    let right = common::seed_recall(&t.db, v.id, "24V220000").await;
    let service = t
        .call(
            "record_service",
            json!({ "vehicle_id": v.id, "service_date": "2026-03-01", "recall_finding_ids": [wrong] }),
        )
        .await;

    let fixed = t
        .call(
            "correct_service",
            json!({ "vehicle_id": v.id, "service_id": service["id"], "recall_finding_ids": [right], "completed_work": "did_not_happen" }),
        )
        .await;
    let performed: Vec<&serde_json::Value> = fixed["performed_recalls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| &r["finding_id"])
        .collect();
    assert_eq!(performed, [&json!(right)]);
    // The recall no longer named reads as open again.
    let coming = t.call("whats_coming", json!({ "vehicle_id": v.id })).await;
    let open: Vec<&serde_json::Value> = coming["recalls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| &r["finding_id"])
        .collect();
    assert_eq!(open, [&json!(wrong)], "got: {coming}");

    let other = vehicle::create(&t.db, new_vehicle("Other")).await.unwrap();
    let foreign = common::seed_recall(&t.db, other.id, "24V330000").await;
    let wrong_vehicle = t
        .call_err(
            "correct_service",
            json!({ "vehicle_id": v.id, "service_id": service["id"], "recall_finding_ids": [foreign] }),
        )
        .await;
    let missing = t
        .call_err(
            "correct_service",
            json!({ "vehicle_id": v.id, "service_id": service["id"], "recall_finding_ids": [99_999] }),
        )
        .await;
    assert_eq!(
        wrong_vehicle.replace(&foreign.to_string(), "N"),
        missing.replace("99999", "N")
    );
}

#[tokio::test]
async fn correct_service_keeps_the_recall_of_a_job_the_service_completed() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let recall = common::seed_recall(&t.db, v.id, "24V110000").await;
    let job = t
        .call(
            "plan_work",
            json!({ "vehicle_id": v.id, "title": "Recall", "research_finding_id": recall }),
        )
        .await;
    let service = t
        .call(
            "record_service",
            json!({ "vehicle_id": v.id, "service_date": "2026-03-01", "work_item_ids": [job["id"]] }),
        )
        .await;

    // The service keeps the job it completed, so the call needs no `completed_work`.
    let fixed = t
        .call(
            "correct_service",
            json!({ "vehicle_id": v.id, "service_id": service["id"], "recall_finding_ids": [] }),
        )
        .await;
    assert_eq!(fixed["performed_recalls"][0]["finding_id"], recall);
    let coming = t.call("whats_coming", json!({ "vehicle_id": v.id })).await;
    assert!(
        coming["recalls"].as_array().unwrap().is_empty(),
        "got: {coming}"
    );
}

#[tokio::test]
async fn correct_service_keeps_the_recall_of_a_job_it_lists() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let recall = common::seed_recall(&t.db, v.id, "24V110000").await;
    let job = t
        .call(
            "plan_work",
            json!({ "vehicle_id": v.id, "title": "Recall", "research_finding_id": recall }),
        )
        .await;
    let service = t
        .call(
            "record_service",
            json!({ "vehicle_id": v.id, "service_date": "2026-03-01", "recall_finding_ids": [recall] }),
        )
        .await;

    // The listed job brings the recall back, so the call needs no `completed_work`.
    let fixed = t
        .call(
            "correct_service",
            json!({ "vehicle_id": v.id, "service_id": service["id"], "recall_finding_ids": [], "work_item_ids": [job["id"]] }),
        )
        .await;
    assert_eq!(fixed["performed_recalls"][0]["finding_id"], recall);
    let coming = t.call("whats_coming", json!({ "vehicle_id": v.id })).await;
    assert!(
        coming["recalls"].as_array().unwrap().is_empty(),
        "got: {coming}"
    );
}

#[tokio::test]
async fn correcting_a_service_moves_its_parts_and_a_shop_part_is_not_charged_twice() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let service = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2024-03-01",
                "mileage": 50_000,
                "total_cost_cents": 20_000,
                "parts": [
                    { "name": "Spark plug set", "supplied_by": "shop", "cost_cents": 6_000 },
                    { "name": "Front pads", "supplied_by": "self", "cost_cents": 9_000 }
                ]
            }),
        )
        .await;

    // The shop's part is inside the invoice total; the owner's pads are not.
    let costs = t.call("cost_summary", json!({ "vehicle_id": v.id })).await;
    assert_eq!(costs["total_cost_cents"], 29_000, "got: {costs}");

    let corrected = t
        .call(
            "correct_service",
            json!({
                "vehicle_id": v.id,
                "service_id": service["id"],
                "service_date": "2024-03-10",
                "mileage": 50_500
            }),
        )
        .await;
    for part in corrected["parts"].as_array().expect("parts") {
        assert_eq!(part["installed_date"], "2024-03-10", "got: {part}");
        assert_eq!(part["installed_odometer"], 50_500, "got: {part}");
    }
}

#[tokio::test]
async fn record_service_installs_parts_recorded_earlier() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let pads = t
        .call(
            "record_part",
            json!({ "vehicle_id": v.id, "name": "Front pads", "seller": "FCP Euro" }),
        )
        .await;
    let service = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-04-01",
                "mileage": 50_000,
                "part_ids": [pads["id"]]
            }),
        )
        .await;
    assert_eq!(service["part_ids"], json!([pads["id"]]));
    assert_eq!(service["parts"][0]["status"], "installed");
    assert_eq!(service["parts"][0]["installed_odometer"], 50_000);

    let other = vehicle::create(&t.db, new_vehicle("Other")).await.unwrap();
    let foreign = t
        .call(
            "record_part",
            json!({ "vehicle_id": other.id, "name": "Rotor" }),
        )
        .await;
    let wrong_vehicle = t
        .call_err(
            "record_service",
            json!({ "vehicle_id": v.id, "service_date": "2026-04-02", "part_ids": [foreign["id"]] }),
        )
        .await;
    let missing = t
        .call_err(
            "record_service",
            json!({ "vehicle_id": v.id, "service_date": "2026-04-02", "part_ids": [99_999] }),
        )
        .await;
    assert_eq!(
        wrong_vehicle.replace(&foreign["id"].to_string(), "N"),
        missing.replace("99999", "N")
    );

    // Deleting the service keeps the part and says which parts were kept.
    let deleted = t
        .call(
            "delete_service",
            json!({ "vehicle_id": v.id, "service_id": service["id"], "completed_work": "did_not_happen" }),
        )
        .await;
    assert_eq!(deleted["kept_part_ids"], json!([pads["id"]]));
    let parts = t.call("list_parts", json!({ "vehicle_id": v.id })).await;
    assert_eq!(parts[0]["status"], "purchased", "got: {parts}");
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "one case per date field")]
async fn malformed_dates_are_refused_naming_the_field() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let id = v.id;
    let service = t
        .call(
            "record_service",
            json!({ "vehicle_id": id, "service_date": "2026-11-10" }),
        )
        .await;
    let cases = [
        (
            "record_service",
            json!({ "vehicle_id": id, "service_date": "11/10/2026" }),
            "service_date",
        ),
        (
            "record_service",
            json!({ "vehicle_id": id, "service_date": "2026-1-5" }),
            "service_date",
        ),
        (
            "record_service",
            json!({ "vehicle_id": id, "service_date": "2026-11-10", "end_date": "2026-11-31" }),
            "end_date",
        ),
        (
            "correct_service",
            json!({ "vehicle_id": id, "service_id": service["id"], "service_date": "yesterday" }),
            "service_date",
        ),
        (
            "list_services",
            json!({ "vehicle_id": id, "from": "2026" }),
            "from",
        ),
        (
            "record_part",
            json!({ "vehicle_id": id, "name": "Pads", "purchase_date": "2026-13-01" }),
            "purchase_date",
        ),
        (
            "update_vehicle",
            json!({ "vehicle_id": id, "warranty_expires_on": "June 2027" }),
            "warranty_expires_on",
        ),
        (
            "log_incident",
            json!({ "vehicle_id": id, "title": "Squeak", "occurred_at": "last week" }),
            "occurred_at",
        ),
        (
            "log_mileage",
            json!({ "vehicle_id": id, "mileage": 1, "recorded_at": "2026-11-10 25:00:00" }),
            "recorded_at",
        ),
        (
            "list_services",
            json!({ "vehicle_id": id, "to": "2026-02-30" }),
            "to",
        ),
        (
            "record_service",
            json!({
                "vehicle_id": id,
                "service_date": "2026-11-10",
                "parts": [{ "name": "Pads", "warranty_expires_on": "2030" }]
            }),
            "warranty_expires_on",
        ),
        (
            "correct_service",
            json!({ "vehicle_id": id, "service_id": service["id"], "end_date": "soon" }),
            "end_date",
        ),
        (
            "add_vehicle",
            json!({ "name": "Other", "purchase_date": "3/4/2020" }),
            "purchase_date",
        ),
        (
            "update_vehicle",
            json!({ "vehicle_id": id, "sold_date": "2026-1-1" }),
            "sold_date",
        ),
        (
            "record_part",
            json!({ "vehicle_id": id, "name": "Pads", "installed_date": "today" }),
            "installed_date",
        ),
        (
            "correct_part",
            json!({ "vehicle_id": id, "part_id": 999, "replaced_date": "2026-11-31" }),
            "replaced_date",
        ),
        (
            "correct_part",
            json!({ "vehicle_id": id, "part_id": 999, "received_on": "yesterday" }),
            "received_on",
        ),
        (
            "update_incident",
            json!({ "vehicle_id": id, "incident_id": 999, "occurred_at": "2026-11-10 8:30" }),
            "occurred_at",
        ),
        (
            "add_incident_followup",
            json!({
                "vehicle_id": id,
                "incident_id": 999,
                "occurred_at": "11/10",
                "summary": "Still squeaks"
            }),
            "occurred_at",
        ),
        (
            "plan_work",
            json!({ "vehicle_id": id, "title": "Brakes", "revisit_by_date": "soon" }),
            "revisit_by_date",
        ),
        (
            "update_planned_work",
            json!({ "vehicle_id": id, "work_item_id": 999, "revisit_by_date": "2027-5-1" }),
            "revisit_by_date",
        ),
        (
            "plan_appointment",
            json!({ "vehicle_id": id, "purpose": "60k", "planned_date": "2026-1-1" }),
            "planned_date",
        ),
        (
            "update_appointment",
            json!({ "vehicle_id": id, "appointment_id": 999, "planned_date": "next week" }),
            "planned_date",
        ),
        (
            "mark_need_ordered",
            json!({ "vehicle_id": id, "need_id": 999, "purchase_date": "11/5" }),
            "purchase_date",
        ),
        (
            "link_part_to_need",
            json!({ "vehicle_id": id, "need_id": 999, "part_id": 999, "received_on": "11/5" }),
            "received_on",
        ),
        (
            "mark_need_received",
            json!({ "vehicle_id": id, "need_id": 999, "received_on": "11/10" }),
            "received_on",
        ),
    ];
    for (tool, args, field) in cases {
        let err = t.call_err(tool, args).await;
        assert!(
            err.starts_with(&format!("{field}: ")) && err.contains("2027-05-01"),
            "{tool} must refuse a bad {field}; got: {err}"
        );
    }

    // A date-time field takes a bare date or a date and time.
    for occurred_at in ["2026-11-10", "2026-11-10 08:30:00"] {
        t.call(
            "log_incident",
            json!({ "vehicle_id": id, "title": "Squeak", "occurred_at": occurred_at }),
        )
        .await;
    }
}

#[tokio::test]
async fn delete_service_asks_what_happened_to_the_recalls_it_performed() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let recall = common::seed_recall(&t.db, v.id, "24V110000").await;
    let service = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2025-03-25",
                "recall_finding_ids": [recall]
            }),
        )
        .await;
    let args = json!({ "vehicle_id": v.id, "service_id": service["id"] });

    let refused = t.call_err("delete_service", args.clone()).await;
    assert!(refused.contains("24V110000"), "got: {refused}");
    assert!(refused.contains(&recall.to_string()), "got: {refused}");
    assert!(
        refused.contains("did not happen or was done another way"),
        "got: {refused}"
    );
    t.call("get_service", args.clone()).await;

    let invalid = t
        .call_err(
            "delete_service",
            json!({ "vehicle_id": v.id, "service_id": service["id"], "completed_work": "ignore" }),
        )
        .await;
    assert!(
        invalid.contains("did_not_happen") && invalid.contains("done_another_way"),
        "got: {invalid}"
    );
    t.call("get_service", args.clone()).await;

    let deleted = t
        .call(
            "delete_service",
            json!({ "vehicle_id": v.id, "service_id": service["id"], "completed_work": "done_another_way" }),
        )
        .await;
    assert_eq!(deleted["recalls_kept_done"], json!([recall]));
    assert_eq!(deleted["recalls_reopened"], json!([]));
    assert_eq!(deleted["recalls_still_done_elsewhere"], json!([]));
    let read = t
        .call(
            "set_recall_status",
            json!({ "vehicle_id": v.id, "finding_id": recall, "status": "done" }),
        )
        .await;
    assert_eq!(read["status"], "done");
    assert!(read["done_service"].is_null(), "got: {read}");
}

#[tokio::test]
async fn recall_and_fee_lines_and_a_manufacturer_payer_are_accepted() {
    let c = connect().await;
    let v = vehicle::create(&c.db, new_vehicle("Daily"))
        .await
        .unwrap()
        .id;
    let out = c
        .call(
            "record_service",
            json!({
                "vehicle_id": v, "service_date": "2025-03-24", "total_cost_cents": 0,
                "paid_by": "manufacturer",
                "line_items": [
                    {"description": "Fuel pump recall", "work_kind": "recall", "cost_cents": 0},
                    {"description": "Shop supplies", "work_kind": "fee", "cost_cents": 0}]
            }),
        )
        .await;
    assert_eq!(out["paid_by"], "manufacturer");
    assert_eq!(out["line_items"][0]["work_kind"], "recall");
    assert_eq!(out["line_items"][1]["work_kind"], "fee");

    let err = c
        .call_err(
            "record_service",
            json!({"vehicle_id": v, "service_date": "2025-03-25", "paid_by": "dealer"}),
        )
        .await;
    assert!(err.contains("manufacturer"), "got: {err}");
}

#[tokio::test]
async fn a_shop_part_is_not_added_to_the_invoice_total_but_a_self_part_is() {
    let c = connect().await;
    let v = vehicle::create(&c.db, new_vehicle("Daily"))
        .await
        .unwrap()
        .id;
    c.call(
        "record_service",
        json!({
            "vehicle_id": v, "service_date": "2025-03-24", "total_cost_cents": 56_900,
            "parts": [
                {"name": "Brake fluid", "supplied_by": "shop", "cost_cents": 2_200},
                {"name": "Pads", "supplied_by": "self", "cost_cents": 8_900,
                 "purchase_date": "2025-03-01"}]
        }),
    )
    .await;
    let summary = c.call("cost_summary", json!({ "vehicle_id": v })).await;
    assert_eq!(summary["service_totals_cents"], 56_900);
    assert_eq!(summary["self_supplied_parts_cents"], 8_900);
    assert_eq!(summary["total_cost_cents"], 65_800);
}

#[tokio::test]
async fn a_total_that_differs_from_its_lines_carries_a_note_and_a_fee_line_closes_it() {
    let c = connect().await;
    let v = vehicle::create(&c.db, new_vehicle("Daily"))
        .await
        .unwrap()
        .id;
    let out = c
        .call(
            "record_service",
            json!({
                "vehicle_id": v, "service_date": "2025-03-24", "total_cost_cents": 57_500,
                "line_items": [
                    {"description": "Brakes", "work_kind": "repair", "cost_cents": 56_900}]
            }),
        )
        .await;
    assert_eq!(out["lines_total_cents"], 56_900);
    let note = out["note"].as_str().expect("note");
    assert!(
        note.contains("57500")
            && note.contains("56900")
            && note.contains("600")
            && note.contains("fee"),
        "got: {note}"
    );
    assert!(out.get("message").is_none());

    let listed = c.call("list_services", json!({ "vehicle_id": v })).await;
    assert_eq!(listed[0]["lines_total_cents"], 56_900);

    let fixed = c
        .call(
            "correct_service",
            json!({
                "vehicle_id": v, "service_id": out["id"],
                "line_items": [
                    {"description": "Brakes", "work_kind": "repair", "cost_cents": 56_900},
                    {"description": "Shop supplies and tax", "work_kind": "fee", "cost_cents": 600}]
            }),
        )
        .await;
    assert_eq!(fixed["lines_total_cents"], 57_500);
    assert!(fixed.get("note").is_none(), "got: {fixed}");

    let bare = c
        .call(
            "record_service",
            json!({"vehicle_id": v, "service_date": "2025-04-01", "total_cost_cents": 100}),
        )
        .await;
    assert!(bare["lines_total_cents"].is_null());
    assert!(bare.get("note").is_none());
}

#[tokio::test]
async fn a_mixed_invoice_marks_the_warranty_line_and_cost_summary_splits_it() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let recorded = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-11-03",
                "description": "Dealer visit",
                "total_cost_cents": 12_000,
                "line_items": [
                    {"description": "Oil change", "work_kind": "maintenance", "cost_cents": 12_000},
                    {"description": "Replace failed water pump under warranty",
                     "work_kind": "repair", "paid_by": "manufacturer"}
                ]
            }),
        )
        .await;
    let service_id = recorded["id"].clone();

    let read = t
        .call(
            "get_service",
            json!({ "vehicle_id": v.id, "service_id": service_id }),
        )
        .await;
    assert!(read["line_items"][0]["paid_by"].is_null(), "got: {read}");
    assert_eq!(read["line_items"][1]["paid_by"], "manufacturer");
    assert_eq!(
        read["line_items"][0]["effective_paid_by"], "self",
        "got: {read}"
    );
    assert_eq!(read["line_items"][1]["effective_paid_by"], "manufacturer");

    let summary = t.call("cost_summary", json!({ "vehicle_id": v.id })).await;
    assert_eq!(summary["out_of_pocket_cents"], 12_000);
    assert_eq!(summary["covered_cents"], 0);
    assert_eq!(summary["covered_work_count"], 1);
    let work = &summary["covered_work"][0];
    assert_eq!(work["service_id"], service_id);
    assert_eq!(work["lines"][0]["paid_by"], "manufacturer");
    assert!(work["lines"][0]["cost_cents"].is_null());

    let err = t
        .call_err(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-11-04",
                "line_items": [
                    {"description": "x", "work_kind": "repair", "paid_by": "neighbor"}
                ]
            }),
        )
        .await;
    assert!(err.contains("paid_by"), "got: {err}");
}

#[tokio::test]
async fn the_worked_invoice_example_reports_14000_out_of_pocket_and_45000_covered() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let recorded = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-11-03",
                "total_cost_cents": 59_000,
                "line_items": [
                    {"description": "Water pump", "work_kind": "repair",
                     "cost_cents": 45_000, "paid_by": "manufacturer"},
                    {"description": "Oil change", "work_kind": "maintenance",
                     "cost_cents": 12_900},
                    {"description": "Shop fees", "work_kind": "fee", "cost_cents": 1_100}
                ]
            }),
        )
        .await;
    assert!(recorded.get("note").is_none(), "got: {recorded}");
    assert_eq!(recorded["lines_total_cents"], 59_000);

    let summary = t.call("cost_summary", json!({ "vehicle_id": v.id })).await;
    assert_eq!(summary["out_of_pocket_cents"], 14_000);
    assert_eq!(summary["covered_cents"], 45_000);
    let work = &summary["covered_work"][0];
    assert_eq!(work["out_of_pocket_cents"], 14_000);
    assert_eq!(work["covered_by_others_cents"], 45_000);
    assert!(work.get("total_cost_cents").is_none());
}

#[tokio::test]
async fn a_service_shows_each_installed_kits_contents() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let kit = t
        .call(
            "record_part",
            json!({
                "vehicle_id": v.id,
                "name": "Oil change kit",
                "contents": [{ "name": "Oil filter", "part_number": "HU 7020 z" }]
            }),
        )
        .await;
    let service = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-04-01",
                "part_ids": [kit["id"]]
            }),
        )
        .await;
    assert_eq!(
        service["parts"][0]["contents"][0]["part_number"], "HU 7020 z",
        "got: {service}"
    );

    let got = t
        .call(
            "get_service",
            json!({ "vehicle_id": v.id, "service_id": service["id"] }),
        )
        .await;
    assert_eq!(got["parts"][0]["contents"][0]["name"], "Oil filter");
    let listed = t.call("list_services", json!({ "vehicle_id": v.id })).await;
    assert_eq!(listed[0]["parts"][0]["contents"][0]["name"], "Oil filter");
}

#[tokio::test]
async fn an_inline_kit_on_an_invoice_records_its_contents() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();

    let service = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-04-01",
                "parts": [{
                    "name": "Oil change kit",
                    "supplied_by": "shop",
                    "contents": [
                        { "name": "Oil filter", "manufacturer": "Mann", "part_number": "HU 7020 z" },
                        { "name": "Engine oil, 1 L", "quantity": 5 }
                    ]
                }]
            }),
        )
        .await;

    let contents = &service["parts"][0]["contents"];
    assert_eq!(contents[0]["part_number"], "HU 7020 z", "got: {service}");
    assert_eq!(contents[1]["quantity"], 5);

    let refused = t
        .call_err(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-04-02",
                "parts": [
                    { "name": "Oil kit", "contents": [{ "name": "Filter" }] },
                    { "name": "Brake kit", "contents": [{ "name": " " }] }
                ]
            }),
        )
        .await;
    assert_eq!(refused, "parts: part 2's item 1 needs a name");
    let listed = t.call("list_services", json!({ "vehicle_id": v.id })).await;
    assert_eq!(
        listed.as_array().unwrap().len(),
        1,
        "a refused invoice records nothing"
    );
}

#[tokio::test]
async fn correct_service_leaves_an_incident_resolved_by_hand_resolved() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let incident = t
        .call(
            "log_incident",
            json!({ "vehicle_id": v.id, "title": "Leak" }),
        )
        .await;
    t.call(
        "update_incident",
        json!({ "vehicle_id": v.id, "incident_id": incident["id"], "resolved": true }),
    )
    .await;
    let job = t
        .call(
            "plan_work",
            json!({ "vehicle_id": v.id, "title": "Fix leak", "incident_id": incident["id"] }),
        )
        .await;
    let service = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-06-01",
                "work_item_ids": [job["id"]]
            }),
        )
        .await;

    let refused = t
        .call_err(
            "correct_service",
            json!({ "vehicle_id": v.id, "service_id": service["id"], "work_item_ids": [] }),
        )
        .await;
    assert!(!refused.contains("incident"), "got: {refused}");

    t.call(
        "correct_service",
        json!({
            "vehicle_id": v.id,
            "service_id": service["id"],
            "work_item_ids": [],
            "completed_work": "did_not_happen"
        }),
    )
    .await;
    let read = t
        .call(
            "get_incident",
            json!({ "vehicle_id": v.id, "incident_id": incident["id"] }),
        )
        .await;
    assert_eq!(read["resolved"], true, "got: {read}");
    assert_eq!(read["resolved_by_hand"], true, "got: {read}");
}

#[tokio::test]
async fn correct_service_names_the_incidents_its_invoice_resolved() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let incident = t
        .call(
            "log_incident",
            json!({ "vehicle_id": v.id, "title": "Leak" }),
        )
        .await;
    let job = t
        .call(
            "plan_work",
            json!({ "vehicle_id": v.id, "title": "Fix leak", "incident_id": incident["id"] }),
        )
        .await;
    let service = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-06-01",
                "work_item_ids": [job["id"]]
            }),
        )
        .await;
    assert_eq!(
        service["resolved_incidents"][0]["named_by_invoice"], false,
        "got: {service}"
    );

    let named = t
        .call(
            "correct_service",
            json!({
                "vehicle_id": v.id,
                "service_id": service["id"],
                "resolved_incident_ids": [incident["id"]]
            }),
        )
        .await;
    assert_eq!(
        named["resolved_incidents"][0]["named_by_invoice"], true,
        "got: {named}"
    );

    // The invoice names the incident, so taking the job off as not done
    // leaves it resolved and the reply says so.
    let out = t
        .call(
            "correct_service",
            json!({
                "vehicle_id": v.id,
                "service_id": service["id"],
                "work_item_ids": [],
                "completed_work": "did_not_happen"
            }),
        )
        .await;
    assert_eq!(
        out["incidents_kept_resolved"],
        json!([incident["id"]]),
        "got: {out}"
    );

    // Taking the name off with no job behind it asks what happened.
    let refused = t
        .call_err(
            "correct_service",
            json!({
                "vehicle_id": v.id,
                "service_id": service["id"],
                "resolved_incident_ids": []
            }),
        )
        .await;
    assert!(refused.contains("incident"), "got: {refused}");

    // An id that is not an incident of the vehicle is refused by name.
    let missing = t
        .call_err(
            "correct_service",
            json!({
                "vehicle_id": v.id,
                "service_id": service["id"],
                "resolved_incident_ids": [99_999]
            }),
        )
        .await;
    assert!(missing.contains("99999"), "got: {missing}");
}
