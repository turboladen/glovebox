//! Inputs for the job-checklist tools: appointments, needs, options,
//! purchases, to-dos and past decisions.
//!
//! A field typed `Option<Option<T>>` follows the convention in `corrections`:
//! an absent key leaves the value alone, `null` clears it, and a value sets it.

use glovebox_shared::inputs::{
    appointment::{NewAppointment, StatusChange, UpdateAppointment},
    need::{NewNeed, NewOption, OrderDetails, ReturnInput, UpdateNeed, UpdateOption},
    todo::{NewTodo, UpdateTodo},
};
use schemars::JsonSchema;
use serde::Deserialize;

use super::corrections::nullable;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanAppointmentInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// What the visit is for, in the owner's words, e.g. "60k service + shore
    /// up loose ends". Required.
    pub purpose: String,
    /// The shop, named the way `record_service` names its shop. Leave it out
    /// until the owner knows.
    pub shop_name: Option<String>,
    /// The planned date, `YYYY-MM-DD`. Leave it out until booked.
    pub planned_date: Option<String>,
    /// Free-form notes.
    pub notes: Option<String>,
}

impl PlanAppointmentInput {
    pub fn into_domain(self) -> (i32, NewAppointment) {
        (
            self.vehicle_id,
            NewAppointment {
                purpose: self.purpose,
                shop_name: self.shop_name,
                shop_id: None,
                planned_date: self.planned_date,
                notes: self.notes,
            },
        )
    }
}

/// The status an appointment can be set to.
#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AppointmentStatus {
    Planning,
    Done,
    Cancelled,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateAppointmentInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Appointment id, from `plan_appointment` or `list_appointments`.
    pub appointment_id: i32,
    /// `done` closes the appointment by hand, `cancelled` cancels it, and
    /// `planning` reopens either. Closing or cancelling takes every job that
    /// is still planned off the appointment, with its needs and research
    /// intact, and the result lists their ids in `unassigned_job_ids`. Omit to
    /// leave the status alone.
    pub status: Option<AppointmentStatus>,
    /// New purpose.
    pub purpose: Option<String>,
    /// The shop.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub shop_name: Option<Option<String>>,
    /// The planned date, `YYYY-MM-DD`.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub planned_date: Option<Option<String>>,
    /// Free-form notes.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub notes: Option<Option<String>>,
}

impl UpdateAppointmentInput {
    pub fn into_domain(self) -> (i32, i32, UpdateAppointment, Option<StatusChange>) {
        let change = self.status.map(|s| match s {
            AppointmentStatus::Planning => StatusChange::Reopen,
            AppointmentStatus::Done => StatusChange::Close,
            AppointmentStatus::Cancelled => StatusChange::Cancel,
        });
        (
            self.vehicle_id,
            self.appointment_id,
            UpdateAppointment {
                purpose: self.purpose,
                shop_name: self.shop_name,
                shop_id: None,
                planned_date: self.planned_date,
                notes: self.notes,
            },
            change,
        )
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListAppointmentsInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Also list appointments that are done or cancelled. Default false.
    pub include_closed: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteAppointmentInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Appointment id, from `list_appointments`.
    pub appointment_id: i32,
}

// The field names are the ids the tool accepts.
#[allow(clippy::struct_field_names)]
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetChecklistInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Read this appointment's checklist. Pass at most one of
    /// `appointment_id` and `work_item_id`.
    pub appointment_id: Option<i32>,
    /// Read this one job, with its needs. Pass at most one of
    /// `appointment_id` and `work_item_id`.
    pub work_item_id: Option<i32>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddNeedInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// The job (planned work id, from `plan_work` or `list_planned_work`) that
    /// needs this.
    pub work_item_id: i32,
    /// What is needed, e.g. "Rear rotors".
    pub name: String,
    /// How many, default 1.
    pub quantity: Option<i32>,
    /// Who supplies it: `self` or `shop`. Leave it out until the owner has
    /// said; do not choose for them.
    pub supplied_by: Option<String>,
    /// `replacement` (the same part as the car has) or `upgrade`. Leave it out
    /// until the owner has said.
    pub approach: Option<String>,
}

