//! End-to-end tests for the MCP surface: drive the production
//! `glovebox_mcp::router` through the Streamable HTTP handshake, then
//! exercise tools and resources over the wire (JSON-RPC bodies, real
//! session ids). Unit tests in the crate cover URI parsing and host
//! allowlisting; these cover the full protocol chain
//! `router → rmcp StreamableHttpService → tool/resource handlers → shared services`.
//!
//! Harness mirrors fewd's `mcp_diet_tags_test.rs`, minus auth (glovebox's
//! `/mcp` is unauthenticated by design — LAN posture, see crate docs).

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use glovebox_shared::{
    inputs::{
        build::NewBuild, schedule::NewScheduleItem, service_record::NewServiceRecord,
        vehicle::NewVehicle,
    },
    services::{build as build_svc, schedule as schedule_svc, service_record as svc_svc, vehicle},
    test_support::test_db,
};
use sea_orm::DatabaseConnection;
use tower::ServiceExt;

async fn setup() -> (Router, DatabaseConnection) {
    let db = test_db().await;
    (glovebox_mcp::router(db.clone()), db)
}

fn new_vehicle(name: &str) -> NewVehicle {
    NewVehicle {
        name: name.into(),
        model_template_id: None,
        year: Some(2019),
        make: Some("Volkswagen".into()),
        model: Some("Golf R".into()),
        trim_level: None,
        body_style: None,
        engine: None,
        transmission: None,
        drivetrain: None,
        vin: None,
        license_plate: None,
        color: None,
        purchase_date: None,
        purchase_price_cents: None,
        purchase_price_currency: None,
        purchase_mileage: None,
        photo_path: None,
        notes: None,
    }
}

fn minimal_service(date: &str, description: &str, build_id: Option<i32>) -> NewServiceRecord {
    NewServiceRecord {
        service_date: date.into(),
        mileage: None,
        description: Some(description.into()),
        parts_cost_cents: None,
        parts_cost_currency: None,
        labor_cost_cents: None,
        labor_cost_currency: None,
        total_cost_cents: Some(15_000),
        total_cost_currency: None,
        shop_name: None,
        shop_id: None,
        notes: None,
        build_id,
        paid_by: None,
        payer_note: None,
        schedule_item_ids: None,
        part_ids: None,
        line_items: None,
    }
}

/// Run `initialize` + `notifications/initialized`; return the session id.
async fn handshake(app: &Router) -> String {
    let init_body = r#"{
        "jsonrpc": "2.0",
        "method": "initialize",
        "id": 1,
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "glovebox-mcp-test", "version": "0" }
        }
    }"#;
    let init_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("host", "localhost")
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .body(Body::from(init_body))
                .unwrap(),
        )
        .await
        .expect("init request");
    assert_eq!(init_resp.status(), StatusCode::OK, "initialize must 200");
    let session_id = init_resp
        .headers()
        .get("mcp-session-id")
        .expect("rmcp sets mcp-session-id on initialize response")
        .to_str()
        .expect("session id is ASCII")
        .to_string();
    drop(to_bytes(init_resp.into_body(), 64 * 1024).await.unwrap());

    let notif_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("host", "localhost")
                .header("mcp-session-id", &session_id)
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                ))
                .unwrap(),
        )
        .await
        .expect("initialized notification request");
    assert!(
        notif_resp.status().is_success(),
        "notifications/initialized must be accepted; got {}",
        notif_resp.status()
    );
    drop(to_bytes(notif_resp.into_body(), 64 * 1024).await.unwrap());

    session_id
}

async fn post_rpc(app: &Router, session_id: &str, body: String) -> String {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("host", "localhost")
                .header("mcp-session-id", session_id)
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .expect("rpc request");
    assert_eq!(resp.status(), StatusCode::OK, "rpc must 200");
    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn call_tool(name: &str, arguments: serde_json::Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "id": 10,
        "params": { "name": name, "arguments": arguments }
    })
    .to_string()
}

fn read_resource(uri: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "resources/read",
        "id": 11,
        "params": { "uri": uri }
    })
    .to_string()
}

