//! Inputs for the planned-work tools.

use glovebox_shared::{
    dates,
    entities::work_item::JobStatus,
    error::DomainResult,
    inputs::work_item::{NewWorkItem, UpdateWorkItem},
};
use schemars::JsonSchema;
use serde::Deserialize;

use super::nullable;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanWorkInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// What the work is, e.g. "Replace fuel pump (recall)".
    pub title: String,
    /// Estimated cost, integer cents (feeds the budget forecast).
    pub est_cost_cents: Option<i32>,
    /// Free-form notes. Printed on the appointment's work sheet for the shop, so keep it to what the shop may read.
    pub notes: Option<String>,
    /// The overdue/due schedule item this satisfies (from
    /// `whats_coming`) — completing the work then clears the
    /// reminder.
    pub schedule_item_id: Option<i32>,
    /// The research finding (e.g. a recall from `whats_coming` or `check_recalls`) this
    /// addresses. The recall reads as planned while the work is open and as done once the
    /// work is done. Completing the work through `record_service` also links the service to it.
    pub research_finding_id: Option<i32>,
    /// The incident this fixes (from `log_incident`) — completing the work
    /// links the incident to the service record. While the job is planned and the incident is unresolved and not a `note`, the appointment's work sheet prints the incident's title and description for the shop as the reported symptom, so check that both hold only what the shop may read.
    pub incident_id: Option<i32>,
    /// Date to look at this item again, `YYYY-MM-DD`, for work to watch
    /// rather than do now.
    pub revisit_by_date: Option<String>,
    /// Odometer reading at which to look at this item again. It is judged
    /// against the estimated mileage, which needs an odometer reading or a
    /// purchase mileage; until the car has one, an item with only this field
    /// reads `revisit_status: "ok"`.
    pub revisit_by_miles: Option<i32>,
    /// The appointment this job goes on (from `list_appointments`). It must be
    /// open. Leave it out for a job with no shop visit yet; it can be moved
    /// later with `update_planned_work`.
    pub appointment_id: Option<i32>,
    /// For a quote with no invoice: who gave it, such as the shop's name. A
    /// quote linked by `quote_service_id` takes its shop from that service.
    pub quoted_by: Option<String>,
    /// The day of the quote, `YYYY-MM-DD`.
    pub quoted_on: Option<String>,
    /// The service whose visit produced the quote (from `list_services`), when
    /// the quote came with an invoice for other work. The service then names
    /// the job in its `quoted_work_items`.
    pub quote_service_id: Option<i32>,
}

impl PlanWorkInput {
    pub fn into_domain(self) -> DomainResult<(i32, NewWorkItem)> {
        Ok((
            self.vehicle_id,
            NewWorkItem {
                title: self.title,
                notes: self.notes,
                schedule_item_id: self.schedule_item_id,
                research_finding_id: self.research_finding_id,
                incident_id: self.incident_id,
                est_cost_cents: self.est_cost_cents,
                revisit_by_date: dates::parse_optional_date(
                    "revisit_by_date",
                    self.revisit_by_date,
                )?,
                revisit_by_miles: self.revisit_by_miles,
                appointment_id: self.appointment_id,
                quoted_by: self.quoted_by,
                quoted_on: dates::parse_optional_date("quoted_on", self.quoted_on)?,
                quote_service_id: self.quote_service_id,
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListPlannedWorkInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Also include finished work: done/dropped items. Default false (open
    /// work only).
    pub include_done: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePlannedWorkInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Planned-work id, from `list_planned_work`.
    pub work_item_id: i32,
    /// New title.
    pub title: Option<String>,
    /// `planned`, `done` or `dropped`. Use `done` only for work that left no paperwork (otherwise `record_service` with `work_item_ids`), `dropped` for work the owner has decided against, and `planned` to reopen an item.
    pub status: Option<JobStatus>,
    /// Free-form notes. Printed on the appointment's work sheet for the shop, so keep it to what the shop may read.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub notes: Option<Option<String>>,
    /// Schedule item this satisfies.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub schedule_item_id: Option<Option<i32>>,
    /// Research finding (e.g. recall) this addresses.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub research_finding_id: Option<Option<i32>>,
    /// Incident this fixes. While the job is planned and the incident is unresolved and not a `note`, the appointment's work sheet prints the incident's title and description for the shop as the reported symptom, so check that both hold only what the shop may read.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub incident_id: Option<Option<i32>>,
    /// Estimated cost, integer cents.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub est_cost_cents: Option<Option<i32>>,
    /// Date to look at it again, `YYYY-MM-DD`.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub revisit_by_date: Option<Option<String>>,
    /// Odometer reading at which to look at it again.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub revisit_by_miles: Option<Option<i32>>,
    /// The appointment this job is on (from `list_appointments`). It must be
    /// open.
    ///
    /// Send `null` to take the job off its appointment; omit the key to leave
    /// it unchanged. Its needs, options and reasons stay with the job.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub appointment_id: Option<Option<i32>>,
    /// Who gave the quote this job came from.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub quoted_by: Option<Option<String>>,
    /// The day of that quote, `YYYY-MM-DD`.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub quoted_on: Option<Option<String>>,
    /// The service whose visit produced the quote (from `list_services`).
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub quote_service_id: Option<Option<i32>>,
}

impl UpdatePlannedWorkInput {
    pub fn into_domain(self) -> DomainResult<(i32, i32, UpdateWorkItem)> {
        Ok((
            self.vehicle_id,
            self.work_item_id,
            UpdateWorkItem {
                title: self.title,
                status: self.status,
                notes: self.notes,
                schedule_item_id: self.schedule_item_id,
                research_finding_id: self.research_finding_id,
                incident_id: self.incident_id,
                est_cost_cents: self.est_cost_cents,
                revisit_by_date: dates::parse_date_change("revisit_by_date", self.revisit_by_date)?,
                revisit_by_miles: self.revisit_by_miles,
                appointment_id: self.appointment_id,
                quoted_by: self.quoted_by,
                quoted_on: dates::parse_date_change("quoted_on", self.quoted_on)?,
                quote_service_id: self.quote_service_id,
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeletePlannedWorkInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Planned-work id, from `list_planned_work`.
    pub work_item_id: i32,
}
