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
        description = "Record work that was done on a vehicle: one call per invoice or repair order. Give `service_date` (and `end_date` for multi-day jobs), `performed_by` (`shop` or `self`), `mileage` (also logs an odometer reading), and costs in integer cents. Put each job on the paperwork in `line_items` with a `work_kind` (`maintenance`, `mod`, `repair`, `inspection`), and list anything installed in `parts` so each part is created and installed by this service. `invoice_ref` is the number printed on the paperwork; filing the same one again returns the existing record with `already_recorded: true` and ignores everything sent this time, including lines, parts, and incident and recall ids (nothing is merged), so correct the existing record instead. If the work completes to-do items from `list_planned_work`, pass their ids in `work_item_ids`; each is marked done and its schedule item's reminder clears. If the work satisfies due/overdue schedule items from `check_due_maintenance`, pass their ids in `schedule_item_ids` so the reminders clear; for records that already exist, reconcile with `link_service_to_maintenance`. Pass `resolved_incident_ids` for incidents this work fixed (each is marked resolved and linked to this service) and `noticed_incident_ids` for incidents first noticed during it. Pass `recall_finding_ids` (from `check_recalls`) for recalls this work performed, e.g. when the invoice names a campaign number; completing a to-do that was planned for a recall links that recall automatically. The result lists `performed_recalls`.",
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
