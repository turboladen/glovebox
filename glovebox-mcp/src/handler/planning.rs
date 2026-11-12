//! Tools for the planning area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, LenientParameters, domain_error, domain_result, tool_json_result};
use serde::Serialize;

use glovebox_shared::{
    entities::work_item,
    services::{
        budget::{self, BudgetForecast},
        reminders::{self, RemindersResponse},
        work_item as work_item_svc,
    },
};

use crate::schemas::{ListPlannedWorkInput, PlanWorkInput, VehicleParams};

#[tool_router(router = planning_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "plan_work",
        description = "Add something to the vehicle's to-do list — work the user intends to do or have done. Link the source so completing the work closes the loop (a recall finding from check_recalls, an overdue schedule item from check_due_maintenance, or an incident). Review the list with list_planned_work.",
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
        description = "The vehicle's planning state: the open work items (the to-do list). Set include_done to also see finished and dropped work.",
        input_schema = rmcp::handler::server::common::schema_for_type::<ListPlannedWorkInput>()
    )]
    async fn list_planned_work(
        &self,
        params: LenientParameters<ListPlannedWorkInput>,
    ) -> Result<CallToolResult, McpError> {
        #[derive(Serialize)]
        struct PlannedWork {
            items: Vec<work_item::Model>,
        }
        let p = match params.into_tool_input("list_planned_work") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let include_done = p.include_done.unwrap_or(false);
        let items = match work_item_svc::list(&*self.db, p.vehicle_id, include_done).await {
            Ok(v) => v,
            Err(e) => return domain_error(e),
        };
        tool_json_result(&PlannedWork { items })
    }

    #[tool(
        name = "check_due_maintenance",
        description = "Answer \"what does this car need?\" — the vehicle's maintenance schedule evaluated against its estimated current mileage: each item's status (ok/due_soon/overdue), when it's due, bundle suggestions for combining work, a 12-month budget forecast, and the warranty status (possibly_covered — remind the user to check coverage before paying). Call before recommending any maintenance. For due/overdue items, offer to plan_work them (linked via schedule_item_id) so the work lands on the to-do list; when recorded work satisfies an item, link it via `record_service`'s `schedule_item_ids` so the reminder clears — or, for service records that already exist (an import you just finished), call `link_service_to_maintenance` with the matching schedule item ids to reconcile them all at once; for an item that doesn't apply to this vehicle, use `dismiss_schedule_item`.",
        input_schema = rmcp::handler::server::common::schema_for_type::<VehicleParams>()
    )]
    async fn check_due_maintenance(
        &self,
        params: LenientParameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        // Composed payload (unit G): reminders (incl. warranty) + the
        // 12-month budget forecast in one call.
        #[derive(Serialize)]
        struct DueMaintenance {
            #[serde(flatten)]
            reminders: RemindersResponse,
            budget: BudgetForecast,
        }
        let p = match params.into_tool_input("check_due_maintenance") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let reminders = match reminders::calculate_reminders(&self.db, p.vehicle_id).await {
            Ok(v) => v,
            Err(e) => return domain_error(e),
        };
        // Reminders are computed ONCE and feed both halves of the payload.
        let budget = match budget::forecast_from(&self.db, p.vehicle_id, &reminders).await {
            Ok(v) => v,
            Err(e) => return domain_error(e),
        };
        tool_json_result(&DueMaintenance { reminders, budget })
    }
}
