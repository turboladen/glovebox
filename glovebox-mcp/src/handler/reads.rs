//! Tools for the reads area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, LenientParameters, domain_error, domain_result};
use glovebox_shared::services::{
    activity, costs,
    search::{self, SearchScope},
};

use crate::schemas::{SearchRecordsInput, SummarizeActivityInput, VehicleParams};

#[tool_router(router = reads_router, vis = "pub(super)")]
impl GloveboxMcp {
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
        name = "search_records",
        description = "Full-text search across everything: vehicles, service history, incidents (and their followups), research findings, maintenance schedule items, and planned work. Returns ranked hits with snippets. Narrow with `scope` and/or `vehicle_id`.",
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
}
