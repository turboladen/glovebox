//! Tools for the recalls area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, LenientParameters, domain_result};
use glovebox_shared::services::{recall, research};

use crate::schemas::{FileResearchFindingInput, SetRecallStatusInput, VehicleParams};

#[tool_router(router = recalls_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "check_recalls",
        description = "Check NHTSA for safety recalls on this vehicle (requires make/model/year on the record; makes a live web request). Returns every known recall with its status (`open`, `planned`, `done` or `not_applicable`) and the work item or service linked to it; `newly_found` counts only recalls stored by this call. If NHTSA cannot be reached, the call still succeeds with the recalls already known, `newly_found: 0`, and a `message` starting \"NHTSA lookup failed:\"; carry on with those. Call it once per conversation; `whats_coming` lists the open recalls after that. NHTSA is queried by make, model and year, so it cannot know whether a recall was already performed on this car: when an invoice names a campaign number, match it to a recall here and pass its `finding_id` in `record_service`'s `recall_finding_ids`. To act on an open recall, `plan_work` it with `research_finding_id`; completing that work through `record_service` links the service to the recall. Use `set_recall_status` for a recall that was done without paperwork or does not apply.",
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
        name = "set_recall_status",
        description = "Mark a recall (a `finding_id` from `whats_coming` or `check_recalls`) `done` when the work was done but left no paperwork, `not_applicable` when it does not apply to this car, or `open` to undo either. A recall already linked to a service stays done until that service is deleted, so changing it is an error. Returns the recall with its status.",
        input_schema = rmcp::handler::server::common::schema_for_type::<SetRecallStatusInput>()
    )]
    async fn set_recall_status(
        &self,
        params: LenientParameters<SetRecallStatusInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("set_recall_status") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        domain_result(recall::set_status(&*self.db, p.vehicle_id, p.finding_id, &p.status).await)
    }

    #[tool(
        name = "file_research_finding",
        description = "Save research you've done about this vehicle (forum consensus, TSBs, known issues, upgrade notes) as a persistent finding. Use after answering research questions so the knowledge isn't lost when the conversation ends.",
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
}
