//! MCP tool + resource handlers. Every tool body is arg-struct →
//! `glovebox_shared::services::*` call → serialize; the single
//! [`domain_error`] helper maps `DomainError` onto the MCP error model.
//! Anything smarter than that belongs in `glovebox-shared`.
//!
//! Each area module defines its tools in its own `#[tool_router]` block;
//! [`GloveboxMcp::tool_router`] adds those routers together.

use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::{common::FromContextPart, router::tool::ToolRouter, tool::ToolCallContext},
    model::{
        CallToolResult, ContentBlock, Implementation, ListResourcesResult, PaginatedRequestParams,
        ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource,
        ResourceContents, ResourcesCapability, ServerCapabilities, ServerConfig, ToolsCapability,
    },
    service::RequestContext,
    tool_handler,
};
use sea_orm::DatabaseConnection;
use serde::{Serialize, de::DeserializeOwned};

use glovebox_shared::{
    error::{DomainError, DomainResult},
    services::{activity, vehicle},
};

use crate::schemas::VehicleBrief;

mod incidents;
mod parts;
mod planning;
mod reads;
mod recalls;
mod schedule;
mod services;
mod vehicles;

pub const VEHICLES_URI: &str = "glovebox://vehicles";

#[derive(Clone)]
pub struct GloveboxMcp {
    db: Arc<DatabaseConnection>,
}

impl GloveboxMcp {
    #[must_use]
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db: Arc::new(db) }
    }

    /// Every tool the server offers, combined from the per-area routers.
    fn tool_router() -> ToolRouter<Self> {
        Self::vehicles_router()
            + Self::services_router()
            + Self::parts_router()
            + Self::incidents_router()
            + Self::planning_router()
            + Self::schedule_router()
            + Self::recalls_router()
            + Self::reads_router()
    }
}

// The `#[tool_handler]` expansion generates async trait methods that contain no `.await`.
#[allow(clippy::unused_async_trait_impl)]
#[tool_handler]
impl ServerHandler for GloveboxMcp {
    fn get_info(&self) -> ServerConfig {
        let mut capabilities = ServerCapabilities::default();
        capabilities.tools = Some(ToolsCapability::default());
        capabilities.resources = Some(ResourcesCapability::default());
        ServerConfig::new(capabilities)
            .with_server_info(Implementation::new(
                "glovebox-mcp",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "glovebox MCP: car maintenance tracking, driven entirely through these tools. \
                 Start with `list_vehicles` for ids (`add_vehicle` for a new car, \
                 `list_model_templates` to give it an inherited schedule). Three loops cover \
                 almost everything. (1) FILE AN INVOICE: `record_service` once per invoice, with \
                 `invoice_ref`, `line_items` (each with a `work_kind`), `parts`, and the ids of \
                 anything the work closes: `schedule_item_ids` from `whats_coming`, \
                 `work_item_ids` from `list_planned_work`, `resolved_incident_ids`, \
                 `recall_finding_ids` from `check_recalls`. Filing the same `invoice_ref` again \
                 returns the existing record; fix a record with `correct_service`, remove one \
                 with `delete_service` (which reopens what it closed). (2) CAPTURE AN ADVISORY: \
                 when a shop flags something it did not fix, `log_incident` with category \
                 `shop_observation` and `noticed_during_service_id`, then `plan_work` the fix \
                 with `revisit_by_date` or `revisit_by_miles`; progress it with \
                 `update_incident`, `add_incident_followup` and `update_planned_work`. (3) ANSWER \
                 \"WHAT'S COMING?\": call `whats_coming` before recommending any work; it covers \
                 the schedule, planned work, open recalls, unresolved incidents, the 12-month \
                 forecast and warranty status. Tune the schedule with `list_schedule`, \
                 `update_schedule_item` (any item, inherited ones included; history carries \
                 over), `add_schedule_item` for a job the schedule lacks, and \
                 `dismiss_schedule_item` / `restore_schedule_item`. Also: `log_mileage` whenever \
                 the user mentions an odometer reading, `record_part` and `list_parts` / \
                 `correct_part` / `delete_part` for parts, `save_note` for facts worth keeping, \
                 `search_records` and `cost_summary` to look things up, `check_recalls` and \
                 `set_recall_status` for recalls. Every record can be listed, corrected and \
                 deleted; a correct or update tool leaves absent fields alone and clears a field \
                 sent as null. All money is integer cents; all dates are YYYY-MM-DD.",
            )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        fn json_resource(uri: String, name: String, description: String) -> Resource {
            Resource::new(uri, name)
                .with_description(description)
                .with_mime_type("application/json")
        }

        let vehicles = vehicle::list(&*self.db).await.map_err(resource_error)?;

        let mut resources: Vec<Resource> = vec![json_resource(
            VEHICLES_URI.to_string(),
            "vehicles".to_string(),
            "Every vehicle in the garage (id, name, year, make, model).".to_string(),
        )];
        for v in &vehicles {
            resources.push(json_resource(
                format!("{VEHICLES_URI}/{}", v.id),
                v.name.clone(),
                format!("Full vehicle record for {}.", v.name),
            ));
            resources.push(json_resource(
                format!("{VEHICLES_URI}/{}/activity", v.id),
                format!("{} — recent activity", v.name),
                format!(
                    "Recent services, incidents, and odometer readings for {}, newest first.",
                    v.name
                ),
            ));
        }
        Ok(ListResourcesResult::with_all_items(resources))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let uri = request.uri.as_str();
        let Some(parsed) = parse_resource_uri(uri) else {
            return Err(McpError::invalid_params(
                format!(
                    "unknown resource uri: {uri}. Known forms: {VEHICLES_URI}, \
                     {VEHICLES_URI}/{{id}}, {VEHICLES_URI}/{{id}}/activity. Call resources/list \
                     to enumerate them."
                ),
                None,
            ));
        };

        let json = match parsed {
            ResourceRef::Vehicles => {
                let vehicles = vehicle::list(&*self.db).await.map_err(resource_error)?;
                let briefs: Vec<VehicleBrief> = vehicles.iter().map(VehicleBrief::from).collect();
                resource_json(&briefs)?
            }
            ResourceRef::Vehicle(id) => {
                let v = vehicle::get(&*self.db, id).await.map_err(resource_error)?;
                resource_json(&v)?
            }
            ResourceRef::Activity(id) => {
                let feed = activity::recent(&*self.db, id, activity::DEFAULT_LIMIT)
                    .await
                    .map_err(resource_error)?;
                resource_json(&feed)?
            }
        };
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(json, &request.uri).with_mime_type("application/json"),
        ])
        .into())
    }
}

