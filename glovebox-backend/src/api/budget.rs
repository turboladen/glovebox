use axum::{
    Json,
    extract::{Path, State},
};

use crate::AppState;
use glovebox_shared::services::budget::{self, BudgetForecast};

use super::error::ApiError;

type Result<T> = std::result::Result<T, ApiError>;

pub async fn get_budget(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<BudgetForecast>> {
    Ok(Json(budget::forecast(&state.db, vehicle_id).await?))
}
