//! Tools for the planning area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, LenientParameters, domain_error, domain_result, tool_json_result};
use serde::Serialize;

use glovebox_shared::services::{
    upcoming,
    work_item::{self as work_item_svc, PlannedItem},
};

use crate::schemas::{ListPlannedWorkInput, PlanWorkInput, VehicleParams};

#[tool_router(router = planning_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "plan_work",
        description = "Add something to the vehicle's to-do list — work the user intends to do or have done. Link the source so completing the work closes the loop (a recall finding from check_recalls, an overdue schedule item from whats_coming, or an incident). Set revisit_by_date and/or revisit_by_miles for work to watch rather than do now. Review the list with list_planned_work.",
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
        description = "The vehicle's planning state: the open work items (the to-do list), each with a revisit_status (ok, due_soon or overdue) when it has a revisit_by_date or revisit_by_miles. Complete an item by passing its id in record_service's work_item_ids. Set include_done to also see finished and dropped work.",
        input_schema = rmcp::handler::server::common::schema_for_type::<ListPlannedWorkInput>()
    )]
    async fn list_planned_work(
        &self,
        params: LenientParameters<ListPlannedWorkInput>,
    ) -> Result<CallToolResult, McpError> {
        #[derive(Serialize)]
        struct PlannedWork {
            items: Vec<PlannedItem>,
        }
        let p = match params.into_tool_input("list_planned_work") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let include_done = p.include_done.unwrap_or(false);
        let items =
            match work_item_svc::list_with_status(&*self.db, p.vehicle_id, include_done).await {
                Ok(v) => v,
                Err(e) => return domain_error(e),
            };
        tool_json_result(&PlannedWork { items })
    }

    #[tool(
        name = "whats_coming",
        description = "Call this first for any \"what does the car need?\" question. One answer covers scheduled maintenance (each item's status, when it is due, and bundle suggestions), the planned to-do list sorted by urgency with its estimated cost, a 12-month budget forecast, open recalls, unresolved incidents, the estimated mileage and the warranty status (possibly_covered: remind the user to check coverage before paying). Group overdue and due-soon items into one shop trip when advising. For items worth doing, offer to plan_work them (linked via schedule_item_id); when recorded work satisfies an item, link it via `record_service`'s `schedule_item_ids` so the reminder clears, or for service records that already exist call `link_service_to_maintenance`; for an item that does not apply to this vehicle, use `dismiss_schedule_item`.",
        input_schema = rmcp::handler::server::common::schema_for_type::<VehicleParams>()
    )]
    async fn whats_coming(
        &self,
        params: LenientParameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("whats_coming") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        domain_result(upcoming::upcoming(&*self.db, p.vehicle_id).await)
    }
}
