//! Tools for the vehicles area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, LenientParameters, domain_error, domain_result};
use glovebox_shared::services::{mileage, vehicle};

use crate::schemas::{EmptyParams, LogMileageInput, VehicleBrief, VehicleParams};

#[tool_router(router = vehicles_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "list_vehicles",
        description = "List the garage — every vehicle's id, name, year, make, and model. Call this first: almost every other tool takes a vehicle_id from here. Use `get_vehicle` for the full record.",
        input_schema = rmcp::handler::server::common::schema_for_type::<EmptyParams>()
    )]
    async fn list_vehicles(
        &self,
        params: LenientParameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = params.into_tool_input("list_vehicles") {
            return Ok(e);
        }
        domain_result(
            vehicle::list(&*self.db)
                .await
                .map(|vs| vs.iter().map(VehicleBrief::from).collect::<Vec<_>>()),
        )
    }

    #[tool(
        name = "get_vehicle",
        description = "Read one vehicle's full record: VIN, engine/transmission/drivetrain, purchase info, plate, color, notes. Call after `list_vehicles` when the conversation needs specifics.",
        input_schema = rmcp::handler::server::common::schema_for_type::<VehicleParams>()
    )]
    async fn get_vehicle(
        &self,
        params: LenientParameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("get_vehicle") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        domain_result(vehicle::get(&*self.db, p.vehicle_id).await)
    }

    #[tool(
        name = "log_mileage",
        description = "Record an odometer reading. Keeps mileage-based maintenance reminders accurate — log whenever the user mentions their current mileage. (`record_service` with `mileage` set logs one automatically.)",
        input_schema = rmcp::handler::server::common::schema_for_type::<LogMileageInput>()
    )]
    async fn log_mileage(
        &self,
        params: LenientParameters<LogMileageInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("log_mileage") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, input) = p.into_domain();
        if let Err(e) = vehicle::require(&*self.db, vehicle_id).await {
            return domain_error(e);
        }
        domain_result(mileage::create(&*self.db, vehicle_id, input).await)
    }
}