// ─── Resource URI parsing ───────────────────────────────────────

enum ResourceRef {
    Vehicles,
    Vehicle(i32),
    Activity(i32),
}

/// Parse a `glovebox://vehicles[/…]` URI. `None` means the shape is unknown
/// (the caller reports the known forms); existence checks happen later.
fn parse_resource_uri(uri: &str) -> Option<ResourceRef> {
    let rest = uri.strip_prefix(VEHICLES_URI)?;
    if rest.is_empty() {
        return Some(ResourceRef::Vehicles);
    }
    let mut parts = rest.strip_prefix('/')?.split('/');
    let vehicle_id: i32 = parts.next()?.parse().ok()?;
    match (parts.next(), parts.next(), parts.next()) {
        (None, ..) => Some(ResourceRef::Vehicle(vehicle_id)),
        (Some("activity"), None, _) => Some(ResourceRef::Activity(vehicle_id)),
        _ => None,
    }
}

// ─── Error mapping & result helpers ─────────────────────────────

/// THE `DomainError` → MCP mapping for tools; every tool routes its service
/// errors through here (no per-tool matching). LLM-recoverable failures
/// (`NotFound`, `Invalid`, `BadRequest`) become tool-level error results so
/// the actionable message reaches the model; `Db`/`Internal` become opaque
/// JSON-RPC internal errors with the detail kept server-side in tracing.
fn domain_error(err: DomainError) -> Result<CallToolResult, McpError> {
    match err {
        DomainError::NotFound(_) | DomainError::Invalid { .. } | DomainError::BadRequest(_) => {
            Ok(tool_user_error(err.to_string()))
        }
        DomainError::Db(e) => Err(db_error(&e)),
        DomainError::Internal(detail) => Err(internal_error(&detail)),
    }
}

/// Serialize a service result or route its error through [`domain_error`].
fn domain_result<T: Serialize>(result: DomainResult<T>) -> Result<CallToolResult, McpError> {
    match result {
        Ok(v) => tool_json_result(&v),
        Err(e) => domain_error(e),
    }
}

/// [`domain_error`]'s counterpart for the resource handlers, where there is
/// no tool-level error channel: `NotFound` maps to the MCP
/// `resource_not_found` error, other client-correctable failures to
/// `invalid_params`, and `Db`/`Internal` stay opaque.
fn resource_error(err: DomainError) -> McpError {
    match err {
        DomainError::NotFound(msg) => McpError::resource_not_found(msg, None),
        DomainError::Invalid { .. } | DomainError::BadRequest(_) => {
            McpError::invalid_params(err.to_string(), None)
        }
        DomainError::Db(e) => db_error(&e),
        DomainError::Internal(detail) => internal_error(&detail),
    }
}

