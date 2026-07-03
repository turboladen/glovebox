use axum::{
    Json,
    extract::{Query, State},
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::services::search::{self as svc, SearchHit, SearchScope};

use super::error::ApiError;

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct SearchParams {
    pub q: String,
    pub scope: Option<String>,
    pub vehicle_id: Option<i32>,
}

pub async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Vec<SearchHit>>> {
    let scope = match params.scope.as_deref() {
        None => SearchScope::All,
        Some(s) => SearchScope::parse(s)?,
    };
    Ok(Json(
        svc::search(&state.db, &params.q, scope, params.vehicle_id).await?,
    ))
}
