//! MCP tool + resource handlers. Every tool body is arg-struct →
//! `glovebox_shared::services::*` call → serialize; the single
//! [`domain_error`] helper maps `DomainError` onto the MCP error model.
//! Anything smarter than that belongs in `glovebox-shared`.

use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::{common::FromContextPart, tool::ToolCallContext},
    model::{
        AnnotateAble, CallToolResult, Content, Implementation, ListResourcesResult,
        PaginatedRequestParams, RawResource, ReadResourceRequestParams, ReadResourceResult,
        Resource, ResourceContents, ResourcesCapability, ServerCapabilities, ServerInfo,
        ToolsCapability,
    },
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use sea_orm::DatabaseConnection;
use serde::{Serialize, de::DeserializeOwned};

use glovebox_shared::{
    error::{DomainError, DomainResult},
    inputs::build::UpdateBuild,
    services::{
        activity, build, costs, mileage, observation, reminders, research, search,
        search::SearchScope, service_record, vehicle,
    },
};

use crate::schemas::{
    BuildParams, EmptyParams, FileResearchFindingInput, FindDocumentsInput, LogMileageInput,
    LogObservationInput, RecordServiceInput, SearchRecordsInput, SummarizeActivityInput,
    UpdateBuildStatusInput, VehicleBrief, VehicleParams,
};

pub const VEHICLES_URI: &str = "glovebox://vehicles";

#[derive(Clone)]
pub struct GloveboxMcp {
    db: Arc<DatabaseConnection>,
}

