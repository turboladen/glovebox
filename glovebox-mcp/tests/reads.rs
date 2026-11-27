mod common;

use common::{connect, minimal_service, new_vehicle};
use glovebox_shared::services::{service_record as svc_svc, vehicle};
use serde_json::json;

#[tokio::test]
async fn search_records_scopes_and_rejects_bad_scope() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    svc_svc::create(
        &t.db,
        v.id,
        minimal_service("2026-05-01", "Brake pad replacement"),
    )
    .await
    .unwrap();

    let hits = t
        .call(
            "search_records",
            json!({ "query": "brake", "scope": "services" }),
        )
        .await
        .to_string();
    assert!(hits.contains("Brake pad replacement"));

    let err = t
        .call_err(
            "search_records",
            json!({ "query": "brake", "scope": "nonsense" }),
        )
        .await;
    assert!(
        err.contains("scope: unknown variant `nonsense`") && err.contains("`maintenance`"),
        "scope error must reach the LLM and list the scopes; got: {err}"
    );
}

#[tokio::test]
async fn search_records_finds_a_kit_by_an_items_number() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let kit = t
        .call(
            "record_part",
            json!({
                "vehicle_id": v.id,
                "name": "Oil change kit",
                "contents": [{ "name": "Oil filter", "manufacturer": "Mann", "part_number": "HU 7020 z" }]
            }),
        )
        .await;

    let hits = t
        .call(
            "search_records",
            json!({ "query": "HU 7020 z", "scope": "parts" }),
        )
        .await;

    assert_eq!(hits.as_array().unwrap().len(), 1, "got: {hits}");
    assert_eq!(hits[0]["kind"], "part");
    assert_eq!(hits[0]["id"], kit["id"]);
    assert_eq!(hits[0]["title"], "Oil change kit");
}

#[tokio::test]
async fn search_records_finds_a_part_number_however_it_is_spaced() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let packed = t
        .call(
            "record_part",
            json!({ "vehicle_id": v.id, "name": "Oil filter", "part_number": "06K115562LM" }),
        )
        .await;
    let spaced = t
        .call(
            "record_part",
            json!({ "vehicle_id": v.id, "name": "Air filter", "part_number": "5Q0 129 620 B" }),
        )
        .await;

    for (query, want) in [("06K 115 562 LM", &packed), ("5Q0129620B", &spaced)] {
        let hits = t
            .call(
                "search_records",
                json!({ "query": query, "scope": "parts" }),
            )
            .await;
        assert_eq!(hits.as_array().unwrap().len(), 1, "query {query}: {hits}");
        assert_eq!(hits[0]["id"], want["id"]);
    }
}

#[tokio::test]
async fn cost_summary_reports_integer_cents() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    svc_svc::create(&t.db, v.id, minimal_service("2026-05-01", "Brakes"))
        .await
        .unwrap();

    let summary = t.call("cost_summary", json!({ "vehicle_id": v.id })).await;
    assert_eq!(summary["total_cost_cents"], 15_000);
}

#[tokio::test]
async fn cost_summary_lists_a_covered_service_that_has_no_total() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let covered = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-11-09",
                "description": "Fuel pump recall",
                "paid_by": "manufacturer",
                "payer_note": "Recall 26V001"
            }),
        )
        .await;
    let service_id = covered["id"].clone();

    let summary = t.call("cost_summary", json!({ "vehicle_id": v.id })).await;
    assert_eq!(summary["covered_cents"], 0);
    assert_eq!(summary["covered_work_count"], 1);
    let listed = &summary["covered_work"][0];
    assert_eq!(listed["service_id"], service_id);
    assert_eq!(listed["paid_by"], "manufacturer");
    assert_eq!(listed["payer_note"], "Recall 26V001");
    assert!(listed.get("total_cost_cents").is_none());
    assert_eq!(listed["covered_by_others_cents"], 0);
    assert_eq!(listed["out_of_pocket_cents"], 0);
}

