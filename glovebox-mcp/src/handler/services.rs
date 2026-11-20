//! Tools for the services area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, Parameters, domain_result};
use glovebox_shared::{
    dates,
    services::service_record::{self, LinkMode},
};

use crate::schemas::{
    CorrectServiceInput, DeleteServiceInput, LinkServiceToMaintenanceInput, ListServicesInput,
    RecordServiceInput,
};

#[tool_router(router = services_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "record_service",
        description = "Record work that was done on a vehicle: one call per invoice or repair \
                       order. Give `service_date` (and `end_date` for multi-day jobs), \
                       `performed_by` (`shop` or `self`), `mileage` (also logs an odometer \
                       reading), and costs in integer cents: `total_cost_cents` is the invoice's \
                       grand total across all lines, including lines someone else paid for; a \
                       covered line's `cost_cents` is the value the invoice shows for that work \
                       (zero or omitted when it shows none), and what the owner paid is derived \
                       as the total minus the covered lines. `parts_cost_cents` and \
                       `labor_cost_cents` are optional subtotals to fill only when the invoice \
                       prints them. Work the manufacturer or dealer paid for, as with a recall or \
                       warranty repair, takes `paid_by: \"manufacturer\"`; the default `self` \
                       records it as owner-paid. The service's `paid_by` is who paid the invoice \
                       overall; a line's `paid_by` overrides it for that line, so a warranty or \
                       recall line on an invoice the owner paid is marked `manufacturer` (a \
                       `recall` line normally is), and a line the owner paid on a covered invoice \
                       is marked `self`. Put each job on the paperwork in `line_items` with a \
                       `work_kind` (`maintenance`, `mod`, `repair`, `inspection`, `recall` for \
                       work done under a recall campaign, or `fee` for tax, shop supplies and \
                       disposal charges, so the lines add up to the total). Each line in a read \
                       carries `effective_paid_by`, its own `paid_by` or else the service's. The \
                       result carries `lines_total_cents` (the sum of all lines) and, when it \
                       differs from `total_cost_cents`, a `note` stating both amounts; that is \
                       information, never an error, and a `fee` line records the difference. \
                       `message` is a different key: it appears only when the call hit an \
                       already-recorded invoice number, and list anything installed in `parts` so \
                       each part is created and installed by this service. A part already \
                       recorded with `record_part` (e.g. bought before the visit) goes in \
                       `part_ids` instead, by its id from `list_parts`, and is marked installed \
                       by this service; do not list it again under `parts`. A part with \
                       `supplied_by: \"self\"` was paid for elsewhere, so its `cost_cents` is not \
                       part of the invoice total. `invoice_ref` is the number printed on the \
                       paperwork; filing the same one again returns the existing record with \
                       `already_recorded: true` and a `message` saying nothing was applied, and \
                       ignores everything sent this time, including lines, parts, and incident \
                       and recall ids (nothing is merged), so fix the existing record with \
                       `correct_service` instead. If the work completes to-do items from \
                       `list_planned_work`, pass their ids in `work_item_ids`; each is marked \
                       done and its schedule item's reminder clears. If the work satisfies \
                       due/overdue schedule items from `whats_coming`, pass the ids of every item \
                       it satisfies, not only the one the owner mentioned, in `schedule_item_ids` \
                       so the reminders clear; for records that already exist, reconcile with \
                       `link_service_to_maintenance`. Pass `resolved_incident_ids` for incidents \
                       this work fixed (each is marked resolved and linked to this service) and \
                       `noticed_incident_ids` for incidents first noticed during it. Pass \
                       `recall_finding_ids` (from `check_recalls`) for recalls this work \
                       performed, e.g. when the invoice names a campaign number; completing a \
                       to-do that was planned for a recall links that recall automatically. The \
                       result lists `performed_recalls` and `completed_work_items`. When the \
                       invoice is for a planned appointment (from `list_appointments`), pass \
                       `appointment_id`; naming it installs and completes nothing by itself, so \
                       still name the finished jobs in `work_item_ids` and the parts the shop \
                       fitted in `part_ids` (a part already recorded) or in `parts` (a new one, \
                       which may name the need it fills with `need_id` and, for a kit, list its \
                       `contents`). A shop does not always fit everything that was brought. The \
                       result's `unfitted_needs` lists every need on a completed job whose part \
                       is linked but neither installed nor replaced; ask the owner about each \
                       one. Tell the owner each incident in `resolved_incidents` by its title, so \
                       a job that closed one never does so unnoticed. The result's \
                       `open_incidents` lists the incidents still unresolved on this vehicle \
                       (each with `planned_work_item_ids`, the planned jobs filed from it); ask \
                       the owner whether this work fixed any of them, and when it did, call \
                       `correct_service` with `resolved_incident_ids`."
    )]
    async fn record_service(
        &self,
        params: Parameters<RecordServiceInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "record_service");
        let (vehicle_id, input) = domain_input!(p.into_domain());
        domain_result(service_record::create(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "link_service_to_maintenance",
        description = "Link an EXISTING service record to maintenance schedule items (from \
                       `whats_coming`) so the matching reminders clear — the reconciliation step \
                       after an import, when the records were created without \
                       `schedule_item_ids`. Default mode `add` unions with the record's existing \
                       links (safe to call repeatedly while working through a list); mode \
                       `replace` overwrites them. The schedule item of a job the service \
                       completed is not one of these links: it stays until `correct_service` \
                       takes the job off. For new work, prefer passing `schedule_item_ids` to \
                       `record_service` directly."
    )]
    async fn link_service_to_maintenance(
        &self,
        params: Parameters<LinkServiceToMaintenanceInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "link_service_to_maintenance");
        let mode = p.mode.unwrap_or(LinkMode::Add);
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
        description = "The vehicle's service history, newest first, each with its line items, \
                       parts, schedule links, the to-do items it completed and the incidents and \
                       recalls it handled. Narrow with `from` and `to` dates (`YYYY-MM-DD`, \
                       inclusive). Use `get_service` for one record, `summarize_recent_activity` \
                       for a short mixed feed, and this tool to find the `service_id` for \
                       `correct_service` or `delete_service`."
    )]
    async fn list_services(
        &self,
        params: Parameters<ListServicesInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "list_services");
        let from = domain_input!(dates::parse_optional_date("from", p.from));
        let to = domain_input!(dates::parse_optional_date("to", p.to));
        domain_result(service_record::list_between(&*self.db, p.vehicle_id, from, to).await)
    }

    #[tool(
        name = "correct_service",
        description = "Fix a service record that was filed wrong: the date, cost, shop, who did \
                       it, who paid, mileage, notes, the invoice number (`invoice_ref`; another \
                       service on the car with the same number is an error naming it), the \
                       invoice lines, the parts it installed, the schedule items it satisfies, \
                       the recalls it performed, the planned jobs it completed (`work_item_ids`), \
                       or the appointment it settles (`appointment_id`). Send only the fields to \
                       change; null clears a field, and a field left out is untouched. \
                       `line_items`, `schedule_item_ids`, `part_ids`, `recall_finding_ids`, \
                       `resolved_incident_ids` and `work_item_ids` REPLACE the existing lists \
                       with exactly what is sent. A job taken off takes its incident, its recall, \
                       the parts fitted to its needs and the reminder baseline of its schedule \
                       item with it, and a job added resolves its incident and performs its \
                       recall, as `record_service` does. `resolved_incident_ids` holds the \
                       incidents the invoice itself names as fixed (`named_by_invoice` in a \
                       service read): one named there stays resolved by this service when its job \
                       is taken off, and the reply lists it in `incidents_kept_resolved`. When \
                       the call takes recorded work off the service (a job, a fitted part, a \
                       recall or an incident), it refuses and names each item until you pass \
                       `completed_work`. Ask the owner and never choose on your own: \
                       `did_not_happen` when the work was not done (jobs go back to planned, \
                       parts read `purchased` and a shop-supplied one leaves its need, recalls \
                       read open, incidents are unresolved unless the owner marked them resolved \
                       by hand), or `done_another_way` when it was done but not on this invoice \
                       (an incident then stays resolved on the owner's word, `resolved_by_hand`). \
                       The reply names any appointment that reopened (`reopened_appointment_ids`) \
                       and any job that left a cancelled one (`unassigned_job_ids`). The schedule \
                       items that stopped counting from this service are in \
                       `released_schedule_item_ids`; when the invoice did cover one, \
                       `link_service_to_maintenance` names it on the service again. Use this when \
                       the invoice was already recorded and something on it is wrong; do not \
                       record the work a second time with `record_service`, which creates a \
                       duplicate. A call that sends `work_item_ids` or `resolved_incident_ids` \
                       returns `open_incidents`, the incidents still unresolved on this vehicle; \
                       tell the owner which incidents the service resolves and ask about the open \
                       ones, as after `record_service`. To close an incident, use \
                       `record_service` for new work or `update_incident`."
    )]
    async fn correct_service(
        &self,
        params: Parameters<CorrectServiceInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "correct_service");
        let (vehicle_id, service_id, input) = domain_input!(p.into_domain());
        domain_result(service_record::update(&*self.db, vehicle_id, service_id, input).await)
    }

    #[tool(
        name = "delete_service",
        description = "Permanently delete a service record, for one that was filed by mistake or \
                       against the wrong car. When the service recorded work as done (jobs it \
                       completed, parts it installed, incidents it resolved other than ones the \
                       owner resolved by hand, recalls it performed), this tool refuses and names \
                       each item until you pass `completed_work`. Ask the owner what happened and \
                       never choose on your own: `did_not_happen` when the work was never done on \
                       this invoice, or `done_another_way` when it was done but has no invoice to \
                       file, which leaves an incident resolved on the owner's word, \
                       `resolved_by_hand` (the owner did it, or the paperwork is lost). When the \
                       owner has the invoice that did the work, choose `did_not_happen` and then \
                       record that invoice with the same jobs, parts and schedule items, since \
                       only a recorded service keeps a reminder's baseline. After \
                       `did_not_happen`, the jobs return to planned, the incidents go back to \
                       unresolved unless another service also fixed them or the owner marked them \
                       resolved by hand, the recalls read open (`recalls_reopened`, or \
                       `recalls_still_done_elsewhere` when another service performed them too), \
                       and installed parts go back to purchased with their install details \
                       cleared; a part a later service replaced stays replaced. A hand-closed \
                       appointment of a reopened job opens again (`appointments_reopened`; \
                       re-filing does not close it, so offer `update_appointment` with `status: \
                       \"done\"` afterwards), and a job on a cancelled appointment goes back to \
                       the unscheduled jobs (`jobs_unassigned`). After `done_another_way` that \
                       work stays done (`recalls_kept_done`) and only the invoice goes. Either \
                       way the service's schedule links, line items and odometer reading are \
                       removed, so its schedule items fall back to an earlier service for their \
                       reminders, and its parts are kept and listed in `kept_part_ids`: when \
                       re-filing the invoice, attach the ones that read purchased with \
                       `record_service`'s `part_ids` (never one that reads replaced); otherwise \
                       remove them with `delete_part`. Planned jobs whose quote came with the \
                       service keep their quote but no longer name it (`quotes_unlinked`). \
                       `schedule_items_released` lists the schedule items that stopped counting \
                       from the service, after either answer; their reminders count from another \
                       service or from none, so tell the owner. To change a record rather than \
                       remove it, use `correct_service`."
    )]
    async fn delete_service(
        &self,
        params: Parameters<DeleteServiceInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "delete_service");
        domain_result(
            service_record::delete(&*self.db, p.vehicle_id, p.service_id, p.completed_work)
                .await
                .map(|out| {
                    serde_json::json!({
                        "deleted": "service",
                        "id": p.service_id,
                        "kept_part_ids": out.kept_part_ids,
                        "recalls_reopened": out.reopened_recall_ids,
                        "recalls_still_done_elsewhere": out.still_done_elsewhere_recall_ids,
                        "recalls_kept_done": out.kept_done_recall_ids,
                        "appointments_reopened": out.reopened_appointment_ids,
                        "jobs_unassigned": out.unassigned_job_ids,
                        "quotes_unlinked": out.unlinked_quote_ids,
                        "schedule_items_released": out.released_schedule_item_ids,
                    })
                }),
        )
    }
}