#[tool_router]
impl GloveboxMcp {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db: Arc::new(db) }
    }

    #[tool(
        name = "list_vehicles",
        description = "List the garage — every vehicle's id, name, year, make, and model. Call this first: almost every other tool takes a vehicle_id from here. Use `get_vehicle` for the full record.",
        input_schema = rmcp::handler::server::common::schema_for_type::<EmptyParams>()
    )]
    async fn list_vehicles(
        &self,
        params: LenientParameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = params.into_tool_input("list_vehicles") {
            return Ok(e);
        }
        domain_result(
            vehicle::list(&*self.db)
                .await
                .map(|vs| vs.iter().map(VehicleBrief::from).collect::<Vec<_>>()),
        )
    }

    #[tool(
        name = "get_vehicle",
        description = "Read one vehicle's full record: VIN, engine/transmission/drivetrain, purchase info, plate, color, notes. Call after `list_vehicles` when the conversation needs specifics.",
        input_schema = rmcp::handler::server::common::schema_for_type::<VehicleParams>()
    )]
    async fn get_vehicle(
        &self,
        params: LenientParameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("get_vehicle") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        domain_result(vehicle::get(&*self.db, p.vehicle_id).await)
    }

    #[tool(
        name = "record_service",
        description = "Record maintenance or repair work that was done: what, when, at what mileage, and what it cost (integer cents). Providing `mileage` also logs an odometer reading. Use `line_items` for itemized invoices and `build_id` to link the work to a build project.",
        input_schema = rmcp::handler::server::common::schema_for_type::<RecordServiceInput>()
    )]
    async fn record_service(
        &self,
        params: LenientParameters<RecordServiceInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("record_service") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, input) = p.into_domain();
        if let Err(e) = vehicle::require(&*self.db, vehicle_id).await {
            return domain_error(e);
        }
        domain_result(service_record::create(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "log_observation",
        description = "Note something about a vehicle that isn't a completed repair: a noise, a leak, a warning light, an OBD code, a to-look-into. Use `record_service` for work that was actually done.",
        input_schema = rmcp::handler::server::common::schema_for_type::<LogObservationInput>()
    )]
    async fn log_observation(
        &self,
        params: LenientParameters<LogObservationInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("log_observation") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, input) = p.into_domain();
        if let Err(e) = vehicle::require(&*self.db, vehicle_id).await {
            return domain_error(e);
        }
        domain_result(observation::create(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "log_mileage",
        description = "Record an odometer reading. Keeps mileage-based maintenance reminders accurate — log whenever the user mentions their current mileage. (`record_service` with `mileage` set logs one automatically.)",
        input_schema = rmcp::handler::server::common::schema_for_type::<LogMileageInput>()
    )]
    async fn log_mileage(
        &self,
        params: LenientParameters<LogMileageInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("log_mileage") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, input) = p.into_domain();
        if let Err(e) = vehicle::require(&*self.db, vehicle_id).await {
            return domain_error(e);
        }
        domain_result(mileage::create(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "check_due_maintenance",
        description = "Answer \"what does this car need?\" — the vehicle's maintenance schedule evaluated against its estimated current mileage: each item's status (ok/due_soon/overdue), when it's due, and bundle suggestions for combining work. Call before recommending any maintenance.",
        input_schema = rmcp::handler::server::common::schema_for_type::<VehicleParams>()
    )]
    async fn check_due_maintenance(
        &self,
        params: LenientParameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("check_due_maintenance") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        if let Err(e) = vehicle::require(&*self.db, p.vehicle_id).await {
            return domain_error(e);
        }
        match reminders::calculate_reminders(&self.db, p.vehicle_id).await {
            Ok(v) => tool_json_result(&v),
            Err(e) => Err(db_error(&e)),
        }
    }

    #[tool(
        name = "summarize_recent_activity",
        description = "The vehicle's recent history in one call: services, observations, and odometer readings merged newest-first. Call at conversation start to get context before answering questions about the car.",
        input_schema = rmcp::handler::server::common::schema_for_type::<SummarizeActivityInput>()
    )]
    async fn summarize_recent_activity(
        &self,
        params: LenientParameters<SummarizeActivityInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("summarize_recent_activity") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let limit = p.limit.map_or(activity::DEFAULT_LIMIT, |l| l as usize);
        domain_result(activity::recent(&*self.db, p.vehicle_id, limit).await)
    }

    #[tool(
        name = "find_documents",
        description = "Full-text search a vehicle's documents (receipts, manuals, invoices, photos with extracted text). Returns ranked hits with snippets. Use `search_records` to search other record kinds or the whole garage.",
        input_schema = rmcp::handler::server::common::schema_for_type::<FindDocumentsInput>()
    )]
    async fn find_documents(
        &self,
        params: LenientParameters<FindDocumentsInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("find_documents") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        domain_result(
            search::search(
                &*self.db,
                &p.query,
                SearchScope::Documents,
                Some(p.vehicle_id),
            )
            .await,
        )
    }

    #[tool(
        name = "search_records",
        description = "Full-text search across everything: vehicles, service history, observations, accidents, documents, and research findings. Returns ranked hits with snippets. Narrow with `scope` and/or `vehicle_id`; use `find_documents` for the common documents-only case.",
        input_schema = rmcp::handler::server::common::schema_for_type::<SearchRecordsInput>()
    )]
    async fn search_records(
        &self,
        params: LenientParameters<SearchRecordsInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("search_records") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let scope = match SearchScope::parse(p.scope.as_deref().unwrap_or("all")) {
            Ok(s) => s,
            Err(e) => return domain_error(e),
        };
        domain_result(search::search(&*self.db, &p.query, scope, p.vehicle_id).await)
    }

    #[tool(
        name = "cost_summary",
        description = "What the vehicle has cost: service/parts/labor totals, cost per mile, and a monthly breakdown. All amounts are integer cents.",
        input_schema = rmcp::handler::server::common::schema_for_type::<VehicleParams>()
    )]
    async fn cost_summary(
        &self,
        params: LenientParameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("cost_summary") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        domain_result(costs::summary(&*self.db, p.vehicle_id).await)
    }

    #[tool(
        name = "check_recalls",
        description = "Check NHTSA for open safety recalls on this vehicle (requires make/model/year on the record; makes a live web request). New recalls are also saved as research findings.",
        input_schema = rmcp::handler::server::common::schema_for_type::<VehicleParams>()
    )]
    async fn check_recalls(
        &self,
        params: LenientParameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("check_recalls") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        domain_result(research::check_recalls(&*self.db, p.vehicle_id).await)
    }

    #[tool(
        name = "file_research_finding",
        description = "Save research you've done about this vehicle (forum consensus, TSBs, known issues, upgrade notes) as a persistent finding. Use after answering research questions so the knowledge isn't lost when the conversation ends. Findings appear in the app's Research view.",
        input_schema = rmcp::handler::server::common::schema_for_type::<FileResearchFindingInput>()
    )]
    async fn file_research_finding(
        &self,
        params: LenientParameters<FileResearchFindingInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("file_research_finding") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, input) = p.into_domain();
        domain_result(research::file_finding(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "list_builds",
        description = "List a vehicle's builds — one-shot upgrade/restoration projects (turbo upgrade, engine swap, road-legal) with lifecycle status. Use `get_build_progress` for a build's linked work and spend.",
        input_schema = rmcp::handler::server::common::schema_for_type::<VehicleParams>()
    )]
    async fn list_builds(
        &self,
        params: LenientParameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("list_builds") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        domain_result(build::list(&*self.db, p.vehicle_id).await)
    }

    #[tool(
        name = "get_build_progress",
        description = "How a build is going: linked services, parts (installed vs pending), observations, and total spend (integer cents) — all derived live from the linked records.",
        input_schema = rmcp::handler::server::common::schema_for_type::<BuildParams>()
    )]
    async fn get_build_progress(
        &self,
        params: LenientParameters<BuildParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("get_build_progress") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        domain_result(build::progress(&*self.db, p.vehicle_id, p.build_id).await)
    }

    #[tool(
        name = "update_build_status",
        description = "Move a build through its lifecycle: planned -> active -> completed (or on_hold / abandoned). Use when the user says they're starting, pausing, finishing, or giving up on a project.",
        input_schema = rmcp::handler::server::common::schema_for_type::<UpdateBuildStatusInput>()
    )]
    async fn update_build_status(
        &self,
        params: LenientParameters<UpdateBuildStatusInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("update_build_status") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        domain_result(
            build::update(
                &*self.db,
                p.vehicle_id,
                p.build_id,
                UpdateBuild {
                    status: Some(p.status),
                    ..Default::default()
                },
            )
            .await,
        )
    }
}