#[tokio::test]
async fn resources_list_and_read_cover_all_uri_forms() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    svc_svc::create(&t.db, v.id, minimal_service("2026-05-01", "Oil change"))
        .await
        .unwrap();

    let resources = t.list_resources().await;
    let uris: Vec<&str> = resources
        .iter()
        .map(|r| r["uri"].as_str().expect("resource uri"))
        .collect();
    for uri in [
        "glovebox://vehicles".to_string(),
        format!("glovebox://vehicles/{}", v.id),
        format!("glovebox://vehicles/{}/activity", v.id),
    ] {
        assert!(
            uris.contains(&uri.as_str()),
            "must advertise {uri}; got: {uris:?}"
        );
    }
    assert!(
        !uris.iter().any(|u| u.contains("builds")),
        "no builds resource remains; got: {uris:?}"
    );

    // Pin the serialized Resource wire shape so a future rmcp bump that
    // renames or drops a field turns into a red test.
    let r0 = &resources[0];
    assert_eq!(
        r0["uri"], "glovebox://vehicles",
        "resources[0] must deterministically be the vehicles list resource"
    );
    assert_eq!(r0["mimeType"], "application/json");
    assert!(r0["description"].is_string());
    assert!(r0.get("annotations").is_none());

    let list = t.read_resource("glovebox://vehicles").await;
    assert!(list.contains("Daily"), "vehicle list resource; got: {list}");

    let record = t
        .read_resource(&format!("glovebox://vehicles/{}", v.id))
        .await;
    assert!(
        record.contains("Volkswagen") && record.contains("Golf R"),
        "full vehicle record resource; got: {record}"
    );

    let feed = t
        .read_resource(&format!("glovebox://vehicles/{}/activity", v.id))
        .await;
    assert!(
        feed.contains("Oil change"),
        "activity resource; got: {feed}"
    );
}

#[tokio::test]
async fn read_resource_unknown_uri_and_missing_record_are_clean_errors() {
    let t = connect().await;

    // An unknown shape is invalid_params naming the way to enumerate URIs.
    let err = t.read_resource_err("glovebox://garage").await;
    assert!(
        err.contains("-32602") && err.contains("resources/list"),
        "unknown uri must be invalid_params with a recovery hint; got: {err}"
    );

    // A well-formed but nonexistent id is resource_not_found.
    let err = t.read_resource_err("glovebox://vehicles/999").await;
    assert!(err.contains("not found"), "got: {err}");
}

#[tokio::test]
async fn the_server_offers_exactly_the_expected_tools() {
    let t = connect().await;
    let mut names = t.tool_names().await;
    names.sort();
    assert_eq!(
        names,
        [
            "add_incident_followup",
            "add_need",
            "add_need_option",
            "add_schedule_item",
            "add_shop",
            "add_todo",
            "add_vehicle",
            "archive_vehicle",
            "check_recalls",
            "correct_part",
            "correct_service",
            "correct_wear_reading",
            "cost_summary",
            "decode_vin",
            "delete_appointment",
            "delete_incident",
            "delete_incident_followup",
            "delete_mileage",
            "delete_need",
            "delete_need_option",
            "delete_part",
            "delete_planned_work",
            "delete_research_finding",
            "delete_service",
            "delete_shop",
            "delete_todo",
            "delete_vehicle",
            "delete_wear_reading",
            "dismiss_schedule_item",
            "file_research_finding",
            "get_checklist",
            "get_incident",
            "get_research_finding",
            "get_service",
            "get_shop",
            "get_vehicle",
            "get_work_sheet",
            "link_part_to_need",
            "link_service_to_maintenance",
            "list_appointments",
            "list_incidents",
            "list_mileage",
            "list_model_templates",
            "list_parts",
            "list_planned_work",
            "list_recalls",
            "list_research_findings",
            "list_schedule",
            "list_services",
            "list_shops",
            "list_vehicle_attributes",
            "list_vehicles",
            "list_wear_readings",
            "log_incident",
            "log_mileage",
            "mark_need_ordered",
            "mark_need_received",
            "merge_shops",
            "past_decisions",
            "plan_appointment",
            "plan_work",
            "record_part",
            "record_service",
            "record_wear_reading",
            "restore_schedule_item",
            "return_part",
            "save_note",
            "search_records",
            "set_recall_status",
            "summarize_recent_activity",
            "unarchive_vehicle",
            "update_appointment",
            "update_incident",
            "update_incident_followup",
            "update_need",
            "update_need_option",
            "update_planned_work",
            "update_research_finding",
            "update_schedule_item",
            "update_shop",
            "update_todo",
            "update_vehicle",
            "whats_coming",
        ]
    );
}

