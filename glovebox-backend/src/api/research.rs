use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::{
    entities::{research_finding, research_report},
    inputs::research::UpdateFinding,
    services::research::{self as svc, ReportWithFindings},
};

use super::{error::ApiError, serde_helpers::deserialize_optional};

// --- Recall check ---

pub async fn check_recalls(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<glovebox_shared::services::nhtsa::RecallCheckResult>, ApiError> {
    Ok(Json(svc::check_recalls(&state.db, vehicle_id).await?))
}

// --- Research reports ---

pub async fn list_reports(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<Vec<research_report::Model>>, ApiError> {
    Ok(Json(svc::list_reports(&state.db, vehicle_id).await?))
}

pub async fn get_report(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<ReportWithFindings>, ApiError> {
    Ok(Json(svc::get_report(&state.db, vehicle_id, id).await?))
}

// --- List findings by status (cross-report) ---

#[derive(Deserialize)]
pub struct FindingsQuery {
    pub status: Option<String>,
}

pub async fn list_findings(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Query(query): Query<FindingsQuery>,
) -> Result<Json<Vec<research_finding::Model>>, ApiError> {
    Ok(Json(
        svc::list_findings(&state.db, vehicle_id, query.status).await?,
    ))
}

// --- Finding management ---

#[derive(Deserialize)]
pub struct UpdateFindingRequest {
    pub status: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub linked_entity_type: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub linked_entity_id: Option<Option<i32>>,
}

pub async fn update_finding_with_body(
    State(state): State<AppState>,
    Path((vehicle_id, report_id, id)): Path<(i32, i32, i32)>,
    Json(body): Json<UpdateFindingRequest>,
) -> Result<Json<research_finding::Model>, ApiError> {
    let updated = svc::update_finding(
        &state.db,
        vehicle_id,
        report_id,
        id,
        UpdateFinding {
            status: body.status,
            linked_entity_type: body.linked_entity_type,
            linked_entity_id: body.linked_entity_id,
        },
    )
    .await?;
    Ok(Json(updated))
}
