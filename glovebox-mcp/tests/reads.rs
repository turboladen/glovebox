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
        err.contains("unknown search scope"),
        "scope error must reach the LLM; got: {err}"
    );
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
            "add_schedule_item",
            "add_vehicle",
            "check_recalls",
            "correct_part",
            "correct_service",
            "cost_summary",
            "delete_incident",
            "delete_part",
            "delete_planned_work",
            "delete_service",
            "dismiss_schedule_item",
            "file_research_finding",
            "get_incident",
            "get_service",
            "get_vehicle",
            "link_service_to_maintenance",
            "list_incidents",
            "list_model_templates",
            "list_parts",
            "list_planned_work",
            "list_schedule",
            "list_services",
            "list_vehicles",
            "log_incident",
            "log_mileage",
            "plan_work",
            "record_part",
            "record_service",
            "save_note",
            "search_records",
            "set_recall_status",
            "summarize_recent_activity",
            "update_incident",
            "update_planned_work",
            "update_schedule_item",
            "update_vehicle",
            "whats_coming",
        ]
    );
}
