use axum::{
    Json,
    extract::{Path, State},
};

use crate::AppState;
use glovebox_shared::services::export::{self as svc, VehicleExport};

use super::error::ApiError;

type Result<T> = std::result::Result<T, ApiError>;

pub async fn export_history(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<VehicleExport>> {
    Ok(Json(svc::vehicle_history(&state.db, vehicle_id).await?))
}
