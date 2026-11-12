//! Tools for the schedule area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, LenientParameters, domain_result};
use glovebox_shared::services::schedule;

use crate::schemas::DismissScheduleItemInput;

#[tool_router(router = schedule_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "dismiss_schedule_item",
        description = "Waive a maintenance schedule item for this vehicle — it stops appearing in the schedule and in `check_due_maintenance`. Use for items that genuinely don't apply (e.g. a dealer-only service the owner skips deliberately). For work that was actually done before tracking started, prefer `record_service` with a minimal past-dated entry linked via `schedule_item_ids` instead, so history stays honest.",
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
}
