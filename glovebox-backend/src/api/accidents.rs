use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::{
    entities::accident_correspondence,
    inputs::accident::{
        NewAccident, NewCorrespondence, UpdateAccident as UpdateAccidentInput,
    },
    services::{
        accident::{self as svc, AccidentWithDetails},
        vehicle as vehicle_svc,
    },
};

use super::{error::ApiError, serde_helpers::deserialize_optional};

type Result<T> = std::result::Result<T, ApiError>;

// --- DTOs ---

#[derive(Deserialize)]
pub struct CreateAccident {
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

#[derive(Deserialize)]
pub struct UpdateAccident {
    pub occurred_at: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub odometer: Option<Option<i32>>,
    pub description: Option<String>,
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
    pub resolved: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub notes: Option<Option<String>>,
    pub service_record_ids: Option<Vec<i32>>,
}

#[derive(Deserialize)]
pub struct CreateCorrespondence {
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
) -> Result<Json<Vec<AccidentWithDetails>>> {
    vehicle_svc::require(&state.db, vehicle_id).await?;
    Ok(Json(svc::list(&state.db, vehicle_id).await?))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<AccidentWithDetails>> {
    vehicle_svc::require(&state.db, vehicle_id).await?;
    Ok(Json(svc::get(&state.db, vehicle_id, id).await?))
}

pub async fn create(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(input): Json<CreateAccident>,
) -> Result<Json<AccidentWithDetails>> {
    vehicle_svc::require(&state.db, vehicle_id).await?;
    let created = svc::create(
        &state.db,
        vehicle_id,
        NewAccident {
            occurred_at: input.occurred_at,
            odometer: input.odometer,
            description: input.description,
            fault: input.fault,
            other_party_name: input.other_party_name,
            other_party_phone: input.other_party_phone,
            other_party_email: input.other_party_email,
            other_party_insurance: input.other_party_insurance,
            other_party_policy_number: input.other_party_policy_number,
            insurance_claim_number: input.insurance_claim_number,
            insurance_adjuster: input.insurance_adjuster,
            insurance_adjuster_phone: input.insurance_adjuster_phone,
            notes: input.notes,
            service_record_ids: input.service_record_ids,
        },
    )
    .await?;
    Ok(Json(created))
}

pub async fn update(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    Json(input): Json<UpdateAccident>,
) -> Result<Json<AccidentWithDetails>> {
    vehicle_svc::require(&state.db, vehicle_id).await?;
    let updated = svc::update(
        &state.db,
        vehicle_id,
        id,
        UpdateAccidentInput {
            occurred_at: input.occurred_at,
            odometer: input.odometer,
            description: input.description,
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
            resolved: input.resolved,
            notes: input.notes,
            service_record_ids: input.service_record_ids,
        },
    )
    .await?;
    Ok(Json(updated))
}

// --- Correspondence sub-resource ---

pub async fn list_correspondence(
    State(state): State<AppState>,
    Path((_vehicle_id, accident_id)): Path<(i32, i32)>,
) -> Result<Json<Vec<accident_correspondence::Model>>> {
    Ok(Json(svc::list_correspondence(&state.db, accident_id).await?))
}

pub async fn create_correspondence(
    State(state): State<AppState>,
    Path((_vehicle_id, accident_id)): Path<(i32, i32)>,
    Json(input): Json<CreateCorrespondence>,
) -> Result<Json<accident_correspondence::Model>> {
    let created = svc::create_correspondence(
        &state.db,
        accident_id,
        NewCorrespondence {
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