fn assert_success(body: &str) {
    assert!(
        !body.contains("\"isError\":true") && !body.contains("\"error\""),
        "expected a success result; got: {body}"
    );
}

fn assert_tool_error(body: &str) {
    assert!(
        body.contains("\"isError\":true"),
        "expected a tool-level error result; got: {body}"
    );
}

/// Parse the JSON-RPC payload out of a raw response body, which rmcp may
/// deliver either as plain JSON or SSE-framed. SSE streams can carry empty
/// priming `data:` lines before the payload, so take the first `data:` line
/// that actually holds a JSON object.
fn extract_json(body: &str) -> serde_json::Value {
    let raw = body
        .lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .find(|payload| payload.trim_start().starts_with('{'))
        .unwrap_or(body);
    serde_json::from_str(raw).unwrap_or_else(|e| panic!("unparseable rpc body ({e}): {body}"))
}

// ─── Tools ──────────────────────────────────────────────────────

#[tokio::test]
async fn tools_list_advertises_the_full_verb_set() {
    let (app, _db) = setup().await;
    let session = handshake(&app).await;
    let body = post_rpc(
        &app,
        &session,
        r#"{"jsonrpc":"2.0","method":"tools/list","id":2}"#.to_string(),
    )
    .await;
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
        "find_documents",
        "search_records",
        "cost_summary",
        "check_recalls",
        "file_research_finding",
        "list_builds",
        "get_build_progress",
        "update_build_status",
    ] {
        assert!(
            body.contains(&format!("\"{tool}\"")),
            "tools/list must advertise {tool}; got: {body}"
        );
    }
    // Every argument-taking tool must advertise a real, non-empty input
    // schema — forgetting the `input_schema =` override on a #[tool]
    // silently advertises an empty one (see handler.rs docs), and a single
    // substring check would never catch that regression.
    let parsed = extract_json(&body);
    let tools = parsed["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools/list result must carry a tools array; got: {body}"));
    assert_eq!(tools.len(), 18, "expected exactly 18 tools; got: {body}");
    for tool in tools {
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

// ─── Transport security ─────────────────────────────────────────

#[tokio::test]
async fn rejects_unknown_host_header() {
    let (app, _db) = setup().await;
    // DNS-rebinding defense: a Host outside the allowlist must be refused
    // before any JSON-RPC processing happens.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("host", "evil.example")
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","method":"initialize","id":1,"params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "unknown Host must be rejected by the allowlist"
    );
}

#[tokio::test]
async fn list_vehicles_empty_then_after_seed() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;

    let body = post_rpc(
        &app,
        &session,
        call_tool("list_vehicles", serde_json::json!({})),
    )
    .await;
    assert_success(&body);
    assert!(
        body.contains("[]"),
        "empty garage must serialize as []; got: {body}"
    );

    vehicle::create(&db, new_vehicle("Daily")).await.unwrap();
    let body = post_rpc(
        &app,
        &session,
        call_tool("list_vehicles", serde_json::json!({})),
    )
    .await;
    assert_success(&body);
    assert!(body.contains("Daily") && body.contains("Volkswagen"));
}

#[tokio::test]
async fn record_service_then_summarize_recent_activity_round_trip() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let v = vehicle::create(&db, new_vehicle("Daily")).await.unwrap();

    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "record_service",
            serde_json::json!({
                "vehicle_id": v.id,
                "service_date": "2026-06-01",
                "description": "Oil change + filter",
                "mileage": 48_000,
                "total_cost_cents": 8_999,
                "line_items": [
                    { "description": "5W-30 oil 5qt", "cost_cents": 4_500 }
                ]
            }),
        ),
    )
    .await;
    assert_success(&body);
    assert!(body.contains("Oil change + filter"));

    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "summarize_recent_activity",
            serde_json::json!({ "vehicle_id": v.id }),
        ),
    )
    .await;
    assert_success(&body);
    assert!(
        body.contains("Oil change + filter") && body.contains("48000"),
        "activity feed must include the recorded service; got: {body}"
    );
}