#[tokio::test]
async fn reads_tools_reject_unknown_arguments() {
    let t = connect().await;
    common::assert_unknown_field_rejected(
        &t,
        "search_records",
        json!({ "query": "brake", "limit": 5 }),
        "limit",
    )
    .await;
}

#[tokio::test]
async fn cost_summary_takes_a_date_range_and_splits_lines_by_work_kind() {
    let t = connect().await;
    let v = common::add_vehicle(&t, "Daily").await;
    for (day, mileage, total, lines) in [
        (
            "2025-03-01",
            20_000,
            12_900,
            json!([{ "description": "Oil", "work_kind": "maintenance", "cost_cents": 12_900 }]),
        ),
        (
            "2026-03-01",
            23_000,
            59_000,
            json!([
                { "description": "Water pump", "work_kind": "repair", "cost_cents": 45_000,
                  "paid_by": "manufacturer" },
                { "description": "Oil", "work_kind": "maintenance", "cost_cents": 12_900 },
                { "description": "Shop supplies", "work_kind": "fee", "cost_cents": 1_100 }
            ]),
        ),
        (
            "2026-09-01",
            26_000,
            8_000,
            json!([{ "description": "Alignment", "work_kind": "maintenance", "cost_cents": 8_000 }]),
        ),
    ] {
        t.call(
            "record_service",
            json!({
                "vehicle_id": v,
                "service_date": day,
                "mileage": mileage,
                "total_cost_cents": total,
                "line_items": lines
            }),
        )
        .await;
    }

    let all = t.call("cost_summary", json!({ "vehicle_id": v })).await;
    assert_eq!(all["total_cost_cents"], 79_900);
    assert!(all["from"].is_null() && all["to"].is_null(), "{all}");
    assert_eq!(all["miles_covered"], 6_000);

    let year = t
        .call(
            "cost_summary",
            json!({ "vehicle_id": v, "from": "2026-01-01", "to": "2026-12-31" }),
        )
        .await;
    assert_eq!(year["from"], "2026-01-01");
    assert_eq!(year["to"], "2026-12-31");
    assert_eq!(year["service_count"], 2);
    assert_eq!(year["total_cost_cents"], 67_000);
    assert_eq!(year["covered_cents"], 45_000);
    assert_eq!(year["out_of_pocket_cents"], 22_000);
    // The miles start at the 2025 service, the latest reading on or before
    // the range's start, and end at the September 2026 service.
    assert_eq!(year["miles_covered"], 6_000);
    assert_eq!(year["history_starts_on"], "2025-03-01");
    assert_eq!(year["cost_per_1000_miles_cents"], 11_167);
    assert_eq!(
        year["by_work_kind"],
        json!([
            { "work_kind": "maintenance", "line_count": 2, "cost_cents": 20_900,
              "out_of_pocket_cents": 20_900, "covered_cents": 0 },
            { "work_kind": "repair", "line_count": 1, "cost_cents": 45_000,
              "out_of_pocket_cents": 0, "covered_cents": 45_000 },
            { "work_kind": "fee", "line_count": 1, "cost_cents": 1_100,
              "out_of_pocket_cents": 1_100, "covered_cents": 0 }
        ])
    );

    let err = t
        .call_err(
            "cost_summary",
            json!({ "vehicle_id": v, "from": "2026-12-31", "to": "2026-01-01" }),
        )
        .await;
    assert!(err.starts_with("from:"), "got: {err}");
    let err = t
        .call_err(
            "cost_summary",
            json!({ "vehicle_id": v, "from": "last year" }),
        )
        .await;
    assert!(err.contains("from"), "got: {err}");
    common::assert_unknown_field_rejected(
        &t,
        "cost_summary",
        json!({ "vehicle_id": v, "since": "2026-01-01" }),
        "since",
    )
    .await;
}
