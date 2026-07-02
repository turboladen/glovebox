use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::{
    entities::observation,
    inputs::observation::{NewObservation, UpdateObservation as UpdateObservationInput},
    services::observation as svc,
};

use super::{error::ApiError, serde_helpers::deserialize_optional};

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct CreateObservation {
    pub category: String,
    pub title: String,
    pub description: Option<String>,
    pub odometer: Option<i32>,
    pub observed_at: Option<String>,
    pub obd_codes: Option<String>,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateObservation {
    pub category: Option<String>,
    pub title: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub odometer: Option<Option<i32>>,
    pub observed_at: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub obd_codes: Option<Option<String>>,
    pub resolved: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub resolved_service_id: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub notes: Option<Option<String>>,
}

pub async fn list(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<Vec<observation::Model>>> {
    glovebox_shared::services::vehicle::require(&state.db, vehicle_id).await?;
    Ok(Json(svc::list(&state.db, vehicle_id).await?))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<observation::Model>> {
    Ok(Json(svc::get(&state.db, vehicle_id, id).await?))
}

pub async fn create(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(input): Json<CreateObservation>,
) -> Result<Json<observation::Model>> {
    glovebox_shared::services::vehicle::require(&state.db, vehicle_id).await?;
    let created = svc::create(
        &state.db,
        vehicle_id,
        NewObservation {
            category: input.category,
            title: input.title,
            description: input.description,
            odometer: input.odometer,
            observed_at: input.observed_at,
            obd_codes: input.obd_codes,
            notes: input.notes,
        },
    )
    .await?;
    Ok(Json(created))
}

pub async fn update(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    Json(input): Json<UpdateObservation>,
) -> Result<Json<observation::Model>> {
    let updated = svc::update(
        &state.db,
        vehicle_id,
        id,
        UpdateObservationInput {
            category: input.category,
            title: input.title,
            description: input.description,
            odometer: input.odometer,
            observed_at: input.observed_at,
            obd_codes: input.obd_codes,
            resolved: input.resolved,
            resolved_service_id: input.resolved_service_id,
            notes: input.notes,
        },
    )
    .await?;
    Ok(Json(updated))
}
