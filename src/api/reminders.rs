use axum::extract::{Path, State};
use axum::Json;

use crate::services::reminders::{self, RemindersResponse};
use crate::AppState;

use super::error::ApiError;

type Result<T> = std::result::Result<T, ApiError>;

pub async fn get_reminders(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<RemindersResponse>> {
    let response = reminders::calculate_reminders(&state.db, vehicle_id)
        .await
        .map_err(|e| match e {
            sea_orm::DbErr::RecordNotFound(msg) => ApiError::NotFound(msg),
            other => ApiError::Internal(other.to_string()),
        })?;
    Ok(Json(response))
}
