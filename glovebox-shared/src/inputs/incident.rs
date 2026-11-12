//! Domain inputs for the unified incident primitive (union of the retired
//! observation + accident input surfaces).

pub struct NewIncident {
    pub category: String,
    pub title: String,
    pub description: Option<String>,
    pub odometer: Option<i32>,
    /// Defaults to now (DB default) when omitted.
    pub occurred_at: Option<String>,
    pub obd_codes: Option<String>,
    pub notes: Option<String>,
    pub fault: Option<String>,
    pub other_party_name: Option<String>,
    pub other_party_phone: Option<String>,
    pub other_party_email: Option<String>,
    pub other_party_insurance: Option<String>,
    pub other_party_policy_number: Option<String>,
    pub insurance_claim_number: Option<String>,
    pub insurance_adjuster: Option<String>,
    pub insurance_adjuster_phone: Option<String>,
    /// Earlier same-vehicle incident this one is a recurrence of.
    pub recurrence_of_id: Option<i32>,
    /// Same-vehicle service where this incident was noticed.
    pub noticed_during_service_id: Option<i32>,
    /// Same-vehicle services that resolved this incident. Linking any marks
    /// the incident resolved.
    pub resolved_by_service_ids: Option<Vec<i32>>,
}

impl Default for NewIncident {
    fn default() -> Self {
        NewIncident {
            category: "general".into(),
            title: String::new(),
            description: None,
            odometer: None,
            occurred_at: None,
            obd_codes: None,
            notes: None,
            fault: None,
            other_party_name: None,
            other_party_phone: None,
            other_party_email: None,
            other_party_insurance: None,
            other_party_policy_number: None,
            insurance_claim_number: None,
            insurance_adjuster: None,
            insurance_adjuster_phone: None,
            recurrence_of_id: None,
            noticed_during_service_id: None,
            resolved_by_service_ids: None,
        }
    }
}

#[derive(Default)]
pub struct UpdateIncident {
    pub category: Option<String>,
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub odometer: Option<Option<i32>>,
    pub occurred_at: Option<String>,
    pub obd_codes: Option<Option<String>>,
    pub resolved: Option<bool>,
    pub notes: Option<Option<String>>,
    pub fault: Option<Option<String>>,
    pub other_party_name: Option<Option<String>>,
    pub other_party_phone: Option<Option<String>>,
    pub other_party_email: Option<Option<String>>,
    pub other_party_insurance: Option<Option<String>>,
    pub other_party_policy_number: Option<Option<String>>,
    pub insurance_claim_number: Option<Option<String>>,
    pub insurance_adjuster: Option<Option<String>>,
    pub insurance_adjuster_phone: Option<Option<String>>,
    pub total_repair_cost_cents: Option<Option<i32>>,
    pub total_repair_cost_currency: Option<Option<String>>,
    pub deductible_cents: Option<Option<i32>>,
    pub deductible_currency: Option<Option<String>>,
    pub insurance_payout_cents: Option<Option<i32>>,
    pub insurance_payout_currency: Option<Option<String>>,
    pub recurrence_of_id: Option<Option<i32>>,
    /// When present, replaces the services that resolved the incident
    /// wholesale and marks it resolved unless `resolved` says otherwise. An
    /// empty list removes every resolving service and marks the incident
    /// unresolved unless `resolved` says otherwise. The service that noticed
    /// the incident is left alone.
    pub resolved_by_service_ids: Option<Vec<i32>>,
}

pub struct NewFollowup {
    pub occurred_at: String,
    pub contact_method: Option<String>,
    pub contact_with: Option<String>,
    pub summary: String,
    pub notes: Option<String>,
}
