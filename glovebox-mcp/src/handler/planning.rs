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
                       whats_coming, or an incident). Set revisit_by_date and/or revisit_by_miles \
                       for work to watch rather than do now. Review the list with \
                       list_planned_work."
    )]
    async fn plan_work(
        &self,
        params: Parameters<PlanWorkInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "plan_work");
        let (vehicle_id, input) = p.into_domain();
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
        let items =
            match work_item_svc::list_with_status(&*self.db, p.vehicle_id, include_done).await {
                Ok(v) => v,
                Err(e) => return domain_error(e),
            };
        tool_json_result(&PlannedWork { items })
    }

    #[tool(
        name = "whats_coming",
        description = "Call this first for any \"what does the car need?\" question. One answer \
                       covers scheduled maintenance (each item's status, `overdue`, `due_soon` or \
                       `ok`, when it is due, and bundle suggestions), the planned to-do list \
                       sorted by urgency with its estimated cost, a 12-month budget forecast, \
                       open recalls, unresolved incidents, the estimated mileage and the warranty \
                       status (possibly_covered: remind the user to check coverage before \
                       paying). Group overdue and due-soon items into one shop trip when \
                       advising. For items worth doing, offer to plan_work them (linked via \
                       schedule_item_id); when recorded work satisfies an item, link it via \
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
                       mileage, the schedule item, recall or incident it is linked to, or its \
                       `status`. Send only the fields to change; null clears a field, and a field \
                       left out is untouched. Set `status: \"dropped\"` when the owner no longer \
                       intends to do the work (it leaves `whats_coming` but stays in history), or \
                       `planned` to reopen an item. When the work was done and there is an \
                       invoice, record it with `record_service` and `work_item_ids` instead, so \
                       the service, reminder, incident and recall are linked; set `done` here \
                       only for work that left no paperwork. Setting `done` here does not close a \
                       recall or incident the item is linked to: also call `set_recall_status` or \
                       `update_incident` for it. Use `delete_planned_work` only for an item added \
                       by mistake."
    )]
    async fn update_planned_work(
        &self,
        params: Parameters<UpdatePlannedWorkInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "update_planned_work");
        let (vehicle_id, work_item_id, input) = p.into_domain();
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
