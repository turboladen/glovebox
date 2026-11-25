//! Tools for the planning area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{
    GloveboxMcp, Parameters, deleted_result, domain_error, domain_result, tool_json_result,
};
use serde::Serialize;

use glovebox_shared::services::{
    upcoming,
    work_item::{self as work_item_svc, PlannedItem},
};

use crate::schemas::{
    DeletePlannedWorkInput, ListPlannedWorkInput, PlanWorkInput, UpdatePlannedWorkInput,
    VehicleParams,
};

#[tool_router(router = planning_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "plan_work",
        description = "Add something to the vehicle's to-do list — work the user intends to do or \
                       have done. Link the source so completing the work closes the loop (a \
                       recall finding from check_recalls, an overdue schedule item from \
                       whats_coming, or an incident). Before planning a repair, read the open \
                       incidents in `whats_coming`; when the job addresses one, ask the owner \
                       whether to link it and pass `incident_id`, because only a linked job \
                       resolves its incident when a service completes it. Set revisit_by_date \
                       and/or revisit_by_miles for work to watch rather than do now. Pass \
                       `appointment_id` to put the job on an appointment (from \
                       `plan_appointment`); a job with no appointment is still planned and can \
                       collect needs with `add_need` before there is a shop or a date. A quote \
                       for work not done yet is planned work: put the quoted amount in \
                       `est_cost_cents`. Pass `quote_service_id` when it came with an invoice for \
                       other work, which then gives its shop and date; pass `quoted_by` and \
                       `quoted_on` for a quote with no invoice. Review the list with \
                       list_planned_work."
    )]
    async fn plan_work(
        &self,
        params: Parameters<PlanWorkInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "plan_work");
        let (vehicle_id, input) = domain_input!(p.into_domain());
        domain_result(work_item_svc::create(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "list_planned_work",
        description = "The vehicle's planning state: the open work items (the to-do list), each \
                       with a revisit_status (ok, due_soon or overdue) when it has a \
                       revisit_by_date or revisit_by_miles. Complete an item by passing its id in \
                       record_service's work_item_ids. Set include_done to also see finished and \
                       dropped work."
    )]
    async fn list_planned_work(
        &self,
        params: Parameters<ListPlannedWorkInput>,
    ) -> Result<CallToolResult, McpError> {
        #[derive(Serialize)]
        struct PlannedWork {
            items: Vec<PlannedItem>,
        }
        let p = tool_input!(params, "list_planned_work");
        let include_done = p.include_done.unwrap_or(false);
        let items = match work_item_svc::list_with_status(
            &*self.db,
            p.vehicle_id,
            include_done,
            glovebox_shared::services::today(),
        )
        .await
        {
            Ok(v) => v,
            Err(e) => return domain_error(e),
        };
        tool_json_result(&PlannedWork { items })
    }

    #[tool(
        name = "whats_coming",
        description = "Call this first for any \"what does the car need?\" question. One answer \
                       covers scheduled maintenance (each item's status, `overdue`, `due_soon` or \
                       `ok`, when it is due, what the due point was measured from in `baseline`, \
                       and `bundle_suggestions`, one shop trip listing every overdue or due-soon \
                       item when there are two or more), the planned to-do list sorted by \
                       urgency, a 12-month budget forecast whose `planned_work_cents` totals that \
                       list's estimates, open and planned recalls only (`list_recalls` shows done \
                       ones too), unresolved incidents, the estimated mileage and the warranty \
                       status. A warranty ends at whichever bound comes first, so report \
                       `warranty.in_force`: true only while neither the date nor the mileage \
                       bound has passed. `date_passed` and `days_left`, `mileage_passed` and \
                       `miles_left` give each bound, and each is null when the vehicle has no \
                       such bound. When `in_force` is true, remind the user to check coverage \
                       before paying. When it is false, say the warranty has ended and which \
                       bound passed, and do not suggest checking coverage. `possibly_covered` is \
                       true while either bound still holds, so do not report it as coverage. \
                       `warranty` is null when the vehicle has no warranty bound on record. An \
                       item whose `status` is `overdue` and whose `baseline` is `purchase` has no \
                       service on record, so say it has no recorded history, not that the work is \
                       known to be late; `none` means the purchase record lacks the date or the \
                       mileage the item counts from, so a missing date counts from the start of \
                       the model year and a missing mileage from zero: ask when the work was last \
                       done. A reminder is `overdue` only past its due point (one exactly at it \
                       is `due_soon`), and its `trigger` names the reading that makes it due \
                       (`mileage`, `time` or `both`) and is null when it is ok. The forecast \
                       leaves out schedule items and planned work that have no estimate and names \
                       them in `unestimated_items` and `unestimated_planned_work` (with counts): \
                       when either list is not empty, say the forecast total is a floor, not a \
                       real total, and offer to add estimates with `update_schedule_item` or \
                       `update_planned_work`. `open_findings` lists the research findings that \
                       still wait (each reads `open`, or `planned` with its job on no \
                       appointment; recalls are in `recalls`), most severe first and at most ten, \
                       and `open_findings_total` counts them all: when one fits the work being \
                       discussed, say so. Offer to `plan_work` only one that reads `open`, with \
                       `research_finding_id`; one that reads `planned` already has its job in \
                       `work_item`, so put that job on the appointment with `update_planned_work` \
                       and never plan it again. Set one the owner rules out to `dismissed` with \
                       `update_research_finding`. Group overdue and due-soon items into one shop \
                       trip when advising. For items worth doing, offer to plan_work them (linked \
                       via schedule_item_id); when recorded work satisfies an item, link it via \
                       `record_service`'s `schedule_item_ids` so the reminder clears, or for \
                       service records that already exist call `link_service_to_maintenance`; for \
                       an item that does not apply to this vehicle, use `dismiss_schedule_item`."
    )]
    async fn whats_coming(
        &self,
        params: Parameters<VehicleParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "whats_coming");
        domain_result(upcoming::upcoming(&*self.db, p.vehicle_id).await)
    }

    #[tool(
        name = "update_planned_work",
        description = "Change a to-do item: title, notes, estimated cost, revisit date or \
                       mileage, the schedule item, recall or incident it is linked to, the \
                       appointment it is on (`appointment_id`: another open appointment to move \
                       it, null to take it off), or its `status`. Send only the fields to change; \
                       null clears a field, and a field left out is untouched. Set `status: \
                       \"dropped\"` when the owner no longer intends to do the work (it leaves \
                       `whats_coming` but stays in history), or `planned` to reopen an item. \
                       Taking a job a service completed off `done`, or changing its schedule \
                       item, moves the reminder with it: the old schedule item no longer counts \
                       from that service. When the work was done and there is an invoice, record \
                       it with `record_service` and `work_item_ids` instead, so the service, \
                       reminder, incident and recall are linked; set `done` here only for work \
                       that left no paperwork. Setting `done` here does not close a recall or \
                       incident the item is linked to, and taking a job a service completed off \
                       `done` does not reopen its incident (giving a job a service completed an \
                       incident does resolve it). When that service stops resolving the incident, \
                       the result names it in `released_incident_id`; ask the owner whether it is \
                       still fixed. Also call `set_recall_status` or `update_incident` for it. \
                       Use `delete_planned_work` only for an item added by mistake. Setting \
                       `planned` on an item whose appointment was closed by hand opens that \
                       appointment again, and the result names it in `reopened_appointment_id`. \
                       On a cancelled appointment the item comes off it and is unscheduled, and \
                       the result names the appointment in `left_cancelled_appointment_id`. When \
                       the schedule item stopped counting from the service, the result names both \
                       in `released_schedule_item_id` and `released_from_service_id`; when the \
                       invoice did cover it, `link_service_to_maintenance` names it on the \
                       service again. Tell the owner when any of these happens."
    )]
    async fn update_planned_work(
        &self,
        params: Parameters<UpdatePlannedWorkInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "update_planned_work");
        let (vehicle_id, work_item_id, input) = domain_input!(p.into_domain());
        domain_result(work_item_svc::update(&*self.db, vehicle_id, work_item_id, input).await)
    }

    #[tool(
        name = "delete_planned_work",
        description = "Permanently delete a to-do item that was added by mistake. For work the \
                       owner decided not to do, use `update_planned_work` with `status: \
                       \"dropped\"`, which keeps the record."
    )]
    async fn delete_planned_work(
        &self,
        params: Parameters<DeletePlannedWorkInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "delete_planned_work");
        deleted_result(
            "planned work",
            p.work_item_id,
            work_item_svc::delete(&*self.db, p.vehicle_id, p.work_item_id).await,
        )
    }
}
