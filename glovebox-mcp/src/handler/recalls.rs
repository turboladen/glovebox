//! Tools for the recalls area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use serde::Serialize;

use super::{GloveboxMcp, Parameters, domain_error, domain_result, tool_json_result};
use glovebox_shared::services::{recall, research};

use crate::schemas::{FileResearchFindingInput, SetRecallStatusInput, VehicleParams};

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
                       starting \"NHTSA lookup failed:\"; carry on with those. Call it once per \
                       conversation; `whats_coming` lists the open and planned recalls after \
                       that, and `list_recalls` lists every recall with its status. NHTSA is \
                       queried by make, model and year, so it cannot know whether a recall was \
                       already performed on this car: when an invoice names a campaign number, \
                       match it to a recall here and pass its `finding_id` in `record_service`'s \
                       `recall_finding_ids`. To act on an open recall, `plan_work` it with \
                       `research_finding_id`; completing that work through `record_service` links \
                       the service to the recall. Use `set_recall_status` for a recall that was \
                       done without paperwork or does not apply."
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
                       it does not apply to this car, or `open` to undo either. A recall already \
                       linked to a service stays done until that service is deleted, so changing \
                       it is an error. Returns the recall with its status."
    )]
    async fn set_recall_status(
        &self,
        params: Parameters<SetRecallStatusInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "set_recall_status");
        domain_result(recall::set_status(&*self.db, p.vehicle_id, p.finding_id, &p.status).await)
    }

    #[tool(
        name = "file_research_finding",
        description = "Save research you've done about this vehicle (forum consensus, TSBs, known \
                       issues, upgrade notes) as a persistent finding. Use after answering \
                       research questions so the knowledge isn't lost when the conversation ends."
    )]
    async fn file_research_finding(
        &self,
        params: Parameters<FileResearchFindingInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "file_research_finding");
        let (vehicle_id, input) = p.into_domain();
        domain_result(research::file_finding(&*self.db, vehicle_id, input).await)
    }
}
