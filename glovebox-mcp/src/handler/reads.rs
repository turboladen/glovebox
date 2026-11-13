//! Tools for the reads area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, Parameters, domain_error, domain_result};
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
        description = "The vehicle's recent history in one call: services, incidents, and \
                       odometer readings merged newest-first. Call at conversation start to get \
                       context before answering questions about the car."
    )]
    async fn summarize_recent_activity(
        &self,
        params: Parameters<SummarizeActivityInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "summarize_recent_activity");
        let limit = p.limit.map_or(activity::DEFAULT_LIMIT, |l| l as usize);
        domain_result(activity::recent(&*self.db, p.vehicle_id, limit).await)
    }

    #[tool(
        name = "search_records",
        description = "Full-text search across everything: vehicles, service history, incidents \
                       (and their followups), research findings, maintenance schedule items, and \
                       planned work. Returns ranked hits with snippets. Narrow with `scope` \
                       and/or `vehicle_id`."
    )]
    async fn search_records(
        &self,
        params: Parameters<SearchRecordsInput>,
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
        description = "What the vehicle has cost, as integer cents. `total_cost_cents` is \
                       `service_totals_cents` (the sum of every service's total; parts the shop \
                       supplied are already inside those invoices) plus \
                       `self_supplied_parts_cents` (parts the owner bought outside any invoice). \
                       `out_of_pocket_cents` and `covered_cents` (services paid by insurance, a \
                       third party or the manufacturer) add up to the total. \
                       `labor_subtotals_cents` and `parts_subtotals_cents` sum the subtotals \
                       entered on services and are null when none was entered. \
                       `cost_per_1000_miles_cents` divides the total by `miles_driven`, the \
                       latest service mileage minus the purchase mileage; both are null when that \
                       is unknown or not positive. `monthly_costs` plus `unbucketed_parts_cents` \
                       (self-supplied parts with neither a purchase nor an install date) add up \
                       to the total."
    )]
    async fn cost_summary(
        &self,
        params: Parameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "cost_summary");
        domain_result(costs::summary(&*self.db, p.vehicle_id).await)
    }

    #[tool(
        name = "get_service",
        description = "One service record in full: line items, parts, schedule links, the recalls \
                       it performed, the to-do items it completed, and the incidents it noticed \
                       or fixed. Each noticed incident names the service that later fixed it (if \
                       any), and each fixed incident names the service where it was first noticed."
    )]
    async fn get_service(
        &self,
        params: Parameters<GetServiceInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "get_service");
        domain_result(service_record::get(&*self.db, p.vehicle_id, p.service_id).await)
    }

    #[tool(
        name = "get_incident",
        description = "One incident in full: followups, the service during which it was noticed, \
                       and the services that fixed it."
    )]
    async fn get_incident(
        &self,
        params: Parameters<GetIncidentInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "get_incident");
        domain_result(incident::get(&*self.db, p.vehicle_id, p.incident_id).await)
    }
}
