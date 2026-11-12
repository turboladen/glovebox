//! Tools for the services area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, LenientParameters, domain_error, domain_result};
use glovebox_shared::services::{
    service_record::{self, LinkMode},
    vehicle,
};

use crate::schemas::{
    CorrectServiceInput, DeleteServiceInput, LinkServiceToMaintenanceInput, ListServicesInput,
    RecordServiceInput,
};

#[tool_router(router = services_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "record_service",
        description = "Record work that was done on a vehicle: one call per invoice or repair order. Give `service_date` (and `end_date` for multi-day jobs), `performed_by` (`shop` or `self`), `mileage` (also logs an odometer reading), and costs in integer cents. Put each job on the paperwork in `line_items` with a `work_kind` (`maintenance`, `mod`, `repair`, `inspection`), and list anything installed in `parts` so each part is created and installed by this service. A part already recorded with `record_part` (e.g. bought before the visit) goes in `part_ids` instead, by its id from `list_parts`, and is marked installed by this service; do not list it again under `parts`. `invoice_ref` is the number printed on the paperwork; filing the same one again returns the existing record with `already_recorded: true` and ignores everything sent this time, including lines, parts, and incident and recall ids (nothing is merged), so fix the existing record with `correct_service` instead. If the work completes to-do items from `list_planned_work`, pass their ids in `work_item_ids`; each is marked done and its schedule item's reminder clears. If the work satisfies due/overdue schedule items from `whats_coming`, pass their ids in `schedule_item_ids` so the reminders clear; for records that already exist, reconcile with `link_service_to_maintenance`. Pass `resolved_incident_ids` for incidents this work fixed (each is marked resolved and linked to this service) and `noticed_incident_ids` for incidents first noticed during it. Pass `recall_finding_ids` (from `check_recalls`) for recalls this work performed, e.g. when the invoice names a campaign number; completing a to-do that was planned for a recall links that recall automatically. The result lists `performed_recalls`.",
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
        description = "Link an EXISTING service record to maintenance schedule items (from `whats_coming`) so the matching reminders clear — the reconciliation step after an import, when the records were created without `schedule_item_ids`. Default mode `add` unions with the record's existing links (safe to call repeatedly while working through a list); mode `replace` overwrites them. For new work, prefer passing `schedule_item_ids` to `record_service` directly.",
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

    #[tool(
        name = "list_services",
        description = "The vehicle's service history, newest first, each with its line items, parts, schedule links and the incidents and recalls it handled. Narrow with `from` and `to` dates (`YYYY-MM-DD`, inclusive). Use `get_service` for one record, `summarize_recent_activity` for a short mixed feed, and this tool to find the `service_id` for `correct_service` or `delete_service`.",
        input_schema = rmcp::handler::server::common::schema_for_type::<ListServicesInput>()
    )]
    async fn list_services(
        &self,
        params: LenientParameters<ListServicesInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("list_services") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        if let Err(e) = vehicle::require(&*self.db, p.vehicle_id).await {
            return domain_error(e);
        }
        domain_result(
            service_record::list_between(
                &*self.db,
                p.vehicle_id,
                p.from.as_deref(),
                p.to.as_deref(),
            )
            .await,
        )
    }

    #[tool(
        name = "correct_service",
        description = "Fix a service record that was filed wrong: the date, cost, shop, who did it, who paid, mileage, notes, the invoice number (`invoice_ref`; another service on the car with the same number is an error naming it), the invoice lines, the parts it installed, the schedule items it satisfies, or the recalls it performed. Send only the fields to change; null clears a field, and a field left out is untouched. `line_items`, `schedule_item_ids`, `part_ids` and `recall_finding_ids` REPLACE the existing lists with exactly what is sent. Use this when the invoice was already recorded and something on it is wrong; do not record the work a second time with `record_service`, which creates a duplicate. To close an incident or a to-do, use `record_service` for new work, or `update_incident` / `update_planned_work` for existing records.",
        input_schema = rmcp::handler::server::common::schema_for_type::<CorrectServiceInput>()
    )]
    async fn correct_service(
        &self,
        params: LenientParameters<CorrectServiceInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("correct_service") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let (vehicle_id, service_id, input) = p.into_domain();
        domain_result(service_record::update(&*self.db, vehicle_id, service_id, input).await)
    }

    #[tool(
        name = "delete_service",
        description = "Permanently delete a service record, for one that was filed by mistake or against the wrong car. If the service performed recalls, this tool refuses and lists them until you pass `recalls`: ask the owner whether those recalls should be reopened (`\"reopen\"`) or kept as done (`\"keep_done\"`), and never choose on your own. The result lists `recalls_reopened`, `recalls_kept_done`, and `recalls_still_done_elsewhere` for reopened recalls that stay done because another service performed them too. Deleting also reopens what else it closed: an incident it fixed goes back to unresolved unless another service also fixed it, and to-dos it completed return to planned. Its schedule links, line items and odometer reading are removed. Parts it installed are kept, reverted to purchased with their install details cleared, and the result lists their ids in `kept_part_ids`: when re-filing the invoice, attach them with `record_service`'s `part_ids`; otherwise remove them with `delete_part`. To change a record rather than remove it, use `correct_service`.",
        input_schema = rmcp::handler::server::common::schema_for_type::<DeleteServiceInput>()
    )]
    async fn delete_service(
        &self,
        params: LenientParameters<DeleteServiceInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = match params.into_tool_input("delete_service") {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        domain_result(
            service_record::delete(
                &*self.db,
                p.vehicle_id,
                p.service_id,
                p.recalls.map(Into::into),
            )
            .await
            .map(|out| {
                serde_json::json!({
                    "deleted": "service",
                    "id": p.service_id,
                    "kept_part_ids": out.kept_part_ids,
                    "recalls_reopened": out.reopened_recall_ids,
                    "recalls_still_done_elsewhere": out.still_done_elsewhere_recall_ids,
                    "recalls_kept_done": out.kept_done_recall_ids,
                })
            }),
        )
    }
}