#[tool_handler]
impl ServerHandler for GloveboxMcp {
    fn get_info(&self) -> ServerInfo {
        let mut capabilities = ServerCapabilities::default();
        capabilities.tools = Some(ToolsCapability::default());
        capabilities.resources = Some(ResourcesCapability::default());
        ServerInfo::new(capabilities)
            .with_server_info(Implementation::new(
                "glovebox-mcp",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "glovebox MCP: car maintenance tracking. Canonical workflow: (1) ORIENT — \
                 `list_vehicles` for ids, then `summarize_recent_activity` for the vehicle's \
                 recent history. (2) ANSWER \"what does it need?\" — `check_due_maintenance` \
                 before recommending any work; `check_recalls` for safety recalls. (3) CAPTURE \
                 what happened — `record_service` for completed work, `log_observation` for \
                 symptoms/notes, `log_mileage` whenever the user mentions an odometer reading. \
                 (4) LOOK THINGS UP — `find_documents` for receipts/manuals, `search_records` for \
                 anything else, `cost_summary` for spend. (5) PROJECTS — `list_builds` / \
                 `get_build_progress` / `update_build_status` for upgrade or restoration builds. \
                 All money is integer cents; all dates are YYYY-MM-DD.",
            )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        fn json_resource(uri: String, name: String, description: String) -> Resource {
            let mut raw = RawResource::new(uri, name);
            raw.description = Some(description);
            raw.mime_type = Some("application/json".into());
            raw.no_annotation()
        }

        let vehicles = vehicle::list(&*self.db).await.map_err(resource_error)?;
        let builds = build::list_all(&*self.db).await.map_err(resource_error)?;

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
                    "Recent services, observations, and odometer readings for {}, newest first.",
                    v.name
                ),
            ));
        }
        for b in &builds {
            let vehicle_name = vehicles
                .iter()
                .find(|v| v.id == b.vehicle_id)
                .map_or("unknown vehicle", |v| v.name.as_str());
            resources.push(json_resource(
                format!("{VEHICLES_URI}/{}/builds/{}", b.vehicle_id, b.id),
                format!("{} — build: {}", vehicle_name, b.name),
                format!(
                    "Progress for the '{}' build on {}: linked work, parts, and spend.",
                    b.name, vehicle_name
                ),
            ));
        }
        Ok(ListResourcesResult::with_all_items(resources))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let uri = request.uri.as_str();
        let Some(parsed) = parse_resource_uri(uri) else {
            return Err(McpError::invalid_params(
                format!(
                    "unknown resource uri: {uri}. Known forms: {VEHICLES_URI}, \
                     {VEHICLES_URI}/{{id}}, {VEHICLES_URI}/{{id}}/activity, \
                     {VEHICLES_URI}/{{id}}/builds/{{build_id}}. Call resources/list to enumerate \
                     them."
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
            ResourceRef::Build {
                vehicle_id,
                build_id,
            } => {
                let progress = build::progress(&*self.db, vehicle_id, build_id)
                    .await
                    .map_err(resource_error)?;
                resource_json(&progress)?
            }
        };
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(json, &request.uri).with_mime_type("application/json"),
        ]))
    }
}

// ─── Resource URI parsing ───────────────────────────────────────

enum ResourceRef {
    Vehicles,
    Vehicle(i32),
    Activity(i32),
    Build { vehicle_id: i32, build_id: i32 },
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
        (Some("builds"), Some(build), None) => {
            build.parse().ok().map(|build_id| ResourceRef::Build {
                vehicle_id,
                build_id,
            })
        }
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
    Ok(CallToolResult::success(vec![Content::text(json)]))
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
    CallToolResult::error(vec![Content::text(message.into())])
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
        assert!(matches!(
            parse_resource_uri("glovebox://vehicles/7/builds/3"),
            Some(ResourceRef::Build {
                vehicle_id: 7,
                build_id: 3
            })
        ));
    }

    #[test]
    fn rejects_unknown_shapes() {
        for uri in [
            "glovebox://garage",
            "glovebox://vehicles/",
            "glovebox://vehicles/abc",
            "glovebox://vehicles/7/services",
            "glovebox://vehicles/7/builds",
            "glovebox://vehicles/7/builds/x",
            "glovebox://vehicles/7/builds/3/extra",
            "glovebox://vehicles/7/activity/extra",
            "fewd://vehicles",
        ] {
            assert!(parse_resource_uri(uri).is_none(), "must reject {uri}");
        }
    }
}