/// Parse the JSON payload a successful tool call returns in its first
/// content block.
fn tool_payload(body: &str) -> serde_json::Value {
    let parsed = extract_json(body);
    let text = parsed["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("tool result must carry a text content block; got: {body}"));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("tool payload must be JSON ({e}): {text}"))
}

fn schedule_item_every_12_months(vehicle_id: i32, name: &str) -> NewScheduleItem {
    NewScheduleItem {
        platform_id: None,
        model_template_id: None,
        vehicle_id: Some(vehicle_id),
        overrides_item_id: None,
        name: name.into(),
        description: None,
        interval_miles: None,
        interval_months: Some(12),
        warning_miles: None,
        warning_days: None,
        enabled: None,
        source: None,
        notes: None,
        is_factory_recommended: None,
        labor_categories: None,
    }
}

/// Status of one named reminder out of a `check_due_maintenance` call.
async fn reminder_status(app: &Router, session: &str, vehicle_id: i32, name: &str) -> String {
    let body = post_rpc(
        app,
        session,
        call_tool(
            "check_due_maintenance",
            serde_json::json!({ "vehicle_id": vehicle_id }),
        ),
    )
    .await;
    assert_success(&body);
    let payload = tool_payload(&body);
    let reminders = payload["reminders"].as_array().expect("reminders array");
    reminders
        .iter()
        .find(|r| r["schedule_item"]["name"] == name)
        .unwrap_or_else(|| panic!("no reminder named {name}; got: {payload}"))["status"]
        .as_str()
        .expect("status string")
        .to_string()
}

#[tokio::test]
async fn record_service_schedule_item_ids_clears_the_overdue_reminder() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let v = vehicle::create(
        &db,
        NewVehicle {
            purchase_date: Some("2020-01-01".into()),
            ..new_vehicle("Daily")
        },
    )
    .await
    .unwrap();
    let item = schedule_svc::create(
        &db,
        schedule_item_every_12_months(v.id, "Brake fluid flush"),
    )
    .await
    .unwrap();

    // Never serviced since a 2020 purchase → the 12-month item is overdue.
    assert_eq!(
        reminder_status(&app, &session, v.id, "Brake fluid flush").await,
        "overdue"
    );

    // Recording the work WITHOUT the link does not clear the reminder;
    // linking via schedule_item_ids does.
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "record_service",
            serde_json::json!({
                "vehicle_id": v.id,
                "service_date": "2026-06-20",
                "description": "Brake fluid flush",
                "schedule_item_ids": [item.id]
            }),
        ),
    )
    .await;
    assert_success(&body);
    assert_eq!(
        reminder_status(&app, &session, v.id, "Brake fluid flush").await,
        "ok"
    );

    // Wrong-vehicle probe: linking another vehicle's schedule item is a clean
    // not-found tool error, and nothing is recorded.
    let other = vehicle::create(&db, new_vehicle("Other")).await.unwrap();
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "record_service",
            serde_json::json!({
                "vehicle_id": other.id,
                "service_date": "2026-06-20",
                "schedule_item_ids": [item.id]
            }),
        ),
    )
    .await;
    assert_tool_error(&body);
    assert!(body.contains("not found"), "got: {body}");
    assert!(
        svc_svc::list(&db, other.id).await.unwrap().is_empty(),
        "rejected link must not create a record"
    );
}

