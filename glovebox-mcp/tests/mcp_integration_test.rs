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
    inputs::{build::NewBuild, service_record::NewServiceRecord, vehicle::NewVehicle},
    services::{build as build_svc, service_record as svc_svc, vehicle},
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
        "log_observation",
        "log_mileage",
        "check_due_maintenance",
        "summarize_recent_activity",
        "find_documents",
        "search_records",
        "cost_summary",
        "check_recalls",
        "list_builds",
        "get_build_progress",
        "update_build_status",
    ] {
        assert!(
            body.contains(&format!("\"{tool}\"")),
            "tools/list must advertise {tool}; got: {body}"
        );
    }
    // The schema override must produce real input schemas, not empty ones.
    assert!(
        body.contains("vehicle_id"),
        "tool input schemas must expose vehicle_id; got: {body}"
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

#[tokio::test]
async fn log_observation_and_mileage_write_through() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let v = vehicle::create(&db, new_vehicle("Daily")).await.unwrap();

    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "log_observation",
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
