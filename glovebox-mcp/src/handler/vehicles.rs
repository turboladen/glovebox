//! Tools for the vehicles area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, Parameters, deleted_result, domain_result};
use glovebox_shared::services::{mileage, model_template, vehicle, vin_decode};

use crate::schemas::{
    AddVehicleInput, ArchiveVehicleInput, DeleteMileageInput, EmptyParams, ListVehiclesInput,
    LogMileageInput, UpdateVehicleInput, VehicleBrief, VehicleParams,
};

#[tool_router(router = vehicles_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "list_vehicles",
        description = "List the garage: each car's id, name, year, make, and model. Archived \
                       (sold) cars are left out unless `include_archived` is true; when the owner \
                       names a car this list lacks, call it again with `include_archived`. Call \
                       this first: almost every other tool takes a vehicle_id from here. Use \
                       `get_vehicle` for the full record."
    )]
    async fn list_vehicles(
        &self,
        params: Parameters<ListVehiclesInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "list_vehicles");
        domain_result(
            vehicle::list(&*self.db, p.include_archived)
                .await
                .map(|vs| vs.iter().map(VehicleBrief::from).collect::<Vec<_>>()),
        )
    }

    #[tool(
        name = "get_vehicle",
        description = "Read one vehicle's full record: VIN, engine/transmission/drivetrain, \
                       purchase info, plate, color, notes. Call after `list_vehicles` when the \
                       conversation needs specifics. The details NHTSA decoded from the VIN \
                       (plant, doors, airbags, displacement and the rest) are not in this record: \
                       `list_vehicle_attributes` reads them."
    )]
    async fn get_vehicle(
        &self,
        params: Parameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "get_vehicle");
        domain_result(vehicle::get(&*self.db, p.vehicle_id).await)
    }

    #[tool(
        name = "log_mileage",
        description = "Record an odometer reading. Keeps mileage-based maintenance reminders \
                       accurate — log whenever the user mentions their current mileage. \
                       (`record_service` with `mileage` set logs one automatically.) A reading \
                       below zero or dated after today is refused."
    )]
    async fn log_mileage(
        &self,
        params: Parameters<LogMileageInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "log_mileage");
        let (vehicle_id, input) = domain_input!(p.into_domain());
        domain_result(mileage::create(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "list_mileage",
        description = "List the car's odometer readings, newest first, each with its `id`, \
                       `mileage`, `recorded_at` and source. A reading a service logged carries \
                       that service's `service_record_id`. Use it to find a mistyped reading for \
                       `delete_mileage`."
    )]
    async fn list_mileage(
        &self,
        params: Parameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "list_mileage");
        domain_result(mileage::list(&*self.db, p.vehicle_id).await)
    }

    #[tool(
        name = "delete_mileage",
        description = "Delete one odometer reading, e.g. a mistyped `log_mileage` entry, by its \
                       id from `list_mileage`. A reading that carries a `service_record_id` \
                       follows that service: fix it with `correct_service` (`mileage`) instead; \
                       deleting it here is an error."
    )]
    async fn delete_mileage(
        &self,
        params: Parameters<DeleteMileageInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "delete_mileage");
        deleted_result(
            "mileage",
            p.mileage_id,
            mileage::delete(&*self.db, p.vehicle_id, p.mileage_id).await,
        )
    }

    #[tool(
        name = "delete_vehicle",
        description = "Delete a vehicle that has no history, to undo a mistaken or duplicate \
                       `add_vehicle`. It is refused when the vehicle has any service record, \
                       part, incident, planned work, appointment, to-do or research finding; its \
                       odometer readings and car-specific schedule items go with it. There is no \
                       way to delete a car with history through this tool."
    )]
    async fn delete_vehicle(
        &self,
        params: Parameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "delete_vehicle");
        deleted_result(
            "vehicle",
            p.vehicle_id,
            vehicle::delete_unused(&*self.db, p.vehicle_id).await,
        )
    }

    #[tool(
        name = "archive_vehicle",
        description = "Take a sold car out of the garage and record the sale: `sold_date`, \
                       `sold_price_cents` and `sold_mileage`, each optional. Its records all \
                       stay, and every tool that takes its vehicle_id still reads it; \
                       `list_vehicles` and the web page leave it out. Use this instead of \
                       `delete_vehicle` for a car with history. It refuses while the car has an \
                       open appointment, a planned job or an open to-do, naming how many, since \
                       no garage list would show them afterwards. `unarchive_vehicle` undoes it."
    )]
    async fn archive_vehicle(
        &self,
        params: Parameters<ArchiveVehicleInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "archive_vehicle");
        let (vehicle_id, sale) = domain_input!(p.into_domain());
        domain_result(vehicle::archive(&*self.db, vehicle_id, sale).await)
    }

    #[tool(
        name = "unarchive_vehicle",
        description = "Put an archived car back in the garage, for one archived by mistake or \
                       bought back. Its sale details stay; clear them with `update_vehicle`."
    )]
    async fn unarchive_vehicle(
        &self,
        params: Parameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "unarchive_vehicle");
        domain_result(vehicle::unarchive(&*self.db, p.vehicle_id).await)
    }

    #[tool(
        name = "add_vehicle",
        description = "Add a car to the garage. Give a `name` and whatever is known. Pass \
                       `model_template_id` (from `list_model_templates`) when the car matches \
                       one, so it inherits that platform's maintenance schedule (`list_schedule` \
                       shows it) and takes the template's year, make, model, trim, body style, \
                       engine, transmission and drivetrain for every one of them you leave out. \
                       Pass `vin` to fill what is still blank from NHTSA; the order is what you \
                       send, then the template, then the VIN decode, and if the lookup fails \
                       (offline, invalid VIN) the car is still added and the result carries \
                       `vin_decode_error`. Use `update_vehicle` to change a car that already \
                       exists, and `decode_vin` to decode the VIN of one."
    )]
    async fn add_vehicle(
        &self,
        params: Parameters<AddVehicleInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "add_vehicle");
        let input = domain_input!(p.into_domain());
        domain_result(vehicle::create_with_vin(&*self.db, input).await)
    }

    #[tool(
        name = "list_vehicle_attributes",
        description = "List what NHTSA decoded from the car's VIN: every stored attribute as \
                       `key`, `value` and `source`, sorted by key (about 140 for a decoded car, \
                       such as `PlantCity`, `Doors` and `DisplacementL`). Use it for a detail \
                       `get_vehicle` does not carry. An empty list means no decode is stored: \
                       `decode_vin` fetches one."
    )]
    async fn list_vehicle_attributes(
        &self,
        params: Parameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "list_vehicle_attributes");
        domain_result(vin_decode::list_attributes(&*self.db, p.vehicle_id).await)
    }

    #[tool(
        name = "decode_vin",
        description = "Decode the VIN a car already on file has, with a live NHTSA request. It \
                       replaces the car's stored attributes (`list_vehicle_attributes` reads \
                       them) and fills only the EMPTY ones among year, make, model, trim, body \
                       style, engine, transmission and drivetrain; a field that holds a value \
                       keeps it, so change one with `update_vehicle`. The result carries the \
                       `vehicle`, `filled_fields` (the names of the fields it filled) and \
                       `attributes_stored`. If the lookup fails (offline, invalid VIN) nothing \
                       changes and the result carries `vin_decode_error`. A car with no VIN is \
                       refused: set one with `update_vehicle` first. `add_vehicle` with a `vin` \
                       already decodes a new car."
    )]
    async fn decode_vin(
        &self,
        params: Parameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "decode_vin");
        domain_result(vin_decode::decode_for_vehicle(&*self.db, p.vehicle_id).await)
    }

    #[tool(
        name = "update_vehicle",
        description = "Change details of an existing vehicle: name, plate, color, VIN, purchase \
                       or sale details, warranty expiry, notes, model template. Setting a VIN \
                       decodes nothing; call `decode_vin` afterwards. Setting a model template \
                       fills the year, make, model, trim, body style, engine, transmission and \
                       drivetrain that are empty on the car and leaves the ones it has. Send only \
                       the fields to change; a field sent as null is cleared, and a field left \
                       out is untouched. Use `add_vehicle` for a car that is not in the garage \
                       yet."
    )]
    async fn update_vehicle(
        &self,
        params: Parameters<UpdateVehicleInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "update_vehicle");
        let (vehicle_id, input) = domain_input!(p.into_domain());
        domain_result(vehicle::update(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "list_model_templates",
        description = "List the known model templates (year, make, model, trim and the platform \
                       each belongs to). Call before `add_vehicle` to find the \
                       `model_template_id` that gives a new car its inherited maintenance \
                       schedule. A car that matches none can still be added without one."
    )]
    async fn list_model_templates(
        &self,
        params: Parameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        tool_input!(params, "list_model_templates");
        domain_result(model_template::list_with_platform(&*self.db).await)
    }
}