#[tokio::test]
async fn dismiss_schedule_item_hides_it_and_rejects_wrong_vehicle() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let v = vehicle::create(
        &db,
        NewVehicle {
            purchase_date: Some("2020-01-01".into()),
            ..new_vehicle("Daily")
        },
    )
    .await
    .unwrap();
    let item = schedule_svc::create(
        &db,
        schedule_item_every_12_months(v.id, "Dealer inspection"),
    )
    .await
    .unwrap();
    assert_eq!(schedule_svc::resolve(&db, v.id).await.unwrap().len(), 1);

    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "dismiss_schedule_item",
            serde_json::json!({
                "vehicle_id": v.id,
                "schedule_item_id": item.id,
                "reason": "independent shop handles this"
            }),
        ),
    )
    .await;
    assert_success(&body);
    let payload = tool_payload(&body);
    assert_eq!(payload["enabled"], false);
    assert!(
        payload["notes"]
            .as_str()
            .unwrap()
            .contains("independent shop handles this")
    );

    // Gone from the resolved schedule and from the reminders.
    assert!(schedule_svc::resolve(&db, v.id).await.unwrap().is_empty());
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "check_due_maintenance",
            serde_json::json!({ "vehicle_id": v.id }),
        ),
    )
    .await;
    assert_success(&body);
    let payload = tool_payload(&body);
    assert_eq!(
        payload["reminders"].as_array().map(Vec::len),
        Some(0),
        "dismissed item must not appear in reminders: {payload}"
    );

    // Wrong-vehicle and nonexistent probes are the same clean tool error.
    let other = vehicle::create(&db, new_vehicle("Other")).await.unwrap();
    for schedule_item_id in [item.id, 9999] {
        let body = post_rpc(
            &app,
            &session,
            call_tool(
                "dismiss_schedule_item",
                serde_json::json!({
                    "vehicle_id": other.id,
                    "schedule_item_id": schedule_item_id
                }),
            ),
        )
        .await;
        assert_tool_error(&body);
        assert!(body.contains("not found"), "got: {body}");
    }
}

#[tokio::test]
async fn log_incident_and_mileage_write_through() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let v = vehicle::create(&db, new_vehicle("Daily")).await.unwrap();

    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "log_incident",
            serde_json::json!({
                "vehicle_id": v.id,
                "title": "Squeak from front left",
                "category": "noise"
            }),
        ),
    )
    .await;
    assert_success(&body);

    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "log_mileage",
            serde_json::json!({ "vehicle_id": v.id, "mileage": 48_250 }),
        ),
    )
    .await;
    assert_success(&body);

    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "summarize_recent_activity",
            serde_json::json!({ "vehicle_id": v.id }),
        ),
    )
    .await;
    assert!(body.contains("Squeak from front left") && body.contains("48250"));
}

#[tokio::test]
async fn log_incident_rejects_unknown_category_and_wrong_links() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let v = vehicle::create(&db, new_vehicle("Daily")).await.unwrap();
    let other = vehicle::create(&db, new_vehicle("Other")).await.unwrap();

    // Unknown category -> tool-level error listing the whitelist.
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "log_incident",
            serde_json::json!({
                "vehicle_id": v.id,
                "title": "Bad category",
                "category": "bogus"
            }),
        ),
    )
    .await;
    assert_tool_error(&body);
    assert!(
        body.contains("Invalid category") && body.contains("warning_light"),
        "the error must steer toward valid categories; got: {body}"
    );

    // Accident-category incident round-trips.
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "log_incident",
            serde_json::json!({
                "vehicle_id": v.id,
                "title": "Rear-ended at a stoplight",
                "category": "accident",
                "description": "Other driver ran the light"
            }),
        ),
    )
    .await;
    assert_success(&body);
    assert!(body.contains("Rear-ended at a stoplight"));
    let incident_id = extract_json(&body)["result"]["content"][0]["text"]
        .as_str()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .and_then(|v| v["id"].as_i64())
        .expect("created incident payload must carry an id");

    // A recurrence pointing at another vehicle's incident must be
    // indistinguishable from a nonexistent one.
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "log_incident",
            serde_json::json!({
                "vehicle_id": other.id,
                "title": "Not my incident",
                "recurrence_of_id": incident_id
            }),
        ),
    )
    .await;
    assert_tool_error(&body);
    assert!(
        body.contains("not found"),
        "cross-vehicle recurrence must read as missing; got: {body}"
    );

    // Same-vehicle recurrence works.
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "log_incident",
            serde_json::json!({
                "vehicle_id": v.id,
                "title": "Hit again in the same spot",
                "category": "accident",
                "recurrence_of_id": incident_id
            }),
        ),
    )
    .await;
    assert_success(&body);
    assert!(body.contains("recurrence_of_id"));
}

