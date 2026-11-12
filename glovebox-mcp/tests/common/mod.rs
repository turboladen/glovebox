//! Shared harness for the integration tests: an in-process MCP client wired
//! to a [`GloveboxMcp`] over an in-memory duplex stream, backed by a fresh
//! in-memory database.

#![allow(dead_code)]

use glovebox_mcp::GloveboxMcp;
use glovebox_shared::{
    inputs::{schedule::NewScheduleItem, service_record::NewServiceRecord, vehicle::NewVehicle},
    test_support::test_db,
};
use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, ReadResourceRequestParams},
    service::RunningService,
};
use sea_orm::DatabaseConnection;
use serde_json::Value;

pub struct TestClient {
    pub db: DatabaseConnection,
    client: RunningService<RoleClient, ()>,
}

/// Starts a server over a fresh in-memory database and returns a client that
/// has completed the MCP handshake with it.
pub async fn connect() -> TestClient {
    let db = test_db().await;
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let server_db = db.clone();
    tokio::spawn(async move {
        let server = GloveboxMcp::new(server_db)
            .serve(server_io)
            .await
            .expect("server handshake");
        server.waiting().await.expect("server task");
    });
    let client = ().serve(client_io).await.expect("client handshake");
    TestClient { db, client }
}

/// The first text block of a tool result, parsed as JSON when it is JSON and
/// kept as a plain string otherwise.
fn first_text(result: &Value) -> Value {
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("tool result must carry a text block; got: {result}"));
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned()))
}

impl TestClient {
    async fn raw_call(&self, tool: &str, args: Value) -> Result<Value, String> {
        let Value::Object(arguments) = args else {
            panic!("tool arguments must be a JSON object; got: {args}");
        };
        let params = CallToolRequestParams::new(tool.to_owned()).with_arguments(arguments);
        match self.client.call_tool(params).await {
            Ok(result) => Ok(serde_json::to_value(result).expect("serialize tool result")),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Calls a tool and returns its payload. Panics on any error.
    pub async fn call(&self, tool: &str, args: Value) -> Value {
        match self.raw_call(tool, args).await {
            Ok(result) if result["isError"] != true => first_text(&result),
            Ok(result) => panic!("{tool} returned a tool error: {result}"),
            Err(e) => panic!("{tool} failed at the protocol level: {e}"),
        }
    }

    /// Calls a tool that must fail and returns the error text. Panics if the
    /// call succeeds.
    pub async fn call_err(&self, tool: &str, args: Value) -> String {
        match self.raw_call(tool, args).await {
            Ok(result) if result["isError"] == true => match first_text(&result) {
                Value::String(s) => s,
                other => other.to_string(),
            },
            Ok(result) => panic!("{tool} was expected to fail; got: {result}"),
            Err(e) => e,
        }
    }

    pub async fn tools(&self) -> Vec<Value> {
        let tools = self.client.list_all_tools().await.expect("list tools");
        tools
            .iter()
            .map(|t| serde_json::to_value(t).expect("serialize tool"))
            .collect()
    }

    pub async fn tool_names(&self) -> Vec<String> {
        self.tools()
            .await
            .iter()
            .map(|t| t["name"].as_str().expect("tool name").to_owned())
            .collect()
    }

    pub async fn list_resources(&self) -> Vec<Value> {
        let resources = self
            .client
            .list_all_resources()
            .await
            .expect("list resources");
        resources
            .iter()
            .map(|r| serde_json::to_value(r).expect("serialize resource"))
            .collect()
    }

    /// Reads a resource and returns the text of its first content block.
    /// Panics on any error.
    pub async fn read_resource(&self, uri: &str) -> String {
        let result = self
            .client
            .read_resource(ReadResourceRequestParams::new(uri))
            .await
            .unwrap_or_else(|e| panic!("reading {uri} failed: {e}"));
        let value = serde_json::to_value(result).expect("serialize resource result");
        value["contents"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("resource must carry text; got: {value}"))
            .to_owned()
    }

    /// Reads a resource that must fail and returns the error text.
    pub async fn read_resource_err(&self, uri: &str) -> String {
        match self
            .client
            .read_resource(ReadResourceRequestParams::new(uri))
            .await
        {
            Ok(_) => panic!("reading {uri} was expected to fail"),
            Err(e) => e.to_string(),
        }
    }
}

pub fn new_vehicle(name: &str) -> NewVehicle {
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
        notes: None,
        warranty_expires_on: None,
        warranty_expires_miles: None,
    }
}

pub fn minimal_service(date: &str, description: &str) -> NewServiceRecord {
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
        paid_by: None,
        payer_note: None,
        schedule_item_ids: None,
        part_ids: None,
        line_items: None,
        invoice_ref: None,
        end_date: None,
        performed_by: None,
        work_item_ids: None,
        resolved_incident_ids: None,
        noticed_incident_ids: None,
        parts: None,
    }
}

pub fn schedule_item_every_12_months(vehicle_id: i32, name: &str) -> NewScheduleItem {
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
        est_cost_cents: None,
    }
}

/// Status of one named reminder out of a `check_due_maintenance` call.
pub async fn reminder_status(client: &TestClient, vehicle_id: i32, name: &str) -> String {
    let payload = client
        .call(
            "check_due_maintenance",
            serde_json::json!({ "vehicle_id": vehicle_id }),
        )
        .await;
    let reminders = payload["reminders"].as_array().expect("reminders array");
    reminders
        .iter()
        .find(|r| r["schedule_item"]["name"] == name)
        .unwrap_or_else(|| panic!("no reminder named {name}; got: {payload}"))["status"]
        .as_str()
        .expect("status string")
        .to_string()
}
