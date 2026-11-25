//! Inputs for the incident and note tools.

use glovebox_shared::{
    dates,
    entities::incident::IncidentCategory,
    error::DomainResult,
    inputs::incident::{NewFollowup, NewIncident, UpdateFollowup, UpdateIncident},
};
use schemars::JsonSchema;
use serde::Deserialize;

use super::nullable;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LogIncidentInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Short title, e.g. "Squeak from front left on braking". When a planned job is filed from the incident, this prints on the appointment's work sheet for the shop, so keep it to what the shop may read.
    pub title: String,
    /// Defaults to `general`. Collisions/crashes with another party belong
    /// under category `accident`; anything a shop flagged but did not fix
    /// belongs under `shop_observation`.
    pub category: Option<IncidentCategory>,
    /// Longer description of what happened or was noticed. When a planned job is filed from the incident, this prints on the appointment's work sheet for the shop, so keep it to what the shop may read.
    pub description: Option<String>,
    /// Odometer reading when it happened.
    pub odometer: Option<i32>,
    /// When it happened, `YYYY-MM-DD HH:MM:SS` or just `YYYY-MM-DD`. Defaults to the current local date and time.
    pub occurred_at: Option<String>,
    /// OBD-II codes if a scanner was involved, e.g. "P0301,P0420".
    pub obd_codes: Option<String>,
    /// Free-form notes.
    pub notes: Option<String>,
    /// If this is the same problem coming back, the id of the earlier
    /// incident it recurs from (same vehicle).
    pub recurrence_of_id: Option<i32>,
    /// The service record (same vehicle) during which this was noticed, e.g.
    /// the visit where the shop flagged it.
    pub noticed_during_service_id: Option<i32>,
}

