//! Tools for the recalls area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, LenientParameters, domain_result};
use glovebox_shared::services::research;

use crate::schemas::{FileResearchFindingInput, VehicleParams};

#[tool_router(router = recalls_router, vis = "pub(super)")]
impl GloveboxMcp {
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
}