fn tool_json_result<T: Serialize>(value: &T) -> Result<CallToolResult, McpError> {
    let json = serde_json::to_string_pretty(value).map_err(|err| {
        tracing::error!(?err, "MCP tool: failed to serialize result");
        McpError::internal_error("failed to serialize result", None)
    })?;
    Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
}

/// Serializes the result of a delete as `{"deleted": "<kind>", "id": <id>}`,
/// or routes the service error through [`domain_error`].
fn deleted_result(
    kind: &str,
    id: i32,
    result: DomainResult<()>,
) -> Result<CallToolResult, McpError> {
    domain_result(result.map(|()| serde_json::json!({ "deleted": kind, "id": id })))
}

fn resource_json<T: Serialize>(value: &T) -> Result<String, McpError> {
    serde_json::to_string_pretty(value).map_err(|err| {
        tracing::error!(?err, "MCP resource: failed to serialize");
        McpError::internal_error("failed to serialize resource", None)
    })
}

// `db_error` / `internal_error` return a fixed wire message: SeaORM's `DbErr`
// Display embeds SQLite detail (column/constraint names, sometimes values)
// and `Internal` carries formatted internal state. The verbose detail goes
// to tracing (operator side); the JSON-RPC client sees the opaque label.
fn db_error(err: &sea_orm::DbErr) -> McpError {
    tracing::error!(?err, "MCP tool: database error");
    McpError::internal_error("database error", None)
}

fn internal_error(detail: &str) -> McpError {
    tracing::error!(%detail, "MCP tool: internal error");
    McpError::internal_error("internal server error", None)
}

/// Build a tool-level error result so the actionable message reaches the
/// LLM (protocol-level `Err(McpError)` is rendered by most clients as a
/// generic "tool failed" with the message dropped).
fn tool_user_error(message: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message.into())])
}

/// Parameter wrapper that defers deserialize errors to the handler so
/// malformed arguments come back as tool-level errors (with the schema
/// hint) instead of bare JSON-RPC `-32602`s that clients render as a
/// generic failure. Pattern lifted from fewd's MCP handler.
///
/// The `#[tool]` attribute MUST also set
/// `input_schema = rmcp::handler::server::common::schema_for_type::<T>()`:
/// rmcp's macro only auto-derives schemas from a literal `Parameters<T>`
/// in the signature, so without the override the tool advertises an empty
/// input schema.
pub(crate) struct LenientParameters<T>(Result<T, String>);

impl<T> LenientParameters<T> {
    /// Unwrap into the deserialized value or a tool-level error naming the
    /// tool. Pairs with the early-return pattern in every handler.
    pub(crate) fn into_tool_input(self, tool_name: &'static str) -> Result<T, CallToolResult> {
        self.0.map_err(|e| {
            tool_user_error(format!("{tool_name}: {e}. Check the tool's input schema."))
        })
    }
}

impl<S, T> FromContextPart<ToolCallContext<'_, S>> for LenientParameters<T>
where
    T: DeserializeOwned,
{
    fn from_context_part(context: &mut ToolCallContext<S>) -> Result<Self, McpError> {
        let arguments = context.arguments.take().unwrap_or_default();
        let parsed = serde_json::from_value::<T>(serde_json::Value::Object(arguments))
            .map_err(|e| e.to_string());
        Ok(Self(parsed))
    }
}

#[cfg(test)]
mod tests {
    use super::{ResourceRef, parse_resource_uri};

    #[test]
    fn parses_all_known_uri_shapes() {
        assert!(matches!(
            parse_resource_uri("glovebox://vehicles"),
            Some(ResourceRef::Vehicles)
        ));
        assert!(matches!(
            parse_resource_uri("glovebox://vehicles/7"),
            Some(ResourceRef::Vehicle(7))
        ));
        assert!(matches!(
            parse_resource_uri("glovebox://vehicles/7/activity"),
            Some(ResourceRef::Activity(7))
        ));
    }

    #[test]
    fn rejects_unknown_shapes() {
        for uri in [
            "glovebox://garage",
            "glovebox://vehicles/",
            "glovebox://vehicles/abc",
            "glovebox://vehicles/7/services",
            "glovebox://vehicles/7/builds/3",
            "glovebox://vehicles/7/activity/extra",
            "fewd://vehicles",
        ] {
            assert!(parse_resource_uri(uri).is_none(), "must reject {uri}");
        }
    }
}
