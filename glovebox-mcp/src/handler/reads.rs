//! Tools for the reads area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, LenientParameters, domain_error, domain_result};
use glovebox_shared::services::{
    activity, costs, incident,
    search::{self, SearchScope},
    service_record,
};

use crate::schemas::{
    GetIncidentInput, GetServiceInput, SearchRecordsInput, SummarizeActivityInput, VehicleParams,
};

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
        let p = tool_input!(params, "summarize_recent_activity");
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
        let p = tool_input!(params, "search_records");
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
        let p = tool_input!(params, "cost_summary");
        domain_result(costs::summary(&*self.db, p.vehicle_id).await)
    }

    #[tool(
        name = "get_service",
        description = "One service record in full: line items, parts, schedule links, the recalls it performed, the to-do items it completed, and the incidents it noticed or fixed. Each noticed incident names the service that later fixed it (if any), and each fixed incident names the service where it was first noticed.",
        input_schema = rmcp::handler::server::common::schema_for_type::<GetServiceInput>()
    )]
    async fn get_service(
        &self,
        params: LenientParameters<GetServiceInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "get_service");
        domain_result(service_record::get(&*self.db, p.vehicle_id, p.service_id).await)
    }

    #[tool(
        name = "get_incident",
        description = "One incident in full: followups, the service during which it was noticed, and the services that fixed it.",
        input_schema = rmcp::handler::server::common::schema_for_type::<GetIncidentInput>()
    )]
    async fn get_incident(
        &self,
        params: LenientParameters<GetIncidentInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "get_incident");
        domain_result(incident::get(&*self.db, p.vehicle_id, p.incident_id).await)
    }
}