#[tokio::test]
async fn save_note_is_searchable_via_search_records() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let v = vehicle::create(&db, new_vehicle("Daily")).await.unwrap();

    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "save_note",
            serde_json::json!({
                "vehicle_id": v.id,
                "note": "Steve prefers Liqui Moly 5W-40 for this engine"
            }),
        ),
    )
    .await;
    assert_success(&body);
    assert!(
        body.contains("\\\"note\\\"") || body.contains("\"note\""),
        "saved note must carry the note category; got: {body}"
    );

    // The FTS trigger on incidents must make the note findable immediately.
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "search_records",
            serde_json::json!({ "query": "Liqui Moly", "scope": "incidents" }),
        ),
    )
    .await;
    assert_success(&body);
    assert!(
        body.contains("Liqui Moly"),
        "note must be searchable under the incidents scope; got: {body}"
    );

    // Nonexistent vehicle -> clean tool error.
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "save_note",
            serde_json::json!({ "vehicle_id": 999, "note": "orphan" }),
        ),
    )
    .await;
    assert_tool_error(&body);
}

#[tokio::test]
async fn search_records_builds_scope_finds_seeded_build() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let v = vehicle::create(&db, new_vehicle("Project")).await.unwrap();
    build_svc::create(
        &db,
        v.id,
        NewBuild {
            name: "Big turbo kilonewton build".into(),
            description: Some("IS38 swap".into()),
            target_date: None,
        },
    )
    .await
    .unwrap();

    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "search_records",
            serde_json::json!({ "query": "kilonewton", "scope": "builds" }),
        ),
    )
    .await;
    assert_success(&body);
    assert!(
        body.contains("Big turbo kilonewton build") && body.contains("build"),
        "builds scope must find the seeded build by name; got: {body}"
    );

    // Retired scopes are clean tool errors, not silent empties.
    for retired in ["observations", "accidents"] {
        let body = post_rpc(
            &app,
            &session,
            call_tool(
                "search_records",
                serde_json::json!({ "query": "anything", "scope": retired }),
            ),
        )
        .await;
        assert_tool_error(&body);
        assert!(
            body.contains("unknown search scope"),
            "retired scope '{retired}' must be rejected; got: {body}"
        );
    }
}

#[tokio::test]
async fn record_part_round_trips_and_rejects_bad_links() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let v = vehicle::create(&db, new_vehicle("Daily")).await.unwrap();
    let other = vehicle::create(&db, new_vehicle("Other")).await.unwrap();
    let foreign_build = build_svc::create(
        &db,
        other.id,
        NewBuild {
            name: "Not mine".into(),
            description: None,
            target_date: None,
        },
    )
    .await
    .unwrap();

    // Parts have no MCP list surface; the tool result payload is the created
    // part record itself.
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "record_part",
            serde_json::json!({
                "vehicle_id": v.id,
                "name": "Sachs SRE clutch kit",
                "cost_cents": 89_900,
                "location": "Front brakes",
                "seller": "FCP Euro"
            }),
        ),
    )
    .await;
    assert_success(&body);
    assert!(
        body.contains("Sachs SRE clutch kit")
            && body.contains("Front brakes")
            && (body.contains("\\\"id\\\"") || body.contains("\"id\"")),
        "created part payload must carry name, location, and id; got: {body}"
    );
    assert!(
        body.contains("purchased"),
        "status must default to purchased; got: {body}"
    );

    // Nonexistent vehicle -> clean tool error.
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "record_part",
            serde_json::json!({ "vehicle_id": 999, "name": "Oil filter" }),
        ),
    )
    .await;
    assert_tool_error(&body);

    // Cross-vehicle build must be indistinguishable from a missing one.
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "record_part",
            serde_json::json!({
                "vehicle_id": v.id,
                "name": "Downpipe",
                "build_id": foreign_build.id
            }),
        ),
    )
    .await;
    assert_tool_error(&body);
    assert!(
        body.contains("not found"),
        "cross-vehicle build must read as missing; got: {body}"
    );
}