impl LogIncidentInput {
    pub fn into_domain(self) -> DomainResult<(i32, NewIncident)> {
        Ok((
            self.vehicle_id,
            NewIncident {
                category: self.category.unwrap_or(IncidentCategory::General),
                title: self.title,
                description: self.description,
                odometer: self.odometer,
                occurred_at: dates::parse_optional_day_or_time("occurred_at", self.occurred_at)?,
                obd_codes: self.obd_codes,
                notes: self.notes,
                recurrence_of_id: self.recurrence_of_id,
                noticed_during_service_id: self.noticed_during_service_id,
                ..Default::default()
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveNoteInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// The thing to remember, in plain words.
    pub note: String,
}

impl SaveNoteInput {
    /// Thin alias over `incident::create`: category `note`, title = first 80
    /// chars of the note, description = the full note.
    pub fn into_domain(self) -> (i32, NewIncident) {
        let title: String = self.note.chars().take(80).collect();
        (
            self.vehicle_id,
            NewIncident {
                category: IncidentCategory::Note,
                title,
                description: Some(self.note),
                ..Default::default()
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListIncidentsInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// `true` for resolved incidents only, `false` for unresolved only. Omit for all.
    pub resolved: Option<bool>,
    /// Only incidents of this category.
    pub category: Option<IncidentCategory>,
    /// The most incidents to return, counted from the newest after the other filters. At least 1; 0 is an error. Omit for all.
    pub limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateIncidentInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Incident id, from `list_incidents` or `log_incident`.
    pub incident_id: i32,
    /// New category.
    pub category: Option<IncidentCategory>,
    /// New title. When a planned job is filed from the incident, this prints on the appointment's work sheet for the shop, so keep it to what the shop may read.
    pub title: Option<String>,
    /// When it happened, `YYYY-MM-DD HH:MM:SS` or just `YYYY-MM-DD`.
    pub occurred_at: Option<String>,
    /// Mark resolved or unresolved by hand, e.g. fixed with no invoice. Resolving by hand is remembered (`resolved_by_hand`), and undoing a service's work later leaves such an incident resolved. Omit to leave the flag to the service links.
    pub resolved: Option<bool>,
    /// REPLACES the services that fixed it with exactly this list. A non-empty list marks the incident resolved; an empty list removes every fixing service and marks it unresolved, unless the owner resolved it by hand. A service that completed a job filed from the incident stays whatever the list says, and naming it here changes nothing, so the list alone neither resolves nor reopens the incident while only such services are on it; send `resolved`, or take the job off with `correct_service`. A `resolved` sent in the same call wins. Omit to leave the links alone.
    pub resolved_by_service_ids: Option<Vec<i32>>,
    /// Longer description. When a planned job is filed from the incident, this prints on the appointment's work sheet for the shop, so keep it to what the shop may read.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub description: Option<Option<String>>,
    /// Odometer reading when it happened.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub odometer: Option<Option<i32>>,
    /// OBD-II codes.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub obd_codes: Option<Option<String>>,
    /// Free-form notes.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub notes: Option<Option<String>>,
    /// Who was at fault (accidents).
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub fault: Option<Option<String>>,
    /// Other party's name (accidents).
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub other_party_name: Option<Option<String>>,
    /// Other party's phone.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub other_party_phone: Option<Option<String>>,
    /// Other party's email.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub other_party_email: Option<Option<String>>,
    /// Other party's insurer.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub other_party_insurance: Option<Option<String>>,
    /// Other party's policy number.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub other_party_policy_number: Option<Option<String>>,
    /// Your insurance claim number.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub insurance_claim_number: Option<Option<String>>,
    /// Adjuster's name.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub insurance_adjuster: Option<Option<String>>,
    /// Adjuster's phone.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub insurance_adjuster_phone: Option<Option<String>>,
    /// Total repair cost, integer cents.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub total_repair_cost_cents: Option<Option<i32>>,
    /// Deductible, integer cents.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub deductible_cents: Option<Option<i32>>,
    /// Insurance payout, integer cents.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub insurance_payout_cents: Option<Option<i32>>,
    /// Earlier incident (same vehicle) this one is a recurrence of.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub recurrence_of_id: Option<Option<i32>>,
    /// The service record (same vehicle) during which this was noticed.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub noticed_during_service_id: Option<Option<i32>>,
}

impl UpdateIncidentInput {
    pub fn into_domain(self) -> DomainResult<(i32, i32, UpdateIncident)> {
        Ok((
            self.vehicle_id,
            self.incident_id,
            UpdateIncident {
                category: self.category,
                title: self.title,
                occurred_at: dates::parse_optional_day_or_time("occurred_at", self.occurred_at)?,
                resolved: self.resolved,
                resolved_by_service_ids: self.resolved_by_service_ids,
                description: self.description,
                odometer: self.odometer,
                obd_codes: self.obd_codes,
                notes: self.notes,
                fault: self.fault,
                other_party_name: self.other_party_name,
                other_party_phone: self.other_party_phone,
                other_party_email: self.other_party_email,
                other_party_insurance: self.other_party_insurance,
                other_party_policy_number: self.other_party_policy_number,
                insurance_claim_number: self.insurance_claim_number,
                insurance_adjuster: self.insurance_adjuster,
                insurance_adjuster_phone: self.insurance_adjuster_phone,
                total_repair_cost_cents: self.total_repair_cost_cents,
                deductible_cents: self.deductible_cents,
                insurance_payout_cents: self.insurance_payout_cents,
                recurrence_of_id: self.recurrence_of_id,
                noticed_during_service_id: self.noticed_during_service_id,
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddIncidentFollowupInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Incident id, from `list_incidents` or `log_incident`.
    pub incident_id: i32,
    /// When the contact happened, `YYYY-MM-DD HH:MM:SS` or just `YYYY-MM-DD`.
    pub occurred_at: String,
    /// What happened or was said, e.g. 'Adjuster approved the estimate'. Blank text is an error.
    pub summary: String,
    /// How, e.g. `phone`, `email`. Blank text is stored as none.
    pub contact_method: Option<String>,
    /// Who with. Blank text is stored as none.
    pub contact_with: Option<String>,
    /// Free-form notes. Blank text is stored as none.
    pub notes: Option<String>,
}

impl AddIncidentFollowupInput {
    pub fn into_domain(self) -> DomainResult<(i32, i32, NewFollowup)> {
        Ok((
            self.vehicle_id,
            self.incident_id,
            NewFollowup {
                occurred_at: dates::parse_day_or_time("occurred_at", &self.occurred_at)?,
                contact_method: self.contact_method,
                contact_with: self.contact_with,
                summary: self.summary,
                notes: self.notes,
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateIncidentFollowupInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Id of the incident the followup belongs to, from `list_incidents`.
    pub incident_id: i32,
    /// Followup id, from the incident's `followups` in `list_incidents` or `get_incident`.
    pub followup_id: i32,
    /// When the contact happened, `YYYY-MM-DD HH:MM:SS` or just `YYYY-MM-DD`.
    pub occurred_at: Option<String>,
    /// What happened or was said. A followup always has one, so blank text is an error and `null` is the same as leaving it out.
    pub summary: Option<String>,
    /// How, e.g. `phone`, `email`.
    ///
    /// Send `null` or blank text to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub contact_method: Option<Option<String>>,
    /// Who with.
    ///
    /// Send `null` or blank text to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub contact_with: Option<Option<String>>,
    /// Free-form notes.
    ///
    /// Send `null` or blank text to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub notes: Option<Option<String>>,
}

impl UpdateIncidentFollowupInput {
    pub fn into_domain(self) -> DomainResult<(i32, i32, i32, UpdateFollowup)> {
        Ok((
            self.vehicle_id,
            self.incident_id,
            self.followup_id,
            UpdateFollowup {
                occurred_at: dates::parse_optional_day_or_time("occurred_at", self.occurred_at)?,
                summary: self.summary,
                contact_method: self.contact_method,
                contact_with: self.contact_with,
                notes: self.notes,
            },
        ))
    }
}

#[expect(
    clippy::struct_field_names,
    reason = "the field names are the ids the tool accepts"
)]
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteIncidentFollowupInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Id of the incident the followup belongs to, from `list_incidents`.
    pub incident_id: i32,
    /// Followup id, from the incident's `followups` in `list_incidents` or `get_incident`.
    pub followup_id: i32,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteIncidentInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Incident id, from `list_incidents` or `log_incident`.
    pub incident_id: i32,
}
