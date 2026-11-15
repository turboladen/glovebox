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
        // The client does not hand `$defs` to the model, so a `$ref` hides the fields behind it.
        assert!(
            !contains_ref(&tool["inputSchema"]),
            "tool {name} input schema must inline every subschema; got: {}",
            tool["inputSchema"]
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

/// True when a `$ref` or `$defs` key appears anywhere in the JSON tree.
fn contains_ref(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            map.contains_key("$ref") || map.contains_key("$defs") || map.values().any(contains_ref)
        }
        serde_json::Value::Array(items) => items.iter().any(contains_ref),
        _ => false,
    }
}

// The `properties` of the objects inside an optional array property, however the
// generated schema spells the nullable array (`items` directly or under `anyOf`).
fn array_item_properties<'a>(
    schema: &'a serde_json::Value,
    property: &str,
) -> &'a serde_json::Map<String, serde_json::Value> {
    let items = step_into(schema, property)
        .and_then(|list| step_into(list, "[]"))
        .unwrap_or_else(|| panic!("{property} has no array items: {schema}"));
    items["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("{property} items expose no properties: {items}"))
}

#[tokio::test]
async fn record_service_schema_exposes_nested_fields_with_descriptions() {
    let t = connect().await;
    let tools = t.tools().await;
    let tool = tools
        .iter()
        .find(|t| t["name"] == "record_service")
        .expect("record_service is advertised");
    let schema = &tool["inputSchema"];
    for (property, keys) in [
        (
            "line_items",
            &[
                "description",
                "work_kind",
                "quantity",
                "unit_cost_cents",
                "cost_cents",
            ][..],
        ),
        (
            "parts",
            &["name", "seller", "cost_cents", "supplied_by"][..],
        ),
    ] {
        let props = array_item_properties(schema, property);
        for key in keys {
            let description = props
                .get(*key)
                .unwrap_or_else(|| panic!("{property} items lack {key}: {props:?}"))["description"]
                .as_str()
                .unwrap_or("");
            assert!(
                !description.is_empty(),
                "{property}.{key} must carry a description"
            );
        }
    }
}

// An argument that takes a value from a fixed list says so in its schema, so
// the model sees the list without reading the description.
#[tokio::test]
async fn a_vocabulary_argument_lists_its_values_in_the_schema() {
    let t = connect().await;
    let tools = t.tools().await;
    let categories = json!([
        "general",
        "noise",
        "leak",
        "warning_light",
        "cosmetic",
        "performance",
        "obd_code",
        "damage",
        "accident",
        "note",
        "shop_observation"
    ]);
    let payers = json!(["self", "insurance", "third_party", "manufacturer"]);
    let performers = json!(["shop", "self"]);
    let work_kinds = json!([
        "maintenance",
        "mod",
        "repair",
        "inspection",
        "recall",
        "fee"
    ]);
    let suppliers = json!(["self", "shop"]);
    let part_statuses = json!(["purchased", "installed", "replaced", "returned"]);
    let job_statuses = json!(["planned", "done", "dropped"]);
    let approaches = json!(["replacement", "upgrade"]);
    let fitments = json!(["confirmed", "unconfirmed", "does_not_fit"]);
    // `[]` in a path steps into the items of a list.
    for (tool, path, values) in [
        ("log_incident", "category", &categories),
        ("update_incident", "category", &categories),
        ("record_service", "paid_by", &payers),
        ("record_service", "performed_by", &performers),
        ("record_service", "line_items.[].work_kind", &work_kinds),
        ("record_service", "line_items.[].paid_by", &payers),
        ("correct_service", "paid_by", &payers),
        ("correct_service", "performed_by", &performers),
        ("correct_service", "line_items.[].work_kind", &work_kinds),
        ("correct_service", "line_items.[].paid_by", &payers),
        ("record_service", "parts.[].supplied_by", &suppliers),
        ("record_part", "supplied_by", &suppliers),
        ("record_part", "status", &part_statuses),
        ("correct_part", "supplied_by", &suppliers),
        ("correct_part", "status", &part_statuses),
        ("list_parts", "status", &part_statuses),
        ("update_planned_work", "status", &job_statuses),
        ("add_need", "supplied_by", &suppliers),
        ("add_need", "approach", &approaches),
        ("update_need", "supplied_by", &suppliers),
        ("update_need", "approach", &approaches),
        ("add_need_option", "fitment", &fitments),
        ("update_need_option", "fitment", &fitments),
    ] {
        let input = tools
            .iter()
            .find(|t| t["name"] == tool)
            .map(|t| &t["inputSchema"])
            .unwrap_or_else(|| panic!("{tool} must be listed"));
        let schema = path.split('.').try_fold(input, step_into);
        assert_eq!(
            schema.and_then(listed_values).as_ref(),
            Some(values),
            "{tool} {path} must list its values; got: {schema:?}"
        );
    }
}

// The schema of `key` inside `schema`: a property, or the items of a list
// for `[]`, looking through the `anyOf` an optional argument is wrapped in.
fn step_into<'a>(schema: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    let own = if key == "[]" {
        schema.get("items")
    } else {
        schema.get("properties").and_then(|p| p.get(key))
    };
    own.or_else(|| {
        schema
            .get("anyOf")?
            .as_array()?
            .iter()
            .find_map(|s| step_into(s, key))
    })
}

// The `enum` list of a schema, looking through the `anyOf` an optional
// argument is wrapped in and leaving out its `null`.
fn listed_values(schema: &serde_json::Value) -> Option<serde_json::Value> {
    let own = schema.get("enum").and_then(|e| e.as_array()).map(|values| {
        let named = values.iter().filter(|v| !v.is_null()).cloned().collect();
        serde_json::Value::Array(named)
    });
    own.or_else(|| {
        schema
            .get("anyOf")?
            .as_array()?
            .iter()
            .find_map(listed_values)
    })
}