#[tokio::test]
async fn find_documents_finds_seeded_extracted_text() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let v = vehicle::create(&db, new_vehicle("Daily")).await.unwrap();
    {
        use glovebox_shared::entities::document;
        use sea_orm::{ActiveModelTrait, Set};
        document::ActiveModel {
            vehicle_id: Set(Some(v.id)),
            title: Set("FCP invoice".into()),
            file_path: Set("docs/fcp.pdf".into()),
            file_name: Set("fcp.pdf".into()),
            extracted_text: Set(Some("Sachs clutch kit and flywheel".into())),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "find_documents",
            serde_json::json!({ "vehicle_id": v.id, "query": "clutch flywheel" }),
        ),
    )
    .await;
    assert_success(&body);
    assert!(
        body.contains("FCP invoice") && body.contains("document"),
        "must surface the seeded document; got: {body}"
    );
}

#[tokio::test]
async fn search_records_scopes_and_rejects_bad_scope() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let v = vehicle::create(&db, new_vehicle("Daily")).await.unwrap();
    svc_svc::create(
        &db,
        v.id,
        minimal_service("2026-05-01", "Brake pad replacement", None),
    )
    .await
    .unwrap();

    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "search_records",
            serde_json::json!({ "query": "brake", "scope": "services" }),
        ),
    )
    .await;
    assert_success(&body);
    assert!(body.contains("Brake pad replacement"));

    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "search_records",
            serde_json::json!({ "query": "brake", "scope": "nonsense" }),
        ),
    )
    .await;
    assert_tool_error(&body);
    assert!(
        body.contains("unknown search scope"),
        "scope error must reach the LLM; got: {body}"
    );
}

#[tokio::test]
async fn check_due_maintenance_returns_without_error() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let v = vehicle::create(&db, new_vehicle("Daily")).await.unwrap();

    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "check_due_maintenance",
            serde_json::json!({ "vehicle_id": v.id }),
        ),
    )
    .await;
    assert_success(&body);
    assert!(
        body.contains("estimated_mileage") && body.contains("reminders"),
        "reminders payload must round-trip; got: {body}"
    );
}

#[tokio::test]
async fn cost_summary_reports_integer_cents() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let v = vehicle::create(&db, new_vehicle("Daily")).await.unwrap();
    svc_svc::create(&db, v.id, minimal_service("2026-05-01", "Brakes", None))
        .await
        .unwrap();

    let body = post_rpc(
        &app,
        &session,
        call_tool("cost_summary", serde_json::json!({ "vehicle_id": v.id })),
    )
    .await;
    assert_success(&body);
    assert!(
        body.contains("\\\"total_cost_cents\\\": 15000")
            || body.contains("\"total_cost_cents\": 15000"),
        "cost summary must carry the 15000-cent total; got: {body}"
    );
}

#[tokio::test]
async fn record_service_payer_flows_into_cost_summary_split() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let v = vehicle::create(&db, new_vehicle("Daily")).await.unwrap();

    // A $100 self-paid service (payer omitted -> defaults to self)…
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "record_service",
            serde_json::json!({
                "vehicle_id": v.id,
                "service_date": "2026-06-01",
                "description": "Brakes",
                "total_cost_cents": 10_000
            }),
        ),
    )
    .await;
    assert_success(&body);

    // …and a $150 insurance-paid repair.
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "record_service",
            serde_json::json!({
                "vehicle_id": v.id,
                "service_date": "2026-06-15",
                "description": "Collision repair",
                "total_cost_cents": 15_000,
                "paid_by": "insurance",
                "payer_note": "Progressive claim #12345"
            }),
        ),
    )
    .await;
    assert_success(&body);
    assert!(
        body.contains("insurance") && body.contains("Progressive claim #12345"),
        "record_service must echo the payer fields; got: {body}"
    );

    let body = post_rpc(
        &app,
        &session,
        call_tool("cost_summary", serde_json::json!({ "vehicle_id": v.id })),
    )
    .await;
    assert_success(&body);
    for expected in [
        "\"out_of_pocket_cents\": 10000",
        "\"covered_cents\": 15000",
        "\"total_cost_cents\": 25000",
    ] {
        let escaped = expected.replace('"', "\\\"");
        assert!(
            body.contains(expected) || body.contains(&escaped),
            "cost summary must carry the payer split ({expected}); got: {body}"
        );
    }

    // An unknown payer is a tool-level error, not a protocol failure.
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "record_service",
            serde_json::json!({
                "vehicle_id": v.id,
                "service_date": "2026-06-20",
                "description": "Bogus payer",
                "paid_by": "my neighbor"
            }),
        ),
    )
    .await;
    assert_tool_error(&body);
    assert!(
        body.contains("third_party"),
        "the error must list the valid payers; got: {body}"
    );
}

