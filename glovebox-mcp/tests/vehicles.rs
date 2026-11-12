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
        "whats_coming",
        "dismiss_schedule_item",
        "summarize_recent_activity",
        "search_records",
        "cost_summary",
        "check_recalls",
        "set_recall_status",
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
    assert!(
        !names.contains(&"check_due_maintenance".to_string()),
        "check_due_maintenance is replaced by whats_coming"
    );
    // Every argument-taking tool must advertise a real, non-empty input
    // schema. Forgetting the `input_schema =` override on a #[tool] silently
    // advertises an empty one, and a name check alone would not catch it.
    for tool in &tools {
        let name = tool["name"].as_str().expect("tool name");
        // Every input refuses keys it does not define, and says so in its schema.
        assert_eq!(
            tool["inputSchema"]["additionalProperties"], false,
            "tool {name} must advertise additionalProperties: false; got: {tool}"
        );
        if name == "list_vehicles" || name == "list_model_templates" {
            continue; // deliberately take no arguments
        }
        let props = tool["inputSchema"]["properties"].as_object();
        assert!(
            props.is_some_and(|p| !p.is_empty()),
            "tool {name} must advertise a non-empty input schema; got: {tool}"
        );
    }
}

#[tokio::test]
async fn add_vehicle_from_a_template_inherits_its_schedule() {
    let t = connect().await;

    let templates = t.call("list_model_templates", json!({})).await;
    let template = templates
        .as_array()
        .and_then(|a| a.first())
        .unwrap_or_else(|| panic!("the seeded template must be listed; got: {templates}"));
    assert!(
        template["platform_name"].is_string(),
        "each template names its platform: {template}"
    );

    let added = t
        .call(
            "add_vehicle",
            json!({
                "name": "Daily",
                "model_template_id": template["id"],
                "license_plate": "ABC123",
                "notes": "bought used"
            }),
        )
        .await;
    assert_eq!(added["name"], "Daily");
    assert_eq!(added["license_plate"], "ABC123");
    assert!(
        added.get("vin_decode_error").is_none(),
        "no vin was sent, so no lookup ran: {added}"
    );

    let schedule = t
        .call("list_schedule", json!({ "vehicle_id": added["id"] }))
        .await;
    assert!(
        schedule.as_array().is_some_and(|a| !a.is_empty()),
        "a templated vehicle inherits maintenance items: {schedule}"
    );
}

#[tokio::test]
async fn update_vehicle_sets_a_field_and_clears_another_with_null() {
    let t = connect().await;
    let added = t
        .call(
            "add_vehicle",
            json!({ "name": "Daily", "notes": "bought used" }),
        )
        .await;
    let id = added["id"].clone();

    let updated = t
        .call(
            "update_vehicle",
            json!({ "vehicle_id": id, "license_plate": "XYZ789", "notes": null }),
        )
        .await;
    assert_eq!(updated["license_plate"], "XYZ789");
    assert!(updated["notes"].is_null(), "null clears notes: {updated}");
    assert_eq!(updated["name"], "Daily", "absent keys are left alone");

    let untouched = t
        .call(
            "update_vehicle",
            json!({ "vehicle_id": id, "color": "red" }),
        )
        .await;
    assert_eq!(
        untouched["license_plate"], "XYZ789",
        "an absent key must not clear the plate"
    );

    let err = t
        .call_err(
            "update_vehicle",
            json!({ "vehicle_id": 9999, "color": "red" }),
        )
        .await;
    assert!(err.contains("not found"), "got: {err}");
}

#[tokio::test]
async fn an_unknown_model_template_is_a_clear_tool_error() {
    let t = connect().await;
    let err = t
        .call_err(
            "add_vehicle",
            json!({ "name": "Daily", "model_template_id": 99999 }),
        )
        .await;
    assert!(err.contains("list_model_templates"), "got: {err}");
}

#[tokio::test]
async fn vehicles_tools_reject_unknown_arguments() {
    let t = connect().await;
    common::assert_unknown_field_rejected(
        &t,
        "update_vehicle",
        json!({ "vehicle_id": 1, "nickname": "x" }),
        "nickname",
    )
    .await;
}

#[tokio::test]
async fn a_mistyped_reading_can_be_listed_and_deleted() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let typo = t
        .call(
            "log_mileage",
            json!({ "vehicle_id": v.id, "mileage": 480_000, "recorded_at": "2026-05-01 09:00:00" }),
        )
        .await;
    let service = t
        .call(
            "record_service",
            json!({ "vehicle_id": v.id, "service_date": "2026-05-02", "mileage": 48_100 }),
        )
        .await;

    let listed = t.call("list_mileage", json!({ "vehicle_id": v.id })).await;
    let readings = listed.as_array().expect("readings");
    assert_eq!(readings.len(), 2, "got: {listed}");
    let from_service = readings
        .iter()
        .find(|r| r["service_record_id"] == service["id"])
        .unwrap_or_else(|| panic!("the service's reading is listed: {listed}"));

    let refused = t
        .call_err(
            "delete_mileage",
            json!({ "vehicle_id": v.id, "mileage_id": from_service["id"] }),
        )
        .await;
    assert!(refused.contains("correct_service"), "got: {refused}");

    let other = vehicle::create(&t.db, new_vehicle("Other")).await.unwrap();
    let wrong_vehicle = t
        .call_err(
            "delete_mileage",
            json!({ "vehicle_id": other.id, "mileage_id": typo["id"] }),
        )
        .await;
    let missing = t
        .call_err(
            "delete_mileage",
            json!({ "vehicle_id": other.id, "mileage_id": 99_999 }),
        )
        .await;
    assert_eq!(
        wrong_vehicle.replace(&typo["id"].to_string(), "N"),
        missing.replace("99999", "N")
    );

    let deleted = t
        .call(
            "delete_mileage",
            json!({ "vehicle_id": v.id, "mileage_id": typo["id"] }),
        )
        .await;
    assert_eq!(deleted["id"], typo["id"]);
    let left = t.call("list_mileage", json!({ "vehicle_id": v.id })).await;
    assert_eq!(left.as_array().map(Vec::len), Some(1), "got: {left}");
}

#[tokio::test]
async fn only_a_vehicle_with_no_history_can_be_deleted() {
    let t = connect().await;
    let template = glovebox_shared::services::model_template::list(&t.db)
        .await
        .unwrap()[0]
        .id;
    let added = t
        .call(
            "add_vehicle",
            json!({ "name": "Duplicate", "model_template_id": template }),
        )
        .await;
    t.call(
        "log_mileage",
        json!({ "vehicle_id": added["id"], "mileage": 1_000 }),
    )
    .await;
    let deleted = t
        .call("delete_vehicle", json!({ "vehicle_id": added["id"] }))
        .await;
    assert_eq!(deleted["id"], added["id"]);
    let gone = t
        .call_err("get_vehicle", json!({ "vehicle_id": added["id"] }))
        .await;
    assert!(gone.contains("not found"), "got: {gone}");

    let kept = t.call("add_vehicle", json!({ "name": "Real car" })).await;
    t.call(
        "plan_work",
        json!({ "vehicle_id": kept["id"], "title": "Wipers" }),
    )
    .await;
    let refused = t
        .call_err("delete_vehicle", json!({ "vehicle_id": kept["id"] }))
        .await;
    assert!(refused.contains("planned work"), "got: {refused}");
    t.call("get_vehicle", json!({ "vehicle_id": kept["id"] }))
        .await;

    let missing = t
        .call_err("delete_vehicle", json!({ "vehicle_id": 99_999 }))
        .await;
    assert!(missing.contains("not found"), "got: {missing}");
}
