//! Tools for the incidents area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, LenientParameters, domain_error, domain_result};
use glovebox_shared::services::{incident, vehicle};

use crate::schemas::{LogIncidentInput, SaveNoteInput};

#[tool_router(router = incidents_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "log_incident",
        description = "Log something that happened to a vehicle that isn't a completed repair: a noise, a leak, a warning light, an OBD code, damage, a collision. Collisions/crashes with another party go here with category `accident`. If it's the same problem coming back, set `recurrence_of_id` to the earlier incident. Use `shop_observation` with `noticed_during_service_id` for anything a shop flagged but did not fix, then `plan_work` the fix with a revisit-by. Use `record_service` for work that was actually done (pass `resolved_incident_ids` to close incidents it fixed), and `save_note` for plain facts to remember.",
        input_schema = rmcp::handler::server::common::schema_for_type::<LogIncidentInput>()
    )]
    async fn log_incident(
        &self,
        params: LenientParameters<LogIncidentInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("log_incident") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, input) = p.into_domain();
        if let Err(e) = vehicle::require(&*self.db, vehicle_id).await {
            return domain_error(e);
        }
        domain_result(incident::create(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "save_note",
        description = "Remember something about this vehicle — a fact, a preference, a memory. Saved as a note incident, searchable later via `search_records`.",
        input_schema = rmcp::handler::server::common::schema_for_type::<SaveNoteInput>()
    )]
    async fn save_note(
        &self,
        params: LenientParameters<SaveNoteInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("save_note") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, input) = p.into_domain();
        if let Err(e) = vehicle::require(&*self.db, vehicle_id).await {
            return domain_error(e);
        }
        domain_result(incident::create(&*self.db, vehicle_id, input).await)
    }
}
