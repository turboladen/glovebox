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
    config::AppConfig,
    entities::work_item,
    error::{DomainError, DomainResult},
    inputs::build::UpdateBuild,
    services::{
        activity, budget, budget::BudgetForecast, build, costs, document, incident, mileage, part,
        reminders, reminders::RemindersResponse, research, schedule, search, search::SearchScope,
        service_record, service_record::LinkMode, vehicle, visit, visit::VisitWithItems,
        work_item as work_item_svc,
    },
};

use crate::schemas::{
    AttachDocumentInput, BuildParams, CancelVisitInput, CompleteVisitInput,
    DismissScheduleItemInput, EmptyParams, FileResearchFindingInput, FindDocumentsInput,
    LinkServiceToMaintenanceInput, ListPlannedWorkInput, LogIncidentInput, LogMileageInput,
    PlanWorkInput, RecordPartInput, RecordServiceInput, SaveNoteInput, ScheduleVisitInput,
    SearchRecordsInput, SummarizeActivityInput, UpdateBuildStatusInput, VehicleBrief,
    VehicleParams,
};

pub const VEHICLES_URI: &str = "glovebox://vehicles";

#[derive(Clone)]
pub struct GloveboxMcp {
    db: Arc<DatabaseConnection>,
    config: Arc<AppConfig>,
    /// Comma-joined candidate base URLs for the HTTP multipart upload route
    /// (non-loopback IPv4 addresses + localhost, port from `config.listen`),
    /// baked
    /// into the served instructions so sandboxed-but-networked clients can
    /// reach `/api/documents` directly. Built once in [`crate::router`].
    base_urls: Arc<str>,
}

#[tool_router]
impl GloveboxMcp {
    pub fn new(db: DatabaseConnection, config: Arc<AppConfig>, base_urls: Arc<str>) -> Self {
        Self {
            db: Arc::new(db),
            config,
            base_urls,
        }
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
        description = "Record maintenance or repair work that was done: what, when, at what mileage, and what it cost (integer cents). Providing `mileage` also logs an odometer reading. Use `line_items` for itemized invoices and `build_id` to link the work to a build project. If the work satisfies due/overdue schedule items from `check_due_maintenance`, pass their ids in `schedule_item_ids` so the reminders clear; for records you already created (e.g. a bulk import), reconcile afterwards with `link_service_to_maintenance` instead of re-recording. When importing a scanned invoice, follow up with `attach_document` (linked to the returned record id) so the original file and its text are kept; if you can't reach the file, hand the user the browser deep link returned alongside this record so they can drag-drop the original from their (non-sandboxed) browser.",
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
        match service_record::create(&*self.db, vehicle_id, input).await {
            Ok(rec) => {
                // Hand the user a browser deep link to attach the original
                // invoice/receipt file: their (non-sandboxed) browser can
                // upload the bytes where the model context can't carry them.
                // Path-mode router (history, not hash) — NO `/#/`; the
                // backend's SPA fallback serves index.html for this path.
                let link = format!(
                    "{}/vehicles/{}/records/documents?attach=service:{}",
                    self.config.public_url.trim_end_matches('/'),
                    rec.record.vehicle_id,
                    rec.record.id,
                );
                let note = format!(
                    "The service record is filed. The invoice/receipt FILE cannot be attached \
                     through this chat (its bytes can't cross the sandbox). Give the user this \
                     link — opening it in their browser lands on a drop zone already scoped to \
                     this record, where they drag the file in: {link} — do NOT call \
                     attach_document for a file that is only in this chat."
                );
                tool_json_result_with_link(&rec, note)
            }
            Err(e) => domain_error(e),
        }
    }

