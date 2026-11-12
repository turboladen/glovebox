//! Tools for the parts area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, LenientParameters, deleted_result, domain_result};
use glovebox_shared::{inputs::part::PartFilter, services::part};

use crate::schemas::{CorrectPartInput, DeletePartInput, ListPartsInput, RecordPartInput};

#[tool_router(router = parts_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "record_part",
        description = "Record a part you bought or installed for this vehicle — purchase info, cost (integer cents), who supplied it (`supplied_by`: `self` or `shop`), where it goes (location), and an optional link to the installing service. For parts listed on an invoice, pass them in `parts` on `record_service` instead; use this tool for a part bought ahead of the install.",
        input_schema = rmcp::handler::server::common::schema_for_type::<RecordPartInput>()
    )]
    async fn record_part(
        &self,
        params: LenientParameters<RecordPartInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("record_part") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, input) = p.into_domain();
        domain_result(part::create(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "list_parts",
        description = "The vehicle's parts, newest first: what was bought, what is installed (and by which service), and what has been replaced. Filter with `status` (`purchased`, `installed`, `replaced`). Use this to find the `part_id` for `correct_part` or `delete_part`.",
        input_schema = rmcp::handler::server::common::schema_for_type::<ListPartsInput>()
    )]
    async fn list_parts(
        &self,
        params: LenientParameters<ListPartsInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("list_parts") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        domain_result(part::list(&*self.db, p.vehicle_id, PartFilter { status: p.status }).await)
    }

    #[tool(
        name = "correct_part",
        description = "Fix a part record: name, maker, part number, seller, cost, status, where it goes, install or replacement details, warranty. Send only the fields to change; null clears a field, and a field left out is untouched. Use this to mark a part `replaced` or to link it to the service that installed it. To remove a part that should never have been recorded, use `delete_part`.",
        input_schema = rmcp::handler::server::common::schema_for_type::<CorrectPartInput>()
    )]
    async fn correct_part(
        &self,
        params: LenientParameters<CorrectPartInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("correct_part") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, part_id, input) = p.into_domain();
        domain_result(part::update(&*self.db, vehicle_id, part_id, input).await)
    }

    #[tool(
        name = "delete_part",
        description = "Permanently delete a part record that was entered by mistake or is a duplicate. A part that was only replaced or is no longer on the car should be marked `replaced` with `correct_part`, which keeps the history. Deleting a service keeps the parts it installed (reverted to purchased), so use this tool to remove them as well.",
        input_schema = rmcp::handler::server::common::schema_for_type::<DeletePartInput>()
    )]
    async fn delete_part(
        &self,
        params: LenientParameters<DeletePartInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("delete_part") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        deleted_result(
            "part",
            p.part_id,
            part::delete(&*self.db, p.vehicle_id, p.part_id).await,
        )
    }
}
