//! Tools for the schedule area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, LenientParameters, domain_result};
use glovebox_shared::services::schedule;

use crate::schemas::{
    AddScheduleItemInput, DismissScheduleItemInput, UpdateScheduleItemInput, VehicleParams,
};

#[tool_router(router = schedule_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "dismiss_schedule_item",
        description = "Waive a maintenance schedule item for this vehicle — it stops appearing in the schedule and in `whats_coming`. Use for items that genuinely don't apply (e.g. a dealer-only service the owner skips deliberately). For work that was actually done before tracking started, prefer `record_service` with a minimal past-dated entry linked via `schedule_item_ids` instead, so history stays honest.",
        input_schema = rmcp::handler::server::common::schema_for_type::<DismissScheduleItemInput>()
    )]
    async fn dismiss_schedule_item(
        &self,
        params: LenientParameters<DismissScheduleItemInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("dismiss_schedule_item") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        domain_result(
            schedule::dismiss_for_vehicle(&*self.db, p.vehicle_id, p.schedule_item_id, p.reason)
                .await,
        )
    }

    #[tool(
        name = "list_schedule",
        description = "The vehicle's maintenance schedule as configured: every item that applies, each with its intervals and where it came from (`inherited_from` names the platform or model template, null for items specific to this car). Items the owner dismissed are left out. Use this to find a `schedule_item_id` for `update_schedule_item`, `dismiss_schedule_item` or `record_service`; use `whats_coming` instead for what is due or overdue.",
        input_schema = rmcp::handler::server::common::schema_for_type::<VehicleParams>()
    )]
    async fn list_schedule(
        &self,
        params: LenientParameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("list_schedule") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        domain_result(schedule::resolve(&*self.db, p.vehicle_id).await)
    }

    #[tool(
        name = "add_schedule_item",
        description = "Add a maintenance item that applies to this car only, e.g. \"Smog check\" every 24 months, with `interval_miles` and/or `interval_months`, an optional `est_cost_cents` for the budget forecast, a `source` and `notes`. It appears in `whats_coming` and clears when a service is linked to it. Items shared by every car of a platform come from the model template and cannot be added here.",
        input_schema = rmcp::handler::server::common::schema_for_type::<AddScheduleItemInput>()
    )]
    async fn add_schedule_item(
        &self,
        params: LenientParameters<AddScheduleItemInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("add_schedule_item") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, input) = p.into_domain();
        domain_result(schedule::create_for_vehicle(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "update_schedule_item",
        description = "Correct a maintenance item that belongs to this car: its name, intervals, warning thresholds, estimated cost, source, notes, or `enabled`. Send only the fields to change; null clears a field. It only works on items specific to this car; for an item inherited from the platform or model template (`inherited_from` is set in `list_schedule`) it is an error, and the answer is `dismiss_schedule_item` to waive it or `add_schedule_item` for a car-specific version. Do not use this to record that work was done; that is `record_service` with `schedule_item_ids`.",
        input_schema = rmcp::handler::server::common::schema_for_type::<UpdateScheduleItemInput>()
    )]
    async fn update_schedule_item(
        &self,
        params: LenientParameters<UpdateScheduleItemInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("update_schedule_item") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, item_id, input) = p.into_domain();
        domain_result(schedule::update_for_vehicle(&*self.db, vehicle_id, item_id, input).await)
    }
}