    #[tool(
        name = "record_part",
        description = "Record a part you bought or installed for this vehicle — purchase info, cost (integer cents), where it goes (location), and optional links to the installing service or a build project. Use record_service for the labor; this is the part itself.",
        input_schema = rmcp::handler::server::common::schema_for_type::<RecordPartInput>()
    )]
    async fn record_part(
        &self,
        params: LenientParameters<RecordPartInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("record_part") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, input) = p.into_domain();
        if let Err(e) = vehicle::require(&*self.db, vehicle_id).await {
            return domain_error(e);
        }
        domain_result(part::create(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "log_incident",
        description = "Log something that happened to a vehicle that isn't a completed repair: a noise, a leak, a warning light, an OBD code, damage, a collision. Collisions/crashes with another party go here with category `accident`. If it's the same problem coming back, set `recurrence_of_id` to the earlier incident. Use `record_service` for work that was actually done, and `save_note` for plain facts to remember.",
        input_schema = rmcp::handler::server::common::schema_for_type::<LogIncidentInput>()
    )]
    async fn log_incident(
        &self,
        params: LenientParameters<LogIncidentInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("log_incident") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, input) = p.into_domain();
        if let Err(e) = vehicle::require(&*self.db, vehicle_id).await {
            return domain_error(e);
        }
        domain_result(incident::create(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "save_note",
        description = "Remember something about this vehicle — a fact, a preference, a memory. Saved as a note incident, searchable later via `search_records`.",
        input_schema = rmcp::handler::server::common::schema_for_type::<SaveNoteInput>()
    )]
    async fn save_note(
        &self,
        params: LenientParameters<SaveNoteInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("save_note") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, input) = p.into_domain();
        if let Err(e) = vehicle::require(&*self.db, vehicle_id).await {
            return domain_error(e);
        }
        domain_result(incident::create(&*self.db, vehicle_id, input).await)
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
        description = "Answer \"what does this car need?\" — the vehicle's maintenance schedule evaluated against its estimated current mileage: each item's status (ok/due_soon/overdue), when it's due, bundle suggestions for combining work, a 12-month budget forecast, and the warranty status (possibly_covered — remind the user to check coverage before paying). Call before recommending any maintenance. For due/overdue items, offer to plan_work them (linked via schedule_item_id) so the work lands on the to-do list; when recorded work satisfies an item, link it via `record_service`'s `schedule_item_ids` so the reminder clears — or, for service records that already exist (an import you just finished), call `link_service_to_maintenance` with the matching schedule item ids to reconcile them all at once; for an item that doesn't apply to this vehicle, use `dismiss_schedule_item`.",
        input_schema = rmcp::handler::server::common::schema_for_type::<VehicleParams>()
    )]
    async fn check_due_maintenance(
        &self,
        params: LenientParameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        // Composed payload (unit G): reminders (incl. warranty) + the
        // 12-month budget forecast in one call.
        #[derive(Serialize)]
        struct DueMaintenance {
            #[serde(flatten)]
            reminders: RemindersResponse,
            budget: BudgetForecast,
        }
        let p = match params.into_tool_input("check_due_maintenance") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let reminders = match reminders::calculate_reminders(&self.db, p.vehicle_id).await {
            Ok(v) => v,
            Err(e) => return domain_error(e),
        };
        // Reminders are computed ONCE and feed both halves of the payload.
        let budget = match budget::forecast_from(&self.db, p.vehicle_id, &reminders).await {
            Ok(v) => v,
            Err(e) => return domain_error(e),
        };
        tool_json_result(&DueMaintenance { reminders, budget })
    }

    #[tool(
        name = "dismiss_schedule_item",
        description = "Waive a maintenance schedule item for this vehicle — it stops appearing in the schedule and in `check_due_maintenance`. Use for items that genuinely don't apply (e.g. a dealer-only service the owner skips deliberately). For work that was actually done before tracking started, prefer `record_service` with a minimal past-dated entry linked via `schedule_item_ids` instead, so history stays honest.",
        input_schema = rmcp::handler::server::common::schema_for_type::<DismissScheduleItemInput>()
    )]
    async fn dismiss_schedule_item(
        &self,
        params: LenientParameters<DismissScheduleItemInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("dismiss_schedule_item") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        domain_result(
            schedule::dismiss_for_vehicle(&*self.db, p.vehicle_id, p.schedule_item_id, p.reason)
                .await,
        )
    }

    #[tool(
        name = "summarize_recent_activity",
        description = "The vehicle's recent history in one call: services, incidents, and odometer readings merged newest-first. Call at conversation start to get context before answering questions about the car.",
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
        description = "Full-text search across everything: vehicles, service history, incidents (and their followups), builds, documents, and research findings. Returns ranked hits with snippets. Narrow with `scope` and/or `vehicle_id`; use `find_documents` for the common documents-only case.",
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
        description = "What the vehicle has cost: service/parts/labor totals, an out-of-pocket vs covered-by-others split (insurance/third-party-paid services), cost per mile, and a monthly breakdown. All amounts are integer cents.",
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
        description = "Check NHTSA for open safety recalls on this vehicle (requires make/model/year on the record; makes a live web request). New recalls are also saved as research findings. When a recall needs action, offer to plan_work it (linked via research_finding_id) — completing that work later closes the recall automatically.",
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
        description = "How a build is going: linked services, parts (installed vs pending), incidents, and total spend (integer cents) — all derived live from the linked records.",
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

    #[tool(
        name = "plan_work",
        description = "Add something to the vehicle's to-do list — work the user intends to do or have done. Link the source so completing the work closes the loop (a recall finding from check_recalls, an overdue schedule item from check_due_maintenance, an incident, a build). Group planned items into a visit with schedule_visit; close them out with complete_visit.",
        input_schema = rmcp::handler::server::common::schema_for_type::<PlanWorkInput>()
    )]
    async fn plan_work(
        &self,
        params: LenientParameters<PlanWorkInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("plan_work") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, input) = p.into_domain();
        domain_result(work_item_svc::create(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "list_planned_work",
        description = "The vehicle's planning state: open work items (the to-do list) and open visits with their attached items and estimated-cost rollups. Set include_done to also see finished/dropped work and completed/canceled visits.",
        input_schema = rmcp::handler::server::common::schema_for_type::<ListPlannedWorkInput>()
    )]
    async fn list_planned_work(
        &self,
        params: LenientParameters<ListPlannedWorkInput>,
    ) -> Result<CallToolResult, McpError> {
        #[derive(Serialize)]
        struct PlannedWork {
            items: Vec<work_item::Model>,
            visits: Vec<VisitWithItems>,
        }
        let p = match params.into_tool_input("list_planned_work") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let include_done = p.include_done.unwrap_or(false);
        let items = match work_item_svc::list(&*self.db, p.vehicle_id, include_done).await {
            Ok(v) => v,
            Err(e) => return domain_error(e),
        };
        let visits = match visit::list(&*self.db, p.vehicle_id, include_done).await {
            Ok(v) => v,
            Err(e) => return domain_error(e),
        };
        tool_json_result(&PlannedWork { items, visits })
    }

    #[tool(
        name = "schedule_visit",
        description = "Group planned work into a shop visit (or DIY session) with a date and estimated cost. Attached work items (from plan_work) flip to scheduled and their est_cost_cents roll up. Name the shop with shop_name (free text; shop_id also accepts a saved shop from the shops list). When the visit happens, close it out with complete_visit; if it won't happen, cancel_visit returns the items to the to-do list.",
        input_schema = rmcp::handler::server::common::schema_for_type::<ScheduleVisitInput>()
    )]
    async fn schedule_visit(
        &self,
        params: LenientParameters<ScheduleVisitInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("schedule_visit") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, input) = p.into_domain();
        domain_result(visit::create(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "complete_visit",
        description = "Close out a visit with the actuals: creates the service record (payer-aware), clears satisfied reminders via the items' schedule links, resolves linked recalls and incidents, and marks the work done — one atomic operation. Providing `mileage` also logs an odometer reading.",
        input_schema = rmcp::handler::server::common::schema_for_type::<CompleteVisitInput>()
    )]
    async fn complete_visit(
        &self,
        params: LenientParameters<CompleteVisitInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("complete_visit") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, visit_id, input) = p.into_domain();
        domain_result(visit::complete(&*self.db, vehicle_id, visit_id, input).await)
    }

    #[tool(
        name = "cancel_visit",
        description = "Cancel a visit that won't happen; its work items return to the to-do list. Completed visits are history and can't be canceled — nothing that already happened is undone.",
        input_schema = rmcp::handler::server::common::schema_for_type::<CancelVisitInput>()
    )]
    async fn cancel_visit(
        &self,
        params: LenientParameters<CancelVisitInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("cancel_visit") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        domain_result(visit::cancel(&*self.db, p.vehicle_id, p.visit_id).await)
    }

    #[tool(
        name = "attach_document",
        description = "Store a file that is ALREADY on the server's disk against a vehicle: the server reads the bytes off its own inbox and creates a document record. **Do NOT call this tool for a file the user uploaded to THIS CHAT** — that file lives in your sandbox, not the server's inbox, and its bytes cannot cross to the server. In that (common) case, don't attempt an attach at all: `record_service` returns a browser deep link — give that link to the user and they attach the file themselves in one drag. There is NO inline-bytes option; files are attached ONLY by `source_path`, resolved on the SERVER inside its inbox directory. Legitimate uses: (a) the file is already in the inbox (the user put it there or told you its name) → pass that name verbatim as `source_path`; (b) your file tools genuinely share this server's filesystem → save it into the inbox first, then `source_path`; (c) your shell has network to the server → `curl -F` multipart to `POST /api/documents` instead (base URLs in the server instructions). NEVER compress, downsize, or re-emit an existing file to force it through. Max 10 MiB. Pass `extracted_text` (the text you read out) so it's findable via `find_documents`, and link it via `linked_entity_type` + `linked_entity_id`.",
        input_schema = rmcp::handler::server::common::schema_for_type::<AttachDocumentInput>()
    )]
    async fn attach_document(
        &self,
        params: LenientParameters<AttachDocumentInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("attach_document") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let input = match p.into_domain() {
            Ok(v) => v,
            Err(e) => return domain_error(e),
        };
        domain_result(document::store(&*self.db, &self.config, input).await)
    }

    #[tool(
        name = "link_service_to_maintenance",
        description = "Link an EXISTING service record to maintenance schedule items (from `check_due_maintenance`) so the matching reminders clear — the reconciliation step after an import, when the records were created without `schedule_item_ids`. Default mode `add` unions with the record's existing links (safe to call repeatedly while working through a list); mode `replace` overwrites them. For new work, prefer passing `schedule_item_ids` to `record_service` directly.",
        input_schema = rmcp::handler::server::common::schema_for_type::<LinkServiceToMaintenanceInput>()
    )]
    async fn link_service_to_maintenance(
        &self,
        params: LenientParameters<LinkServiceToMaintenanceInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("link_service_to_maintenance") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let mode = match LinkMode::parse(p.mode.as_deref().unwrap_or("add")) {
            Ok(m) => m,
            Err(e) => return domain_error(e),
        };
        domain_result(
            service_record::link_schedule_items(
                &*self.db,
                p.vehicle_id,
                p.service_record_id,
                &p.schedule_item_ids,
                mode,
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
            .with_instructions(format!(
                "glovebox MCP: car maintenance tracking. Canonical workflow: (1) ORIENT — \
                 `list_vehicles` for ids, then `summarize_recent_activity` for the vehicle's \
                 recent history. (2) ANSWER \"what does it need?\" — `check_due_maintenance` \
                 before recommending any work (it includes the 12-month budget forecast and \
                 warranty status); `check_recalls` for safety recalls. Resolve due/overdue items \
                 by linking completed work (`record_service` with `schedule_item_ids`, or \
                 `link_service_to_maintenance` for records that already exist — the import \
                 reconciliation loop) or waiving ones that don't apply (`dismiss_schedule_item`). \
                 (3) PLAN — when the user intends to do something about a recall, an overdue \
                 item, or an incident, `plan_work` it with the source linked; group items into a \
                 shop visit or DIY session with `schedule_visit`; review with \
                 `list_planned_work`; when the work happens, `complete_visit` records the \
                 service, clears the reminders, and closes the linked recalls/incidents in one \
                 step; a visit that won't happen is `cancel_visit`-ed and its items return to the \
                 to-do list. (4) CAPTURE what happened outside a visit — `record_service` for \
                 completed work, `record_part` for parts bought or installed, `log_incident` for \
                 symptoms/damage/accidents, `save_note` for facts worth remembering, \
                 `log_mileage` whenever the user mentions an odometer reading, `attach_document` \
                 for a file ALREADY on the server. ATTACHING A FILE — decide by WHERE the file \
                 is, in this order: (1) THE COMMON CASE — the user uploaded/attached the file to \
                 THIS chat. Its bytes live in your sandbox and CANNOT reach the server; do NOT \
                 try to move them and do NOT call `attach_document`. `record_service` already \
                 returned a browser deep link — hand THAT link to the user; their (non-sandboxed) \
                 browser opens a drop zone pre-scoped to the record and they drag the file in. \
                 This is the primary path — reach for it first. (2) The file is genuinely already \
                 in the server inbox `{inbox_dir}` (the user put it there or told you its name) → \
                 `attach_document` with `source_path` = that name. (3) Your file tools truly \
                 share the server's filesystem → save into `{inbox_dir}`, then `source_path`. (4) \
                 Your shell has network TO THE SERVER → `curl -sS -F \"file=@<path>\" -F \
                 \"vehicle_id=<id>\" -F \"linked_entity_type=service\" -F \
                 \"linked_entity_id=<id>\" -F \"extracted_text=<text>\" <BASE>/api/documents` \
                 (candidate <BASE>, first whose `/api/health` answers: {base_urls}). NEVER \
                 compress, downsize, or re-emit a file to force it through — if none of (2)–(4) \
                 apply, use (1). Whichever route, pass `extracted_text` and link the service \
                 record so one conversation yields record + document. (5) LOOK THINGS UP — \
                 `find_documents` for receipts/manuals, `search_records` for anything else, \
                 `cost_summary` for spend. (6) PROJECTS — `list_builds` / `get_build_progress` / \
                 `update_build_status` for upgrade or restoration builds. All money is integer \
                 cents; all dates are YYYY-MM-DD.",
                inbox_dir = self.config.inbox_dir,
                base_urls = self.base_urls,
            ))
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
                    "Recent services, incidents, and odometer readings for {}, newest first.",
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

/// Like [`tool_json_result`] but appends a SECOND text content block carrying
/// a human note + deep link. A standalone text block is the surest way most
/// MCP clients surface the URL as a clickable link (rather than burying it in
/// the JSON payload) — used by `record_service` to hand the user a browser
/// upload link their sandboxed client can't reach.
fn tool_json_result_with_link<T: Serialize>(
    value: &T,
    note_and_link: String,
) -> Result<CallToolResult, McpError> {
    let json = serde_json::to_string_pretty(value).map_err(|err| {
        tracing::error!(?err, "MCP tool: failed to serialize result");
        McpError::internal_error("failed to serialize result", None)
    })?;
    Ok(CallToolResult::success(vec![
        Content::text(json),
        Content::text(note_and_link),
    ]))
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
