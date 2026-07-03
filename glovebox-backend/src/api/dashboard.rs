//! Dashboard + activity HTTP surface (2hea unit F): the garage-wide
//! dashboard aggregation, the garage-wide merged activity feed, and the
//! per-vehicle activity feed (the Timeline tab's stream).

use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::services::{
    activity,
    activity::ActivityItem,
    dashboard::{self, GarageDashboard},
};

use super::error::ApiError;

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct ActivityQuery {
    pub limit: Option<usize>,
}

pub async fn get_dashboard(State(state): State<AppState>) -> Result<Json<GarageDashboard>> {
    Ok(Json(dashboard::garage(&state.db).await?))
}

pub async fn garage_activity(
    State(state): State<AppState>,
    Query(q): Query<ActivityQuery>,
) -> Result<Json<Vec<ActivityItem>>> {
    let limit = q.limit.unwrap_or(activity::DEFAULT_LIMIT);
    Ok(Json(activity::recent_all(&state.db, limit).await?))
}

pub async fn vehicle_activity(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Query(q): Query<ActivityQuery>,
) -> Result<Json<Vec<ActivityItem>>> {
    let limit = q.limit.unwrap_or(activity::DEFAULT_LIMIT);
    Ok(Json(activity::recent(&state.db, vehicle_id, limit).await?))
}
