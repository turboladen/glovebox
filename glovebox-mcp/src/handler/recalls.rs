//! Tools for the recalls area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use serde::Serialize;

use super::{GloveboxMcp, Parameters, domain_error, domain_result, tool_json_result};
use glovebox_shared::services::{finding, recall, research};

use crate::schemas::{
    FileResearchFindingInput, ListResearchFindingsInput, ResearchFindingParams,
    SetRecallStatusInput, UpdateResearchFindingInput, VehicleParams,
};

#[tool_router(router = recalls_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "check_recalls",
        description = "Check NHTSA for safety recalls on this vehicle (requires make/model/year \
                       on the record; makes a live web request). Returns every known recall with \
                       its status (`open`, `planned`, `done` or `not_applicable`) and the work \
                       item (`work_item_id`) or service linked to it; `newly_found` counts only \
                       recalls stored by this call. If NHTSA cannot be reached, the call still \
                       succeeds with the recalls already known, `newly_found: 0`, and a `message` \
                       starting \"NHTSA lookup failed:\"; carry on with those. NHTSA lists \
                       recalls by model line, so when it does not list the car's own model \
                       `searched_model` names the line it was asked about (`Golf` for a Golf \
                       GTI): say a recall may not apply to this car, and mark one that does not \
                       with `set_recall_status`. Call it once per conversation; `whats_coming` \
                       lists the open and planned recalls after that, and `list_recalls` lists \
                       every recall with its status. NHTSA is queried by make, model and year, so \
                       it cannot know whether a recall was already performed on this car: when an \
                       invoice names a campaign number, match it to a recall here and pass its \
                       `finding_id` in `record_service`'s `recall_finding_ids`. To act on an open \
                       recall, `plan_work` it with `research_finding_id`; completing that work \
                       through `record_service` links the service to the recall. Use \
                       `set_recall_status` for a recall that was done without paperwork or does \
                       not apply."
    )]
    async fn check_recalls(
        &self,
        params: Parameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "check_recalls");
        domain_result(research::check_recalls(&*self.db, p.vehicle_id).await)
    }

    #[tool(
        name = "list_recalls",
        description = "Every recall stored for this vehicle with its status (`open`, `planned`, \
                       `done` or `not_applicable`), campaign number, title, the work item that \
                       addresses or addressed it (`work_item_id`: the planned one, else the \
                       latest completed one) and the service that performed it. Reads only what \
                       is stored and makes no web request. Use this to see recall statuses, \
                       including recalls already done; `check_recalls` is only for asking NHTSA \
                       whether new recalls exist."
    )]
    async fn list_recalls(
        &self,
        params: Parameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        #[derive(Serialize)]
        struct Recalls {
            recalls: Vec<recall::RecallWithStatus>,
        }
        let p = tool_input!(params, "list_recalls");
        match recall::list_for_vehicle(&*self.db, p.vehicle_id).await {
            Ok(recalls) => tool_json_result(&Recalls { recalls }),
            Err(e) => domain_error(e),
        }
    }

    #[tool(
        name = "set_recall_status",
        description = "Mark a recall (a `finding_id` from `whats_coming` or `list_recalls`) \
                       `done` when the work was done but left no paperwork, `not_applicable` when \
                       it does not apply to this car, or `open` to undo either. Undoing a mark \
                       returns the recall to what its jobs say, so it may then read `planned` or \
                       `done` and not `open`. A status the recall already reads changes nothing. \
                       A recall already linked to a service stays done until that service is \
                       deleted, so changing it is an error. A recall with no mark whose job is \
                       done stays done until that job is planned again or dropped, so setting it \
                       `open` is an error. Returns the recall as it reads afterwards."
    )]
    async fn set_recall_status(
        &self,
        params: Parameters<SetRecallStatusInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "set_recall_status");
        domain_result(recall::set_status(&*self.db, p.vehicle_id, p.finding_id, p.status).await)
    }

    #[tool(
        name = "file_research_finding",
        description = "Save research you've done about this vehicle (forum consensus, TSBs, known \
                       issues, upgrade notes) as a persistent finding. Use after answering \
                       research questions so the knowledge isn't lost when the conversation ends. \
                       Returns the finding with its `id` and its status, `open`. A recall is \
                       never filed here: the category `recall` is refused, because recalls come \
                       only from `check_recalls`."
    )]
    async fn file_research_finding(
        &self,
        params: Parameters<FileResearchFindingInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "file_research_finding");
        let (vehicle_id, input) = p.into_domain();
        domain_result(finding::file(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "list_research_findings",
        description = "The research findings saved for this vehicle (forum reports, known issues, \
                       service bulletins, suggested maintenance, upgrade ideas), newest first, as \
                       `findings`. Each has its status (`open`, `planned`, `done` or \
                       `dismissed`), the job filed from it (`work_item`: `id`, `title`, `status` \
                       and `appointment_id`; a planned job on an appointment, else the oldest \
                       planned one, else the latest done one) and the service that completed that \
                       job (`done_service`). Call it before researching a question again, to see \
                       what an earlier conversation saved. Filter with `category`, `status` or \
                       `has_planned_work`. Recalls are never listed here: use `list_recalls`."
    )]
    async fn list_research_findings(
        &self,
        params: Parameters<ListResearchFindingsInput>,
    ) -> Result<CallToolResult, McpError> {
        #[derive(Serialize)]
        struct Findings {
            findings: Vec<finding::FindingView>,
        }
        let p = tool_input!(params, "list_research_findings");
        let (vehicle_id, filter) = p.into_domain();
        match finding::list(&*self.db, vehicle_id, filter).await {
            Ok(findings) => tool_json_result(&Findings { findings }),
            Err(e) => domain_error(e),
        }
    }

    #[tool(
        name = "get_research_finding",
        description = "One research finding with its status, the job filed from it (`work_item`) \
                       and the service that completed that job (`done_service`). A recall's id is \
                       not found here: read a recall with `list_recalls`."
    )]
    async fn get_research_finding(
        &self,
        params: Parameters<ResearchFindingParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "get_research_finding");
        domain_result(finding::get(&*self.db, p.vehicle_id, p.finding_id).await)
    }

    #[tool(
        name = "update_research_finding",
        description = "Correct a research finding's title, description, category, severity or \
                       source URL, or set its status: `done` when it was acted on and no job \
                       records that, `dismissed` when the owner decided against it, `open` to \
                       undo either. Undoing a mark returns the finding to what its jobs say, so \
                       it may then read `planned` or `done` rather than `open`. `planned` is \
                       never set: a finding reads it while a planned job addresses it \
                       (`plan_work` with `research_finding_id`), and reads `done` once that job \
                       is done. A status the finding already reads changes nothing. A finding \
                       with no mark whose job is done stays done until that job is planned again \
                       or dropped, so setting it `open` is an error. A finding cannot become a \
                       recall, and a recall's id is not found here: use `set_recall_status` for a \
                       recall. Returns the finding as it reads afterwards."
    )]
    async fn update_research_finding(
        &self,
        params: Parameters<UpdateResearchFindingInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "update_research_finding");
        let (vehicle_id, finding_id, input) = p.into_domain();
        domain_result(finding::update(&*self.db, vehicle_id, finding_id, input).await)
    }

    #[tool(
        name = "delete_research_finding",
        description = "Permanently delete a research finding that was filed by mistake or is \
                       wrong. Every job planned from it is kept with its status and stops naming \
                       the finding; the reply lists those jobs in `unlinked_work_items`, so tell \
                       the owner which ones. For a finding the owner decided against, set it \
                       `dismissed` with `update_research_finding` instead, which keeps the \
                       record. A recall cannot be deleted: its id is not found here."
    )]
    async fn delete_research_finding(
        &self,
        params: Parameters<ResearchFindingParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "delete_research_finding");
        domain_result(
            finding::delete(&*self.db, p.vehicle_id, p.finding_id)
                .await
                .map(|out| {
                    serde_json::json!({
                        "deleted": "research finding",
                        "id": out.id,
                        "title": out.title,
                        "unlinked_work_items": out.unlinked_work_items,
                    })
                }),
        )
    }
}
