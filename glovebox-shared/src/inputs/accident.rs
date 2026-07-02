pub struct NewAccident {
    pub occurred_at: String,
    pub odometer: Option<i32>,
    pub description: String,
    pub fault: Option<String>,
    pub other_party_name: Option<String>,
    pub other_party_phone: Option<String>,
    pub other_party_email: Option<String>,
    pub other_party_insurance: Option<String>,
    pub other_party_policy_number: Option<String>,
    pub insurance_claim_number: Option<String>,
    pub insurance_adjuster: Option<String>,
    pub insurance_adjuster_phone: Option<String>,
    pub notes: Option<String>,
    pub service_record_ids: Option<Vec<i32>>,
}

#[derive(Default)]
pub struct UpdateAccident {
    pub occurred_at: Option<String>,
    pub odometer: Option<Option<i32>>,
    pub description: Option<String>,
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
    pub resolved: Option<bool>,
    pub notes: Option<Option<String>>,
    pub service_record_ids: Option<Vec<i32>>,
}

pub struct NewCorrespondence {
    pub occurred_at: String,
    pub contact_method: Option<String>,
    pub contact_with: Option<String>,
    pub summary: String,
    pub notes: Option<String>,
}
