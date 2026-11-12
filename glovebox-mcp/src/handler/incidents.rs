//! Tools for the incidents area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, LenientParameters, deleted_result, domain_result};
use glovebox_shared::services::incident;

use crate::schemas::{
    AddIncidentFollowupInput, DeleteIncidentInput, ListIncidentsInput, LogIncidentInput,
    SaveNoteInput, UpdateIncidentInput,
};

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
        let p = tool_input!(params, "log_incident");
        let (vehicle_id, input) = p.into_domain();
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
        let p = tool_input!(params, "save_note");
        let (vehicle_id, input) = p.into_domain();
        domain_result(incident::create(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "list_incidents",
        description = "The vehicle's incidents (noises, leaks, warning lights, damage, accidents, shop observations, notes), newest first, each with its followups and the services that noticed or fixed it. Pass `resolved: false` for what is still open. Use `get_incident` for one in full and this tool to find an `incident_id` for `update_incident`.",
        input_schema = rmcp::handler::server::common::schema_for_type::<ListIncidentsInput>()
    )]
    async fn list_incidents(
        &self,
        params: LenientParameters<ListIncidentsInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "list_incidents");
        domain_result(incident::list_filtered(&*self.db, p.vehicle_id, p.resolved).await)
    }

    #[tool(
        name = "update_incident",
        description = "Correct or progress an incident: title, category, description, odometer, codes, notes, accident and insurance details, repair cost, deductible, payout, recurrence link, and `noticed_during_service_id` (the service during which it was noticed). Send only the fields to change; null clears a field, and a field left out is untouched. Set `resolved` to close it by hand (e.g. fixed without an invoice) or reopen it. `resolved_by_service_ids` REPLACES the services that fixed it: a non-empty list marks it resolved and an empty list marks it unresolved (unless `resolved` is sent too). For a fix that has paperwork, prefer `record_service` with `resolved_incident_ids`. To note a phone call or email about it, use `add_incident_followup`.",
        input_schema = rmcp::handler::server::common::schema_for_type::<UpdateIncidentInput>()
    )]
    async fn update_incident(
        &self,
        params: LenientParameters<UpdateIncidentInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "update_incident");
        let (vehicle_id, incident_id, input) = p.into_domain();
        domain_result(incident::update(&*self.db, vehicle_id, incident_id, input).await)
    }

    #[tool(
        name = "add_incident_followup",
        description = "Add a dated followup to an incident: a call with the adjuster, an email to the shop, what was learned. Give `occurred_at`, a `summary`, and optionally `contact_method`, `contact_with` and `notes`. Use `update_incident` to change the incident's own fields.",
        input_schema = rmcp::handler::server::common::schema_for_type::<AddIncidentFollowupInput>()
    )]
    async fn add_incident_followup(
        &self,
        params: LenientParameters<AddIncidentFollowupInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "add_incident_followup");
        let (vehicle_id, incident_id, input) = p.into_domain();
        domain_result(incident::create_followup(&*self.db, vehicle_id, incident_id, input).await)
    }

    #[tool(
        name = "delete_incident",
        description = "Permanently delete an incident that was logged by mistake, with its followups and service links. Incidents that recurred from it are unlinked, and planned work pointing at it is detached. To close an incident that was really fixed, use `update_incident` with `resolved: true` or `record_service` with `resolved_incident_ids`, which keep the history.",
        input_schema = rmcp::handler::server::common::schema_for_type::<DeleteIncidentInput>()
    )]
    async fn delete_incident(
        &self,
        params: LenientParameters<DeleteIncidentInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "delete_incident");
        deleted_result(
            "incident",
            p.incident_id,
            incident::delete(&*self.db, p.vehicle_id, p.incident_id).await,
        )
    }
}