#[tokio::test]
async fn build_progress_and_status_update_round_trip() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let v = vehicle::create(&db, new_vehicle("Project")).await.unwrap();
    let b = build_svc::create(
        &db,
        v.id,
        NewBuild {
            name: "Turbo upgrade".into(),
            description: None,
            target_date: None,
        },
    )
    .await
    .unwrap();
    svc_svc::create(
        &db,
        v.id,
        minimal_service("2026-05-01", "Downpipe install", Some(b.id)),
    )
    .await
    .unwrap();

    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "get_build_progress",
            serde_json::json!({ "vehicle_id": v.id, "build_id": b.id }),
        ),
    )
    .await;
    assert_success(&body);
    assert!(
        body.contains("Turbo upgrade") && body.contains("services_count"),
        "build progress must roll up linked work; got: {body}"
    );

    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "update_build_status",
            serde_json::json!({ "vehicle_id": v.id, "build_id": b.id, "status": "active" }),
        ),
    )
    .await;
    assert_success(&body);
    assert!(body.contains("\\\"active\\\"") || body.contains("\"active\""));

    // Invalid lifecycle status is an LLM-recoverable tool error.
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "update_build_status",
            serde_json::json!({ "vehicle_id": v.id, "build_id": b.id, "status": "bogus" }),
        ),
    )
    .await;
    assert_tool_error(&body);
    assert!(body.contains("Invalid status"));
}

#[tokio::test]
async fn file_research_finding_persists_and_is_readable() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let v = vehicle::create(&db, new_vehicle("Daily")).await.unwrap();

    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "file_research_finding",
            serde_json::json!({
                "vehicle_id": v.id,
                "category": "maintenance",
                "title": "DSG interval 40k",
                "source_url": "https://example.com/t"
            }),
        ),
    )
    .await;
    assert_success(&body);
    assert!(body.contains("DSG interval 40k"));

    // Wrong vehicle -> clean tool error
    let err = post_rpc(
        &app,
        &session,
        call_tool(
            "file_research_finding",
            serde_json::json!({"vehicle_id": 999, "category": "x", "title": "y"}),
        ),
    )
    .await;
    assert_tool_error(&err);
}

#[tokio::test]
async fn wrong_vehicle_ids_are_clean_tool_errors() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let v = vehicle::create(&db, new_vehicle("Mine")).await.unwrap();
    let other = vehicle::create(&db, new_vehicle("Other")).await.unwrap();
    let b = build_svc::create(
        &db,
        v.id,
        NewBuild {
            name: "B".into(),
            description: None,
            target_date: None,
        },
    )
    .await
    .unwrap();

    // Nonexistent vehicle.
    let body = post_rpc(
        &app,
        &session,
        call_tool("get_vehicle", serde_json::json!({ "vehicle_id": 999 })),
    )
    .await;
    assert_tool_error(&body);
    assert!(
        body.contains("not found"),
        "message must reach the LLM; got: {body}"
    );

    // Write against a nonexistent vehicle.
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "record_service",
            serde_json::json!({ "vehicle_id": 999, "service_date": "2026-06-01" }),
        ),
    )
    .await;
    assert_tool_error(&body);

    // Cross-vehicle build must be indistinguishable from a missing one.
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "get_build_progress",
            serde_json::json!({ "vehicle_id": other.id, "build_id": b.id }),
        ),
    )
    .await;
    assert_tool_error(&body);
}

