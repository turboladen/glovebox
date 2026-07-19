use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::{
    entities::incident_followup,
    inputs::incident::{NewFollowup, NewIncident, UpdateIncident as UpdateIncidentInput},
    services::{
        incident::{self as svc, IncidentWithDetails},
        vehicle as vehicle_svc,
    },
};

use super::{error::ApiError, serde_helpers::deserialize_optional};

type Result<T> = std::result::Result<T, ApiError>;

// --- DTOs ---

#[derive(Deserialize)]
pub struct CreateIncident {
    pub category: String,
    pub title: String,
    pub description: Option<String>,
    pub odometer: Option<i32>,
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
    pub recurrence_of_id: Option<i32>,
    pub build_id: Option<i32>,
    pub service_record_ids: Option<Vec<i32>>,
}

#[derive(Deserialize)]
pub struct UpdateIncident {
    pub category: Option<String>,
    pub title: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub odometer: Option<Option<i32>>,
    pub occurred_at: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub obd_codes: Option<Option<String>>,
    pub resolved: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub notes: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub fault: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub other_party_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub other_party_phone: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub other_party_email: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub other_party_insurance: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub other_party_policy_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub insurance_claim_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub insurance_adjuster: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub insurance_adjuster_phone: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub total_repair_cost_cents: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub total_repair_cost_currency: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub deductible_cents: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub deductible_currency: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub insurance_payout_cents: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub insurance_payout_currency: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub recurrence_of_id: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub build_id: Option<Option<i32>>,
    pub service_record_ids: Option<Vec<i32>>,
}

#[derive(Deserialize)]
pub struct CreateFollowup {
    pub occurred_at: String,
    pub contact_method: Option<String>,
    pub contact_with: Option<String>,
    pub summary: String,
    pub notes: Option<String>,
}

// --- Handlers ---

pub async fn list(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<Vec<IncidentWithDetails>>> {
    vehicle_svc::require(&state.db, vehicle_id).await?;
    Ok(Json(svc::list(&state.db, vehicle_id).await?))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<IncidentWithDetails>> {
    vehicle_svc::require(&state.db, vehicle_id).await?;
    Ok(Json(svc::get(&state.db, vehicle_id, id).await?))
}

pub async fn create(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(input): Json<CreateIncident>,
) -> Result<Json<IncidentWithDetails>> {
    vehicle_svc::require(&state.db, vehicle_id).await?;
    let created = svc::create(
        &state.db,
        vehicle_id,
        NewIncident {
            category: input.category,
            title: input.title,
            description: input.description,
            odometer: input.odometer,
            occurred_at: input.occurred_at,
            obd_codes: input.obd_codes,
            notes: input.notes,
            fault: input.fault,
            other_party_name: input.other_party_name,
            other_party_phone: input.other_party_phone,
            other_party_email: input.other_party_email,
            other_party_insurance: input.other_party_insurance,
            other_party_policy_number: input.other_party_policy_number,
            insurance_claim_number: input.insurance_claim_number,
            insurance_adjuster: input.insurance_adjuster,
            insurance_adjuster_phone: input.insurance_adjuster_phone,
            recurrence_of_id: input.recurrence_of_id,
            build_id: input.build_id,
            service_record_ids: input.service_record_ids,
        },
    )
    .await?;
    Ok(Json(created))
}

pub async fn update(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    Json(input): Json<UpdateIncident>,
) -> Result<Json<IncidentWithDetails>> {
    vehicle_svc::require(&state.db, vehicle_id).await?;
    let updated = svc::update(
        &state.db,
        vehicle_id,
        id,
        UpdateIncidentInput {
            category: input.category,
            title: input.title,
            description: input.description,
            odometer: input.odometer,
            occurred_at: input.occurred_at,
            obd_codes: input.obd_codes,
            resolved: input.resolved,
            notes: input.notes,
            fault: input.fault,
            other_party_name: input.other_party_name,
            other_party_phone: input.other_party_phone,
            other_party_email: input.other_party_email,
            other_party_insurance: input.other_party_insurance,
            other_party_policy_number: input.other_party_policy_number,
            insurance_claim_number: input.insurance_claim_number,
            insurance_adjuster: input.insurance_adjuster,
            insurance_adjuster_phone: input.insurance_adjuster_phone,
            total_repair_cost_cents: input.total_repair_cost_cents,
            total_repair_cost_currency: input.total_repair_cost_currency,
            deductible_cents: input.deductible_cents,
            deductible_currency: input.deductible_currency,
            insurance_payout_cents: input.insurance_payout_cents,
            insurance_payout_currency: input.insurance_payout_currency,
            recurrence_of_id: input.recurrence_of_id,
            build_id: input.build_id,
            service_record_ids: input.service_record_ids,
        },
    )
    .await?;
    Ok(Json(updated))
}

pub async fn delete(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    q: super::documents::DeleteDocsQuery,
) -> Result<Json<serde_json::Value>> {
    vehicle_svc::require(&state.db, vehicle_id).await?;
    let doc_files = svc::delete(&state.db, vehicle_id, id, q.documents).await?;
    super::documents::remove_files_best_effort(&state.config, &doc_files).await;
    Ok(Json(serde_json::json!({ "deleted": id })))
}

// --- Followups sub-resource ---

pub async fn list_followups(
    State(state): State<AppState>,
    Path((vehicle_id, incident_id)): Path<(i32, i32)>,
) -> Result<Json<Vec<incident_followup::Model>>> {
    vehicle_svc::require(&state.db, vehicle_id).await?;
    Ok(Json(
        svc::list_followups(&state.db, vehicle_id, incident_id).await?,
    ))
}

pub async fn create_followup(
    State(state): State<AppState>,
    Path((vehicle_id, incident_id)): Path<(i32, i32)>,
    Json(input): Json<CreateFollowup>,
) -> Result<Json<incident_followup::Model>> {
    vehicle_svc::require(&state.db, vehicle_id).await?;
    let created = svc::create_followup(
        &state.db,
        vehicle_id,
        incident_id,
        NewFollowup {
            occurred_at: input.occurred_at,
            contact_method: input.contact_method,
            contact_with: input.contact_with,
            summary: input.summary,
            notes: input.notes,
        },
    )
    .await?;
    Ok(Json(created))
}
