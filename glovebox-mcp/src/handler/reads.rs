//! Tools for the reads area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, Parameters, domain_result};
use glovebox_shared::services::{
    activity, costs, incident,
    search::{self, SearchScope},
    service_record,
};

use crate::schemas::{
    CostSummaryInput, GetIncidentInput, GetServiceInput, SearchRecordsInput, SummarizeActivityInput,
};

#[tool_router(router = reads_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "summarize_recent_activity",
        description = "The vehicle's recent history in one call: services, incidents, and \
                       odometer readings merged newest-first, at most 200. Call at conversation \
                       start to get context before answering questions about the car."
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
                       (and their followups), research findings, maintenance schedule items, \
                       planned work, and parts. A part is found by its own name and numbers or by \
                       an item inside it when it is a kit, such as a filter's number in an oil \
                       change kit; the hit is the part. A part number in the query finds the part \
                       or kit that carries it however the number is spaced or punctuated, and is \
                       listed first even when the query's other words do not match it; the whole \
                       stored number must match, and a number written in a note or an invoice \
                       line matches as typed. Returns ranked hits with snippets, each with a \
                       `kind` and an `id`. A `research_finding` hit is read with \
                       `get_research_finding`; a `recall` hit is a recall, whose `id` is the \
                       `finding_id` that `list_recalls` shows and `set_recall_status` takes, and \
                       the finding tools do not find it. The `research` scope covers both kinds. \
                       Narrow with `scope` and/or `vehicle_id`."
    )]
    async fn search_records(
        &self,
        params: Parameters<SearchRecordsInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "search_records");
        let scope = p.scope.unwrap_or(SearchScope::All);
        domain_result(search::search(&*self.db, &p.query, scope, p.vehicle_id).await)
    }

    #[tool(
        name = "cost_summary",
        description = "What the vehicle has cost, as integer cents. `total_cost_cents` is \
                       `service_totals_cents` (the sum of every service's total; parts the shop \
                       supplied are already inside those invoices) plus \
                       `self_supplied_parts_cents` (parts the owner bought outside any invoice). \
                       `out_of_pocket_cents` and `covered_cents` add up to the total. A service's \
                       total is the invoice's grand total including covered lines; the owner's \
                       share is the total minus the lines someone else paid. Payment is judged \
                       per line: on a self-paid service, lines paid by someone else are covered; \
                       on a service someone else paid, only lines marked `self` are out of \
                       pocket; a line without its own `paid_by` follows its service. A service \
                       with no total adds nothing to either figure. `covered_work` is where to \
                       look for what warranty, a recall or insurance covered: one entry per \
                       service that someone else paid or that has such a line, newest first, with \
                       its id, date, description, `paid_by`, `payer_note`, \
                       `covered_by_others_cents` and `out_of_pocket_cents` for that service (both \
                       0 when it has no total; across the entries the covered figures add up to \
                       `covered_cents`), the recalls it performed and its covered `lines` \
                       (description, `work_kind`, payer and `cost_cents`, null when the line has \
                       no amount); `covered_work_count` counts the entries. \
                       `labor_subtotals_cents` and `parts_subtotals_cents` sum the subtotals \
                       entered on services and are null when none was entered. \
                       `cost_per_1000_miles_cents` divides the total by `miles_covered`, the \
                       miles the records cover: the highest odometer reading on record minus the \
                       mileage of the earliest-dated service or reading, which \
                       `history_starts_on` dates. The purchase mileage is not used. Costs \
                       recorded for only part of the car's life are spread over only those miles, \
                       so say when `history_starts_on` is recent. `miles_covered` is null when \
                       the records cover no miles. `cost_per_1000_miles_cents` is withheld (null) \
                       until the records cover at least 1,000 miles; `miles_covered` and \
                       `history_starts_on` are still reported. `monthly_costs` plus \
                       `unbucketed_parts_cents` (self-supplied parts with neither a purchase nor \
                       an install date) add up to the total. `by_work_kind` splits the counted \
                       services' lines by kind (`maintenance`, `mod`, `repair`, `inspection`, \
                       `recall`, `fee`): `line_count`, `cost_cents`, and that amount split into \
                       `out_of_pocket_cents` and `covered_cents` by each line's payer. A kind \
                       with no line is left out, and the figures are the lines' own amounts, so \
                       they add up to the service totals only when every invoice's lines do. The \
                       per-kind split is by line and is not comparable with the top-level \
                       `out_of_pocket_cents` and `covered_cents`, which leave out a service with \
                       no total and hold each service's split inside its total, so never present \
                       the per-kind figures as shares of those. `from` and `to` (`YYYY-MM-DD`, \
                       inclusive; `from` after `to` is an error) restrict every figure to one \
                       span, such as a calendar year: the services dated inside it, and the parts \
                       bought inside it (by install date when a part has no purchase date; a part \
                       with neither is left out of a ranged summary). With a range, \
                       `miles_covered` ends at the highest odometer reading dated on or before \
                       `to`. With a `from`, it starts at the latest reading (a service's mileage \
                       or a logged one) dated on or before `from`, so the miles driven before the \
                       first reading inside the range count; when no reading is that early it \
                       starts at the earliest reading inside the range. `history_starts_on` is \
                       the date of the starting reading, so it can be before `from`, and a date \
                       after `from` means the miles from `from` up to it are missing and the rate \
                       reads high: say so. `cost_per_1000_miles_cents` is the range's cost over \
                       those miles and is null when they are under 1,000. The reply repeats \
                       `from` and `to`, null for an open end."
    )]
    async fn cost_summary(
        &self,
        params: Parameters<CostSummaryInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "cost_summary");
        let (vehicle_id, range) = domain_input!(p.into_domain());
        domain_result(costs::summary_in(&*self.db, vehicle_id, range).await)
    }

    #[tool(
        name = "get_service",
        description = "One service record in full: line items, parts (a kit with its `contents`), \
                       schedule links, the recalls it performed, the to-do items it completed, \
                       the planned jobs whose quote came with it (`quoted_work_items`, work not \
                       done on this visit), and the incidents it noticed or fixed. Each noticed \
                       incident names the service that later fixed it (if any), and each fixed \
                       incident names the service where it was first noticed."
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
