//! Tools for wear readings of brake pads, brake rotors and tire tread.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, Parameters, deleted_result, domain_result};
use glovebox_shared::services::wear;

use crate::schemas::{
    CorrectWearReadingInput, DeleteWearReadingInput, ListWearReadingsInput, RecordWearReadingInput,
};

#[tool_router(router = wear_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "record_wear_reading",
        description = "Record one wear reading: a brake pad or rotor thickness, or a tire tread \
                       depth, for one axle, in the number and unit the shop wrote (`mm`, `in_32` \
                       for 32nds of an inch, or `in`). Readings on a new invoice go in \
                       `record_service`'s `wear_readings`; use this tool for one taken another \
                       way, or to add the readings of a service already recorded (give its \
                       `service_id`, and the reading takes its date and mileage). Give the \
                       minimum the shop stated as `limit`. A percentage (\"front 50%\") is not a \
                       length: leave it in the service's notes. When the pads, rotors or tires \
                       were replaced, tag the new part with `wears_as` so the projection starts \
                       over from its install."
    )]
    async fn record_wear_reading(
        &self,
        params: Parameters<RecordWearReadingInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "record_wear_reading");
        let (vehicle_id, input) = domain_input!(p.into_domain());
        domain_result(wear::create(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "list_wear_readings",
        description = "The vehicle's wear readings, newest first, each with its `reading` and \
                       `limit` as written (`value`, `unit` and the `text` to quote). Filter by \
                       `component`, `axle` or `service_id`. Use it to find the `wear_reading_id` \
                       for `correct_wear_reading` or `delete_wear_reading`. For when each \
                       component reaches its limit, read `wear` in `whats_coming`."
    )]
    async fn list_wear_readings(
        &self,
        params: Parameters<ListWearReadingsInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "list_wear_readings");
        let (vehicle_id, filter) = p.into_domain();
        domain_result(wear::list(&*self.db, vehicle_id, filter).await)
    }

    #[tool(
        name = "correct_wear_reading",
        description = "Fix a wear reading: its component, axle, reading, limit, day, odometer, \
                       service or notes. Absent fields are left alone and a field sent as null is \
                       cleared."
    )]
    async fn correct_wear_reading(
        &self,
        params: Parameters<CorrectWearReadingInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "correct_wear_reading");
        let (vehicle_id, id, input) = domain_input!(p.into_domain());
        domain_result(wear::update(&*self.db, vehicle_id, id, input).await)
    }

    #[tool(
        name = "delete_wear_reading",
        description = "Delete a wear reading recorded by mistake."
    )]
    async fn delete_wear_reading(
        &self,
        params: Parameters<DeleteWearReadingInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "delete_wear_reading");
        deleted_result(
            "wear_reading",
            p.wear_reading_id,
            wear::delete(&*self.db, p.vehicle_id, p.wear_reading_id).await,
        )
    }
}
