mod common;

use common::{connect, new_vehicle};
use glovebox_shared::services::vehicle;
use serde_json::json;

#[tokio::test]
async fn list_vehicles_empty_then_after_seed() {
    let t = connect().await;

    let empty = t.call("list_vehicles", json!({})).await;
    assert_eq!(empty, json!([]), "empty garage must serialize as []");

    vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let listed = t.call("list_vehicles", json!({})).await.to_string();
    assert!(listed.contains("Daily") && listed.contains("Volkswagen"));
}

#[tokio::test]
async fn nonexistent_vehicle_ids_are_clean_tool_errors() {
    let t = connect().await;

    let err = t
        .call_err("get_vehicle", json!({ "vehicle_id": 999 }))
        .await;
    assert!(
        err.contains("not found"),
        "message must reach the LLM; got: {err}"
    );

    t.call_err(
        "record_service",
        json!({ "vehicle_id": 999, "service_date": "2026-06-01" }),
    )
    .await;
}

#[tokio::test]
async fn malformed_arguments_are_schema_errors_not_protocol_failures() {
    let t = connect().await;

    let err = t
        .call_err(
            "record_service",
            json!({ "vehicle_id": "not-a-number", "service_date": "2026-06-01" }),
        )
        .await;
    assert!(
        err.contains("input schema"),
        "error must point at the schema; got: {err}"
    );

    let err = t.call_err("get_vehicle", json!({})).await;
    assert!(
        err.contains("vehicle_id"),
        "must name the missing field; got: {err}"
    );
}

#[tokio::test]
async fn tools_list_has_no_build_visit_or_document_verbs() {
    let t = connect().await;
    let names = t.tool_names().await;
    for gone in [
        "list_builds",
        "get_build_progress",
        "update_build_status",
        "schedule_visit",
        "complete_visit",
        "cancel_visit",
        "attach_document",
        "find_documents",
    ] {
        assert!(!names.contains(&gone.to_string()), "{gone} must be removed");
    }
}

#[tokio::test]
async fn tools_list_advertises_the_remaining_verb_set() {
    let t = connect().await;
    let tools = t.tools().await;
    let names = t.tool_names().await;
    for tool in [
        "list_vehicles",
        "get_vehicle",
        "record_service",
        "record_part",
        "log_incident",
        "save_note",
        "log_mileage",
        "check_due_maintenance",
        "dismiss_schedule_item",
        "summarize_recent_activity",
        "search_records",
        "cost_summary",
        "check_recalls",
        "file_research_finding",
        "plan_work",
        "list_planned_work",
        "link_service_to_maintenance",
        "get_service",
        "get_incident",
    ] {
        assert!(
            names.contains(&tool.to_string()),
            "tools/list must advertise {tool}"
        );
    }
    assert_eq!(tools.len(), 19, "expected exactly 19 tools; got: {names:?}");
    // Every argument-taking tool must advertise a real, non-empty input
    // schema. Forgetting the `input_schema =` override on a #[tool] silently
    // advertises an empty one, and a name check alone would not catch it.
    for tool in &tools {
        let name = tool["name"].as_str().expect("tool name");
        if name == "list_vehicles" {
            continue; // deliberately takes no arguments
        }
        let props = tool["inputSchema"]["properties"].as_object();
        assert!(
            props.is_some_and(|p| !p.is_empty()),
            "tool {name} must advertise a non-empty input schema; got: {tool}"
        );
    }
}
