//! Tools for the vehicles area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, LenientParameters, deleted_result, domain_result};
use glovebox_shared::{
    entities,
    services::{mileage, model_template, vehicle, vin_decode},
};
use serde::Serialize;

use crate::schemas::{
    AddVehicleInput, DeleteMileageInput, EmptyParams, LogMileageInput, UpdateVehicleInput,
    VehicleBrief, VehicleParams,
};

/// The created vehicle plus what the VIN lookup did, when one was asked for.
#[derive(Serialize)]
struct AddedVehicle {
    #[serde(flatten)]
    vehicle: entities::vehicle::Model,
    /// Present only when a `vin` was sent and the lookup failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    vin_decode_error: Option<String>,
}

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
        domain_result(mileage::create(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "list_mileage",
        description = "List the car's odometer readings, newest first, each with its `id`, `mileage`, `recorded_at` and source. A reading a service logged carries that service's `service_record_id`. Use it to find a mistyped reading for `delete_mileage`.",
        input_schema = rmcp::handler::server::common::schema_for_type::<VehicleParams>()
    )]
    async fn list_mileage(
        &self,
        params: LenientParameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("list_mileage") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        domain_result(mileage::list(&*self.db, p.vehicle_id).await)
    }

    #[tool(
        name = "delete_mileage",
        description = "Delete one odometer reading, e.g. a mistyped `log_mileage` entry, by its id from `list_mileage`. A reading that carries a `service_record_id` follows that service: fix it with `correct_service` (`mileage`) instead; deleting it here is an error.",
        input_schema = rmcp::handler::server::common::schema_for_type::<DeleteMileageInput>()
    )]
    async fn delete_mileage(
        &self,
        params: LenientParameters<DeleteMileageInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("delete_mileage") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        deleted_result(
            "mileage",
            p.mileage_id,
            mileage::delete(&*self.db, p.vehicle_id, p.mileage_id).await,
        )
    }

    #[tool(
        name = "delete_vehicle",
        description = "Delete a vehicle that has no history, to undo a mistaken or duplicate `add_vehicle`. It is refused when the vehicle has any service record, part, incident, planned work or research finding; its odometer readings and car-specific schedule items go with it. There is no way to delete a car with history through this tool.",
        input_schema = rmcp::handler::server::common::schema_for_type::<VehicleParams>()
    )]
    async fn delete_vehicle(
        &self,
        params: LenientParameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("delete_vehicle") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        deleted_result(
            "vehicle",
            p.vehicle_id,
            vehicle::delete_unused(&*self.db, p.vehicle_id).await,
        )
    }

    #[tool(
        name = "add_vehicle",
        description = "Add a car to the garage. Give a `name` and whatever is known. Pass `model_template_id` (from `list_model_templates`) when the car matches one, so it inherits that platform's maintenance schedule (`list_schedule` shows it). Pass `vin` to fill blank year, make, model, trim, engine, transmission and drivetrain from NHTSA; anything sent explicitly wins, and if the lookup fails (offline, invalid VIN) the car is still added and the result carries `vin_decode_error`. Use `update_vehicle` to change a car that already exists.",
        input_schema = rmcp::handler::server::common::schema_for_type::<AddVehicleInput>()
    )]
    async fn add_vehicle(
        &self,
        params: LenientParameters<AddVehicleInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("add_vehicle") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let input = p.into_domain();
        let (decoded, vin_decode_error) = match input.vin.as_deref().map(str::trim) {
            Some(vin) if !vin.is_empty() => match vin_decode::decode_vin(vin).await {
                Ok(d) => (Some(d), None),
                Err(e) => (None, Some(e)),
            },
            _ => (None, None),
        };
        domain_result(
            vehicle::create_with_vin_decode(&*self.db, input, decoded.as_ref())
                .await
                .map(|vehicle| AddedVehicle {
                    vehicle,
                    vin_decode_error,
                }),
        )
    }

    #[tool(
        name = "update_vehicle",
        description = "Change details of an existing vehicle: name, plate, color, VIN, purchase or sale details, warranty expiry, notes, model template. Send only the fields to change; a field sent as null is cleared, and a field left out is untouched. Use `add_vehicle` for a car that is not in the garage yet.",
        input_schema = rmcp::handler::server::common::schema_for_type::<UpdateVehicleInput>()
    )]
    async fn update_vehicle(
        &self,
        params: LenientParameters<UpdateVehicleInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("update_vehicle") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, input) = p.into_domain();
        domain_result(vehicle::update(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "list_model_templates",
        description = "List the known model templates (year, make, model, trim and the platform each belongs to). Call before `add_vehicle` to find the `model_template_id` that gives a new car its inherited maintenance schedule. A car that matches none can still be added without one.",
        input_schema = rmcp::handler::server::common::schema_for_type::<EmptyParams>()
    )]
    async fn list_model_templates(
        &self,
        params: LenientParameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = params.into_tool_input("list_model_templates") {
            return Ok(e);
        }
        domain_result(model_template::list_with_platform(&*self.db).await)
    }
}
