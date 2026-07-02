use axum::{
    Json,
    extract::{Path, State},
};

use crate::AppState;
use glovebox_shared::services::costs::{self, CostSummary};

use super::error::ApiError;

type Result<T> = std::result::Result<T, ApiError>;

pub async fn get_costs(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<CostSummary>> {
    glovebox_shared::services::vehicle::require(&state.db, vehicle_id).await?;
    Ok(Json(costs::summary(&state.db, vehicle_id).await?))
}