impl AddNeedInput {
    pub fn into_domain(self) -> (i32, i32, NewNeed) {
        (
            self.vehicle_id,
            self.work_item_id,
            NewNeed {
                name: self.name,
                quantity: self.quantity,
                supplied_by: self.supplied_by,
                approach: self.approach,
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateNeedInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Need id, from `get_checklist` or `add_need`.
    pub need_id: i32,
    /// New name.
    pub name: Option<String>,
    /// New quantity, at least 1.
    pub quantity: Option<i32>,
    /// `self` or `shop`. Set it only from the owner's answer.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub supplied_by: Option<Option<String>>,
    /// `replacement` or `upgrade`.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub approach: Option<Option<String>>,
    /// The option the owner chose (an id from `add_need_option` or
    /// `get_checklist`). Choosing stamps the decision date and REPLACES the
    /// stored reason with `decision_reason`, clearing it when that is left
    /// out. An option whose fitment is `does_not_fit`, or that is rejected, is
    /// refused. Choose only what the owner chose.
    ///
    /// Send `null` to clear the choice, its reason and its date (do not send a
    /// `decision_reason` with it); omit the key to leave the choice unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub chosen_option_id: Option<Option<i32>>,
    /// Why the owner decided as they did, in their words. Sent with
    /// `chosen_option_id` it is stored with the choice; sent alone it edits
    /// the reason of the choice already made. It is refused together with
    /// `chosen_option_id: null`, because a reason needs a chosen option.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged (unless
    /// `chosen_option_id` is sent).
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub decision_reason: Option<Option<String>>,
}

impl UpdateNeedInput {
    pub fn into_domain(self) -> (i32, i32, UpdateNeed, Option<Option<i32>>) {
        (
            self.vehicle_id,
            self.need_id,
            UpdateNeed {
                name: self.name,
                quantity: self.quantity,
                supplied_by: self.supplied_by,
                approach: self.approach,
                decision_reason: self.decision_reason,
            },
            self.chosen_option_id,
        )
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteNeedInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Need id, from `get_checklist`.
    pub need_id: i32,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddNeedOptionInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Need id, from `get_checklist` or `add_need`.
    pub need_id: i32,
    /// What the option is, e.g. "310 mm slotted rotor".
    pub description: String,
    /// Brand. Always fill it when known: the owner reads an option by
    /// manufacturer and part number.
    pub manufacturer: Option<String>,
    /// Manufacturer part number. Always fill it when known.
    pub part_number: Option<String>,
    /// Where it can be bought, e.g. "FCP Euro".
    pub seller: Option<String>,
    /// Price, integer cents.
    pub price_cents: Option<i32>,
    /// Listing URL.
    pub url: Option<String>,
    /// Whether it fits this car: `confirmed`, `unconfirmed` (the default) or
    /// `does_not_fit`. Record every option researched, including ones that do
    /// not fit, so the same mistake is not made twice.
    pub fitment: Option<String>,
    /// The evidence for the fitment verdict, e.g. "340 x 30 mm, Performance
    /// Pack, checked against sales code AU16X2".
    pub fitment_note: Option<String>,
}

impl AddNeedOptionInput {
    pub fn into_domain(self) -> (i32, i32, NewOption) {
        (
            self.vehicle_id,
            self.need_id,
            NewOption {
                description: self.description,
                manufacturer: self.manufacturer,
                part_number: self.part_number,
                seller: self.seller,
                price_cents: self.price_cents,
                url: self.url,
                fitment: self.fitment,
                fitment_note: self.fitment_note,
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateNeedOptionInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Option id, from `add_need_option` or `get_checklist`.
    pub option_id: i32,
    /// New description.
    pub description: Option<String>,
    /// `confirmed`, `unconfirmed` or `does_not_fit`. Marking the chosen option
    /// `does_not_fit` clears the choice.
    pub fitment: Option<String>,
    /// Brand.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub manufacturer: Option<Option<String>>,
    /// Manufacturer part number.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub part_number: Option<Option<String>>,
    /// Where it can be bought.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub seller: Option<Option<String>>,
    /// Price, integer cents.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub price_cents: Option<Option<i32>>,
    /// Listing URL.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub url: Option<Option<String>>,
    /// The evidence for the fitment verdict.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub fitment_note: Option<Option<String>>,
    /// Why the option was ruled out. Setting a reason rejects the option, and a
    /// rejected option cannot be chosen; the rejection is kept as history.
    ///
    /// Send `null` to un-reject it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub rejected_reason: Option<Option<String>>,
}

impl UpdateNeedOptionInput {
    pub fn into_domain(self) -> (i32, i32, UpdateOption) {
        (
            self.vehicle_id,
            self.option_id,
            UpdateOption {
                description: self.description,
                manufacturer: self.manufacturer,
                part_number: self.part_number,
                seller: self.seller,
                price_cents: self.price_cents,
                url: self.url,
                fitment: self.fitment,
                fitment_note: self.fitment_note,
                rejected_reason: self.rejected_reason,
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteNeedOptionInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Option id, from `get_checklist`.
    pub option_id: i32,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkNeedOrderedInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Need id, from `get_checklist`. It must have a chosen option and must
    /// not be supplied by the shop.
    pub need_id: i32,
    /// Seller, when it differs from the chosen option's.
    pub seller: Option<String>,
    /// The order's date, `YYYY-MM-DD`. Defaults to today.
    pub purchase_date: Option<String>,
    /// What was paid, integer cents, when it differs from the option's price.
    pub cost_cents: Option<i32>,
    /// The seller's order or confirmation number.
    pub order_ref: Option<String>,
}

impl MarkNeedOrderedInput {
    pub fn into_domain(self) -> (i32, i32, OrderDetails) {
        (
            self.vehicle_id,
            self.need_id,
            OrderDetails {
                seller: self.seller,
                purchase_date: self.purchase_date,
                cost_cents: self.cost_cents,
                order_ref: self.order_ref,
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LinkPartToNeedInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Need id, from `get_checklist`.
    pub need_id: i32,
    /// Required. The id of a part already recorded (from `list_parts` or
    /// `record_part`) to link to the need, or `null` to unlink the need's
    /// current part (the part itself is kept).
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub part_id: Option<Option<i32>>,
    /// The day the part arrived, `YYYY-MM-DD`, when linking a part that is
    /// already in hand. Ask the owner once and set it here. Not allowed when
    /// `part_id` is `null`.
    pub received_on: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkNeedReceivedInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Need id, from `get_checklist`. It must have a part linked.
    pub need_id: i32,
    /// `true` (the default) records that the part arrived; `false` clears the
    /// arrival date so the need reads `ordered` again.
    pub received: Option<bool>,
    /// The day it arrived, `YYYY-MM-DD`. Defaults to today. Not allowed with
    /// `received: false`.
    pub received_on: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReturnPartInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Need id, from `get_checklist`. Its stage must be `ordered` or
    /// `received`.
    pub need_id: i32,
    /// Why the part is going back, e.g. "box held 272 mm rotors".
    pub reason: String,
    /// `true` when the chosen option itself was the mistake: the option is
    /// rejected for the same reason and the need goes back to `undecided`.
    /// Default `false`: the need goes back to `to_order`.
    pub option_was_wrong: Option<bool>,
}

impl ReturnPartInput {
    pub fn into_domain(self) -> (i32, i32, ReturnInput) {
        (
            self.vehicle_id,
            self.need_id,
            ReturnInput {
                reason: self.reason,
                option_was_wrong: self.option_was_wrong.unwrap_or(false),
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddTodoInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// What to do, e.g. "Ask KW to confirm the VAQ differential".
    pub title: String,
    /// Free-form notes.
    pub notes: Option<String>,
    /// The appointment it belongs to (from `list_appointments`). Leave it out
    /// for a to-do on the vehicle alone.
    pub appointment_id: Option<i32>,
}

impl AddTodoInput {
    pub fn into_domain(self) -> (i32, NewTodo) {
        (
            self.vehicle_id,
            NewTodo {
                title: self.title,
                notes: self.notes,
                appointment_id: self.appointment_id,
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateTodoInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// To-do id, from `get_checklist` or `add_todo`.
    pub todo_id: i32,
    /// `true` ticks it off with today's date; `false` un-ticks it. Omit to
    /// leave it alone.
    pub done: Option<bool>,
    /// New title.
    pub title: Option<String>,
    /// Free-form notes.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub notes: Option<Option<String>>,
    /// The appointment it belongs to.
    ///
    /// Send `null` to take it off its appointment; omit the key to leave it
    /// unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub appointment_id: Option<Option<i32>>,
}

impl UpdateTodoInput {
    pub fn into_domain(self) -> (i32, i32, Option<bool>, UpdateTodo) {
        (
            self.vehicle_id,
            self.todo_id,
            self.done,
            UpdateTodo {
                title: self.title,
                notes: self.notes,
                appointment_id: self.appointment_id,
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteTodoInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// To-do id, from `get_checklist`.
    pub todo_id: i32,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PastDecisionsInput {
    /// What kind of part to look up, e.g. "brake pads" or "rotor". Matches a
    /// need's name and its options' description, manufacturer and part number
    /// on every vehicle.
    pub query: String,
}
