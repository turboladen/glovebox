mod common;

use common::{
    connect, minimal_service, new_vehicle, reminder_status, schedule_item_every_12_months,
};
use glovebox_shared::{
    inputs::vehicle::NewVehicle,
    services::{schedule as schedule_svc, service_record as svc_svc, vehicle},
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
async fn record_service_schedule_item_ids_clears_the_overdue_reminder() {
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

    t.call(
        "record_service",
        json!({
            "vehicle_id": v.id,
            "service_date": "2026-06-20",
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
        err.contains("third_party"),
        "the error must list the valid payers; got: {err}"
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
            purchase_date: Some("2020-01-01".into()),
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
    // overdue.
    let imported = svc_svc::create(
        &t.db,
        v.id,
        minimal_service("2026-06-01", "Oil + brake fluid service"),
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
    assert!(err.contains("add, replace"), "got: {err}");
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
    assert!(fetched.schedule_item_ids.is_empty());
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
    assert_eq!(out["already_recorded"], false);

    let again = c
        .call(
            "record_service",
            json!({"vehicle_id": v, "service_date": "2025-03-24", "invoice_ref": "RO-1182"}),
        )
        .await;
    assert_eq!(again["already_recorded"], true);
    assert_eq!(again["id"], out["id"]);
}

#[tokio::test]
async fn record_service_rejects_a_bad_work_kind_with_the_field_name() {
    let c = connect().await;
    let v = vehicle::create(&c.db, new_vehicle("Daily"))
        .await
        .unwrap()
        .id;
    let err = c
        .call_err(
            "record_service",
            json!({"vehicle_id": v, "service_date": "2025-03-24",
                   "line_items": [{"description": "Tune", "work_kind": "upgrade"}]}),
        )
        .await;
    assert!(err.contains("work_kind"), "got: {err}");
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

    t.call(
        "delete_service",
        json!({ "vehicle_id": v.id, "service_id": recorded["id"] }),
    )
    .await;

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
            json!({ "vehicle_id": v.id, "service_id": service["id"], "recall_finding_ids": [right] }),
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
            json!({ "vehicle_id": v.id, "service_id": service["id"] }),
        )
        .await;
    assert_eq!(deleted["kept_part_ids"], json!([pads["id"]]));
    let parts = t.call("list_parts", json!({ "vehicle_id": v.id })).await;
    assert_eq!(parts[0]["status"], "purchased", "got: {parts}");
}

#[tokio::test]
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
async fn delete_service_asks_what_to_do_with_the_recalls_it_performed() {
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
        refused.contains("reopen") && refused.contains("keep_done"),
        "got: {refused}"
    );
    t.call("get_service", args.clone()).await;

    let invalid = t
        .call_err(
            "delete_service",
            json!({ "vehicle_id": v.id, "service_id": service["id"], "recalls": "ignore" }),
        )
        .await;
    assert!(
        invalid.contains("reopen") && invalid.contains("keep_done"),
        "got: {invalid}"
    );
    t.call("get_service", args.clone()).await;

    let deleted = t
        .call(
            "delete_service",
            json!({ "vehicle_id": v.id, "service_id": service["id"], "recalls": "keep_done" }),
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
