use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::{
    entities::mileage_log, inputs::mileage::NewMileageEntry, services::mileage as svc,
};

use super::error::ApiError;

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct CreateMileageEntry {
    pub mileage: i32,
    pub recorded_at: Option<String>,
    pub source: Option<String>,
    pub notes: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<Vec<mileage_log::Model>>> {
    glovebox_shared::services::vehicle::require(&state.db, vehicle_id).await?;
    Ok(Json(svc::list(&state.db, vehicle_id).await?))
}

pub async fn create(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(input): Json<CreateMileageEntry>,
) -> Result<Json<mileage_log::Model>> {
    glovebox_shared::services::vehicle::require(&state.db, vehicle_id).await?;
    let created = svc::create(
        &state.db,
        vehicle_id,
        NewMileageEntry {
            mileage: input.mileage,
            recorded_at: input.recorded_at,
            source: input.source,
            notes: input.notes,
        },
    )
    .await?;
    Ok(Json(created))
}
