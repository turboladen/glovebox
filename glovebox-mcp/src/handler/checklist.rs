//! Tools for appointments, needs, options, purchases, to-dos and past
//! decisions.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use glovebox_shared::{
    error::DomainError,
    services::{appointment, checklist, need, todo},
};

use super::{GloveboxMcp, Parameters, deleted_result, domain_error, domain_result};
use crate::schemas::{
    AddNeedInput, AddNeedOptionInput, AddTodoInput, DeleteAppointmentInput, DeleteNeedInput,
    DeleteNeedOptionInput, DeleteTodoInput, GetChecklistInput, LinkPartToNeedInput,
    ListAppointmentsInput, MarkNeedOrderedInput, MarkNeedReceivedInput, PastDecisionsInput,
    PlanAppointmentInput, ReturnPartInput, UpdateAppointmentInput, UpdateNeedInput,
    UpdateNeedOptionInput, UpdateTodoInput,
};

#[tool_router(router = checklist_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "plan_appointment",
        description = "Start an appointment: one shop trip that groups jobs and to-dos, with a \
                       purpose in the owner's words (\"60k service + shore up loose ends\"). The \
                       shop and date are optional until known. Put jobs on it with `plan_work` \
                       (`appointment_id`) or `update_planned_work`, add what each job needs with \
                       `add_need`, and read the whole picture with `get_checklist`."
    )]
    async fn plan_appointment(
        &self,
        params: Parameters<PlanAppointmentInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "plan_appointment");
        let (vehicle_id, input) = p.into_domain();
        domain_result(appointment::create(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "update_appointment",
        description = "Change an appointment's purpose, shop, planned date or notes, and its \
                       `status`: `done` closes it by hand, `cancelled` cancels it, and `planning` \
                       reopens either. Closing or cancelling unassigns the jobs that are still \
                       planned, which return to the unscheduled pool with their needs and \
                       research intact, and the result lists their ids in `unassigned_job_ids`. \
                       An appointment also reads `done` by itself once it has jobs and every one \
                       is done or dropped, so set `done` only to put one away early. Send only \
                       the fields to change; null clears a field. All changes apply together or \
                       not at all."
    )]
    async fn update_appointment(
        &self,
        params: Parameters<UpdateAppointmentInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "update_appointment");
        let (vehicle_id, appointment_id, input, change) = p.into_domain();
        domain_result(
            appointment::update_with_status(&*self.db, vehicle_id, appointment_id, input, change)
                .await,
        )
    }

    #[tool(
        name = "list_appointments",
        description = "The vehicle's appointments with their status, whether they are booked, and \
                       counts of jobs, open needs and open to-dos, soonest first. Done and \
                       cancelled ones are left out unless `include_closed` is set. Use \
                       `get_checklist` to read one appointment in full."
    )]
    async fn list_appointments(
        &self,
        params: Parameters<ListAppointmentsInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "list_appointments");
        domain_result(
            appointment::list(&*self.db, p.vehicle_id, p.include_closed.unwrap_or(false)).await,
        )
    }

    #[tool(
        name = "delete_appointment",
        description = "Permanently delete an appointment made by mistake. Its jobs, to-dos and \
                       services stay and simply lose the appointment. To put one away, use \
                       `update_appointment` with a `status` instead."
    )]
    async fn delete_appointment(
        &self,
        params: Parameters<DeleteAppointmentInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "delete_appointment");
        deleted_result(
            "appointment",
            p.appointment_id,
            appointment::delete(&*self.db, p.vehicle_id, p.appointment_id).await,
        )
    }

    #[tool(
        name = "get_checklist",
        description = "The job checklist. With `appointment_id`, that appointment's: counts, \
                       `next_steps` in the order the owner works (`decide` for needs nobody has \
                       decided, `order` for decided needs still to buy, `arriving` for ordered \
                       parts not yet received, `todos`, then `done`), each job with its needs \
                       (stage, supplier, chosen option, fitment, reason, purchase and the other \
                       options with their rejection reasons), and `warnings`. With neither id, \
                       the same view for the planned jobs that have needs and are on no \
                       appointment. With `work_item_id`, one job and its needs. Call it before \
                       advising on what is left to do, and read the `warnings` aloud: an option \
                       whose fitment is only `unconfirmed`, or a part number that differs from \
                       the chosen option's. Pass at most one id."
    )]
    async fn get_checklist(
        &self,
        params: Parameters<GetChecklistInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "get_checklist");
        match (p.appointment_id, p.work_item_id) {
            (Some(_), Some(_)) => domain_error(DomainError::invalid(
                "appointment_id",
                "pass either appointment_id or work_item_id, not both",
            )),
            (Some(appointment_id), None) => domain_result(
                checklist::for_appointment(&*self.db, p.vehicle_id, appointment_id).await,
            ),
            (None, Some(work_item_id)) => {
                domain_result(checklist::for_job(&*self.db, p.vehicle_id, work_item_id).await)
            }
            (None, None) => {
                domain_result(checklist::for_unscheduled(&*self.db, p.vehicle_id).await)
            }
        }
    }

    #[tool(
        name = "past_decisions",
        description = "What the owner decided before for a kind of part, on every vehicle: the \
                       needs whose name or options match `query` (\"brake pads\", \"rotor\"), \
                       newest decision first, each with the chosen option, the reason in the \
                       owner's words, the options ruled out in `rejected` (the rejected ones with \
                       their reasons and the ones marked `does_not_fit`; one that was chosen \
                       first keeps the reason it was chosen in `chosen_reason` and the date in \
                       `chosen_on`), and any part that was ordered and returned. Call it before \
                       researching a part the owner may have chosen before, and to answer \"what \
                       did I choose for X and why?\"."
    )]
    async fn past_decisions(
        &self,
        params: Parameters<PastDecisionsInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "past_decisions");
        domain_result(checklist::past_decisions(&*self.db, &p.query).await)
    }

    #[tool(
        name = "add_need",
        description = "Add one thing a job needs (\"Rear rotors\"), as research finds it. A need \
                       starts `undecided`. Then record every option researched with \
                       `add_need_option`, and ask the owner who supplies it and which option they \
                       want; never choose the supplier or the option for them. `work_item_id` is \
                       a job from `plan_work` or `list_planned_work`."
    )]
    async fn add_need(&self, params: Parameters<AddNeedInput>) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "add_need");
        let (vehicle_id, work_item_id, input) = p.into_domain();
        domain_result(need::add(&*self.db, vehicle_id, work_item_id, input).await)
    }

    #[tool(
        name = "update_need",
        description = "Change a need: its name, quantity, supplier (`self` or `shop`), approach \
                       (`replacement` or `upgrade`), and the owner's decision. Record a decision \
                       by passing `chosen_option_id` with `decision_reason`, the owner's reason \
                       in their words; this refuses an option that does not fit or is rejected, \
                       and replaces any earlier reason. `chosen_option_id: null` clears the \
                       choice. Ask the owner first; record their answer, do not make it. A need \
                       is `to_order` once the supplier is `self` and an option is chosen, and \
                       `shop_supplies` when the supplier is `shop`. Send only the fields to \
                       change; all changes apply together or not at all. Next, \
                       `mark_need_ordered` when the owner has bought it."
    )]
    async fn update_need(
        &self,
        params: Parameters<UpdateNeedInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "update_need");
        let (vehicle_id, need_id, input, choice) = p.into_domain();
        domain_result(need::update_with_choice(&*self.db, vehicle_id, need_id, input, choice).await)
    }

    #[tool(
        name = "delete_need",
        description = "Permanently delete a need and its options, added by mistake. To record \
                       that a need is no longer wanted, drop the job with `update_planned_work` \
                       instead. A part already bought for the need stays in `list_parts`."
    )]
    async fn delete_need(
        &self,
        params: Parameters<DeleteNeedInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "delete_need");
        deleted_result(
            "need",
            p.need_id,
            need::delete(&*self.db, p.vehicle_id, p.need_id).await,
        )
    }

    #[tool(
        name = "add_need_option",
        description = "Record one researched candidate for a need. Record every option \
                       considered, including ones that do not fit, with a `fitment` verdict \
                       (`confirmed`, `unconfirmed` or `does_not_fit`) and the evidence for it in \
                       `fitment_note`. Always fill `manufacturer` and `part_number` when known: \
                       the owner reads an option by those. Choose among options with \
                       `update_need`; rule one out with `update_need_option` and a \
                       `rejected_reason`."
    )]
    async fn add_need_option(
        &self,
        params: Parameters<AddNeedOptionInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "add_need_option");
        let (vehicle_id, need_id, input) = p.into_domain();
        domain_result(need::add_option(&*self.db, vehicle_id, need_id, input).await)
    }

    #[tool(
        name = "update_need_option",
        description = "Change an option: its facts, its `fitment` and `fitment_note` (when \
                       research confirms or rules out a fit), or reject it by setting \
                       `rejected_reason`. Rejected options are kept as history, cannot be chosen, \
                       and `rejected_reason: null` un-rejects one. Marking the chosen option \
                       `does_not_fit` or rejecting it clears the choice, so tell the owner the \
                       need is undecided again."
    )]
    async fn update_need_option(
        &self,
        params: Parameters<UpdateNeedOptionInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "update_need_option");
        let (vehicle_id, option_id, input) = p.into_domain();
        domain_result(need::update_option(&*self.db, vehicle_id, option_id, input).await)
    }

    #[tool(
        name = "delete_need_option",
        description = "Permanently delete an option added by mistake; a need that had chosen it \
                       is left with no choice. To keep the history of an option that was ruled \
                       out, use `update_need_option` with a `rejected_reason` instead."
    )]
    async fn delete_need_option(
        &self,
        params: Parameters<DeleteNeedOptionInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "delete_need_option");
        deleted_result(
            "option",
            p.option_id,
            need::delete_option(&*self.db, p.vehicle_id, p.option_id).await,
        )
    }

    #[tool(
        name = "mark_need_ordered",
        description = "Record that the owner ordered a need's chosen option. It creates the part \
                       record from the option (name, manufacturer, part number, seller, price, \
                       link) with the owner as supplier, links it to the need and returns the \
                       need as `ordered`. Fields left out take the option's values, the option's \
                       unit price times the need's quantity for the cost, or today for the date; \
                       pass what the order actually was (`order_ref`, `cost_cents`, \
                       `purchase_date`, `seller`). The need needs a chosen option and a supplier \
                       other than `shop`. For a part that is already recorded, use \
                       `link_part_to_need` so it is not entered twice. A mistaken order is \
                       corrected with `correct_part`, `link_part_to_need` or `return_part`. Next, \
                       `mark_need_received` when it arrives."
    )]
    async fn mark_need_ordered(
        &self,
        params: Parameters<MarkNeedOrderedInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "mark_need_ordered");
        let (vehicle_id, need_id, details) = p.into_domain();
        domain_result(need::mark_ordered(&*self.db, vehicle_id, need_id, details).await)
    }

    #[tool(
        name = "link_part_to_need",
        description = "Attach a part that is already recorded (from `list_parts`) to a need, so \
                       it is not entered again, or detach it with `part_id: null` (the part is \
                       kept). Linking needs no chosen option; a need with no supplier takes the \
                       part's. Pass `received_on` when the part is already in hand; ask the owner \
                       once and set it here. A part reads `ordered` until someone marks it \
                       received, and a part linked to one need cannot be linked to another. \
                       `part_id` is required: send null to unlink, never leave it out."
    )]
    async fn link_part_to_need(
        &self,
        params: Parameters<LinkPartToNeedInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "link_part_to_need");
        match p.part_id {
            None => domain_error(DomainError::invalid(
                "part_id",
                "is required: send a part id to link, or null to unlink",
            )),
            Some(None) if p.received_on.is_some() => domain_error(DomainError::invalid(
                "received_on",
                "cannot be sent when unlinking (part_id: null)",
            )),
            Some(None) => {
                domain_result(need::unlink_part(&*self.db, p.vehicle_id, p.need_id).await)
            }
            Some(Some(part_id)) => domain_result(
                need::link_part(&*self.db, p.vehicle_id, p.need_id, part_id, p.received_on).await,
            ),
        }
    }

    #[tool(
        name = "mark_need_received",
        description = "Record that a need's part arrived, stamping the part with the date \
                       (`received_on`, default today). Arrival is its own step because checking \
                       the box is when a wrong part is caught. `received: false` clears the date \
                       so the need reads `ordered` again. The need must have a part linked. If \
                       the part is wrong, call `return_part`."
    )]
    async fn mark_need_received(
        &self,
        params: Parameters<MarkNeedReceivedInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "mark_need_received");
        if p.received.unwrap_or(true) {
            domain_result(
                need::mark_received(&*self.db, p.vehicle_id, p.need_id, p.received_on).await,
            )
        } else if p.received_on.is_some() {
            domain_error(DomainError::invalid(
                "received_on",
                "cannot be sent with received: false",
            ))
        } else {
            domain_result(need::clear_received(&*self.db, p.vehicle_id, p.need_id).await)
        }
    }

    #[tool(
        name = "return_part",
        description = "Record that an ordered or received part is wrong and going back. In one \
                       write it marks the part `returned` with the reason and today's date (kept \
                       in history, and left out of parts spending), unlinks it from the need, and \
                       adds a to-do \"Return <part> to <seller>\" on the job's appointment, or on \
                       the vehicle when the job has none or its appointment is closed or \
                       cancelled. The need goes back to `to_order`, or to `undecided` with the \
                       chosen option rejected for the same reason when `option_was_wrong` is \
                       true, so set it when the option itself was the mistake (the wrong size or \
                       fitment) and leave it false for a damaged or mis-shipped part. The reason \
                       the owner gave for choosing a rejected option stays on it as \
                       `chosen_reason`. To undo a return made by mistake, three calls are needed: \
                       `correct_part` with status `purchased`, `link_part_to_need` to link the \
                       part again, and `delete_todo` for the \"Return ...\" to-do. When \
                       `option_was_wrong` was true, also clear the rejection with \
                       `update_need_option` and choose the option again with `update_need`."
    )]
    async fn return_part(
        &self,
        params: Parameters<ReturnPartInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "return_part");
        let (vehicle_id, need_id, input) = p.into_domain();
        domain_result(need::return_part(&*self.db, vehicle_id, need_id, input).await)
    }

    #[tool(
        name = "add_todo",
        description = "Add a plain to-do (\"Return the HP Plus pads\", \"Ask KW to confirm the \
                       differential\"), optionally on an appointment. Tick it off with \
                       `update_todo`; it appears in `get_checklist` under `todos`."
    )]
    async fn add_todo(&self, params: Parameters<AddTodoInput>) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "add_todo");
        let (vehicle_id, input) = p.into_domain();
        domain_result(todo::add(&*self.db, vehicle_id, input).await)
    }

    #[tool(
        name = "update_todo",
        description = "Tick a to-do off with `done: true` (stamped with today's date) or un-tick \
                       it with `done: false`, and change its title, notes or appointment. Send \
                       only the fields to change; null clears notes or the appointment. Find \
                       to-do ids in `get_checklist`."
    )]
    async fn update_todo(
        &self,
        params: Parameters<UpdateTodoInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "update_todo");
        let (vehicle_id, todo_id, done, input) = p.into_domain();
        domain_result(todo::update_with_done(&*self.db, vehicle_id, todo_id, input, done).await)
    }

    #[tool(
        name = "delete_todo",
        description = "Permanently delete a to-do added by mistake. Tick off one that is done \
                       with `update_todo` instead, which keeps it in the checklist's history."
    )]
    async fn delete_todo(
        &self,
        params: Parameters<DeleteTodoInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "delete_todo");
        deleted_result(
            "to-do",
            p.todo_id,
            todo::delete(&*self.db, p.vehicle_id, p.todo_id).await,
        )
    }
}