#[tokio::test]
async fn malformed_arguments_are_schema_errors_not_protocol_failures() {
    let (app, _db) = setup().await;
    let session = handshake(&app).await;

    // Wrong type.
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "record_service",
            serde_json::json!({ "vehicle_id": "not-a-number", "service_date": "2026-06-01" }),
        ),
    )
    .await;
    assert_tool_error(&body);
    assert!(
        body.contains("input schema"),
        "error must point at the schema; got: {body}"
    );

    // Missing required field.
    let body = post_rpc(
        &app,
        &session,
        call_tool("get_vehicle", serde_json::json!({})),
    )
    .await;
    assert_tool_error(&body);
    assert!(
        body.contains("vehicle_id"),
        "must name the missing field; got: {body}"
    );
}

// ─── Resources ──────────────────────────────────────────────────

#[tokio::test]
async fn resources_list_and_read_cover_all_uri_forms() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let v = vehicle::create(&db, new_vehicle("Daily")).await.unwrap();
    let b = build_svc::create(
        &db,
        v.id,
        NewBuild {
            name: "Turbo upgrade".into(),
            description: None,
            target_date: None,
        },
    )
    .await
    .unwrap();
    svc_svc::create(&db, v.id, minimal_service("2026-05-01", "Oil change", None))
        .await
        .unwrap();

    let body = post_rpc(
        &app,
        &session,
        r#"{"jsonrpc":"2.0","method":"resources/list","id":2}"#.to_string(),
    )
    .await;
    for uri in [
        "glovebox://vehicles".to_string(),
        format!("glovebox://vehicles/{}", v.id),
        format!("glovebox://vehicles/{}/activity", v.id),
        format!("glovebox://vehicles/{}/builds/{}", v.id, b.id),
    ] {
        assert!(
            body.contains(&uri),
            "resources/list must advertise {uri}; got: {body}"
        );
    }

    let body = post_rpc(&app, &session, read_resource("glovebox://vehicles")).await;
    assert!(body.contains("Daily"), "vehicle list resource; got: {body}");

    let body = post_rpc(
        &app,
        &session,
        read_resource(&format!("glovebox://vehicles/{}", v.id)),
    )
    .await;
    assert!(
        body.contains("Volkswagen") && body.contains("Golf R"),
        "full vehicle record resource; got: {body}"
    );

    let body = post_rpc(
        &app,
        &session,
        read_resource(&format!("glovebox://vehicles/{}/activity", v.id)),
    )
    .await;
    assert!(
        body.contains("Oil change"),
        "activity resource; got: {body}"
    );

    let body = post_rpc(
        &app,
        &session,
        read_resource(&format!("glovebox://vehicles/{}/builds/{}", v.id, b.id)),
    )
    .await;
    assert!(
        body.contains("Turbo upgrade") && body.contains("services_count"),
        "build progress resource; got: {body}"
    );
}

#[tokio::test]
async fn read_resource_unknown_uri_and_missing_record_are_clean_errors() {
    let (app, _db) = setup().await;
    let session = handshake(&app).await;

    // Unknown shape → invalid_params (-32602) naming the known forms.
    let body = post_rpc(&app, &session, read_resource("glovebox://garage")).await;
    assert!(
        body.contains("-32602") && body.contains("resources/list"),
        "unknown uri must be invalid_params with a recovery hint; got: {body}"
    );

    // Well-formed but nonexistent → resource_not_found (-32002), not a 500.
    let body = post_rpc(&app, &session, read_resource("glovebox://vehicles/999")).await;
    assert!(
        body.contains("\"error\"") && body.contains("not found"),
        "missing vehicle must be a clean resource error; got: {body}"
    );
}
