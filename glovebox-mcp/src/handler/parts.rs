//! Tools for the parts area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, LenientParameters, domain_error, domain_result};
use glovebox_shared::services::{part, vehicle};

use crate::schemas::RecordPartInput;

#[tool_router(router = parts_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "record_part",
        description = "Record a part you bought or installed for this vehicle — purchase info, cost (integer cents), where it goes (location), and an optional link to the installing service. Use record_service for the labor; this is the part itself.",
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
}
