//! Tools for the schedule area.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, Parameters, domain_result};
use glovebox_shared::services::schedule;

use crate::schemas::{
    AddScheduleItemInput, DismissScheduleItemInput, ListScheduleInput, RestoreScheduleItemInput,
    UpdateScheduleItemInput,
};

#[tool_router(router = schedule_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "dismiss_schedule_item",
        description = "Waive a maintenance schedule item for this vehicle: it stops appearing in \
                       the schedule and in `whats_coming`. Works on any item `list_schedule` \
                       shows, inherited or not; other cars are not affected, and services already \
                       linked to it stay linked. The result's `id` is what \
                       `restore_schedule_item` takes. Use for items that genuinely do not apply \
                       (e.g. a dealer-only service the owner skips deliberately). For work that \
                       was actually done before tracking started, prefer `record_service` with a \
                       minimal past-dated entry linked via `schedule_item_ids` instead, so \
                       history stays honest."
    )]
    async fn dismiss_schedule_item(
        &self,
        params: Parameters<DismissScheduleItemInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "dismiss_schedule_item");
        domain_result(
            schedule::dismiss_for_vehicle(&*self.db, p.vehicle_id, p.schedule_item_id, p.reason)
                .await,
        )
    }

    #[tool(
        name = "list_schedule",
        description = "The vehicle's maintenance schedule as configured: every item that applies, \
                       each with its intervals and where it came from (`inherited_from` names the \
                       platform or model template, null for items specific to this car). Items \
                       the owner dismissed are left out unless `include_dismissed` is true. Use \
                       this to find a `schedule_item_id` for `update_schedule_item`, \
                       `dismiss_schedule_item` or `record_service`; use `whats_coming` instead \
                       for what is due or overdue."
    )]
    async fn list_schedule(
        &self,
        params: Parameters<ListScheduleInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "list_schedule");
        domain_result(
            schedule::resolve_for_listing(
                &*self.db,
                p.vehicle_id,
                p.include_dismissed.unwrap_or(false),
            )
            .await,
        )
    }

    #[tool(
        name = "restore_schedule_item",
        description = "Undo `dismiss_schedule_item`: the item appears in the schedule and in \
                       `whats_coming` again, with the services linked to it still counting. Takes \
                       the id `dismiss_schedule_item` returned; `list_schedule` with \
                       `include_dismissed: true` shows dismissed items with `enabled: false` when \
                       that id is not at hand."
    )]
    async fn restore_schedule_item(
        &self,
        params: Parameters<RestoreScheduleItemInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "restore_schedule_item");
        domain_result(
            schedule::undismiss_for_vehicle(&*self.db, p.vehicle_id, p.schedule_item_id).await,
        )
    }

    #[tool(
        name = "add_schedule_item",
        description = "Add a maintenance item that applies to this car only, e.g. \"Smog check\" \
                       every 24 months, with `interval_miles` and/or `interval_months`, an \
                       optional `est_cost_cents` for the budget forecast, a `source` and `notes`. \
                       It appears in `whats_coming` and clears when a service is linked to it. To \
                       change an item the car already has, inherited or not, use \
                       `update_schedule_item` instead; adding an item under a name the car \
                       already has its own item for is an error."
    )]
    async fn add_schedule_item(
        &self,
        params: Parameters<AddScheduleItemInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "add_schedule_item");
        let (vehicle_id, input) = p.into_domain();
        domain_result(schedule::create_for_vehicle(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "update_schedule_item",
        description = "Change a maintenance item as this car sees it: its intervals, warning \
                       thresholds, estimated cost, source, notes, `enabled`, or (for an item \
                       added with `add_schedule_item`) its name. Works on any item \
                       `list_schedule` shows. An item inherited from the platform or model \
                       template is shared with other cars, so the first change gives this car its \
                       own copy (the result has a new `id` and `overrides_item_id` set to the \
                       inherited one); later calls with either id change that copy. Services and \
                       planned work already linked to the item stay linked, so its last service \
                       and status carry over. Send only the fields to change; null clears a \
                       field. Do not use this to record that work was done; that is \
                       `record_service` with `schedule_item_ids`."
    )]
    async fn update_schedule_item(
        &self,
        params: Parameters<UpdateScheduleItemInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "update_schedule_item");
        let (vehicle_id, item_id, input) = p.into_domain();
        domain_result(schedule::update_for_vehicle(&*self.db, vehicle_id, item_id, input).await)
    }
}
