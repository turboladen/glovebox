use axum::{
    Json,
    extract::{Path, State},
};

use crate::AppState;
use glovebox_shared::services::costs::{self, CostSummary};

use super::{error::ApiError, require_vehicle};

type Result<T> = std::result::Result<T, ApiError>;

pub async fn get_costs(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<CostSummary>> {
    require_vehicle(&state.db, vehicle_id).await?;
    Ok(Json(costs::summary(&state.db, vehicle_id).await?))
}
