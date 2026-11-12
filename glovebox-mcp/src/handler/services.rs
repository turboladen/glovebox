//! Tools for the services area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, LenientParameters, domain_error, domain_result};
use glovebox_shared::services::{
    service_record::{self, LinkMode},
    vehicle,
};

use crate::schemas::{LinkServiceToMaintenanceInput, RecordServiceInput};

#[tool_router(router = services_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "record_service",
        description = "Record maintenance or repair work that was done: what, when, at what mileage, and what it cost (integer cents). Providing `mileage` also logs an odometer reading. Use `line_items` for itemized invoices. If the work satisfies due/overdue schedule items from `check_due_maintenance`, pass their ids in `schedule_item_ids` so the reminders clear; for records you already created (e.g. a bulk import), reconcile afterwards with `link_service_to_maintenance` instead of re-recording.",
        input_schema = rmcp::handler::server::common::schema_for_type::<RecordServiceInput>()
    )]
    async fn record_service(
        &self,
        params: LenientParameters<RecordServiceInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("record_service") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, input) = p.into_domain();
        if let Err(e) = vehicle::require(&*self.db, vehicle_id).await {
            return domain_error(e);
        }
        domain_result(service_record::create(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "link_service_to_maintenance",
        description = "Link an EXISTING service record to maintenance schedule items (from `check_due_maintenance`) so the matching reminders clear — the reconciliation step after an import, when the records were created without `schedule_item_ids`. Default mode `add` unions with the record's existing links (safe to call repeatedly while working through a list); mode `replace` overwrites them. For new work, prefer passing `schedule_item_ids` to `record_service` directly.",
        input_schema = rmcp::handler::server::common::schema_for_type::<LinkServiceToMaintenanceInput>()
    )]
    async fn link_service_to_maintenance(
        &self,
        params: LenientParameters<LinkServiceToMaintenanceInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("link_service_to_maintenance") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let mode = match LinkMode::parse(p.mode.as_deref().unwrap_or("add")) {
            Ok(m) => m,
            Err(e) => return domain_error(e),
        };
        domain_result(
            service_record::link_schedule_items(
                &*self.db,
                p.vehicle_id,
                p.service_record_id,
                &p.schedule_item_ids,
                mode,
            )
            .await,
        )
    }
}
