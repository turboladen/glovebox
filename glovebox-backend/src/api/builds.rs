use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::{
    entities::build,
    inputs::build::{NewBuild, UpdateBuild as UpdateBuildInput},
    services::build::{self as svc, BuildProgress},
};

use super::{error::ApiError, serde_helpers::deserialize_optional};

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct CreateBuild {
    pub name: String,
    pub description: Option<String>,
    pub target_date: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateBuild {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub target_date: Option<Option<String>>,
    pub status: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<Vec<build::Model>>> {
    Ok(Json(svc::list(&state.db, vehicle_id).await?))
}

pub async fn create(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(input): Json<CreateBuild>,
) -> Result<Json<build::Model>> {
    let created = svc::create(
        &state.db,
        vehicle_id,
        NewBuild {
            name: input.name,
            description: input.description,
            target_date: input.target_date,
        },
    )
    .await?;
    Ok(Json(created))
}

/// Detail view is the derived progress rollup (build + linked-record counts/costs).
pub async fn get_one(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<BuildProgress>> {
    Ok(Json(svc::progress(&state.db, vehicle_id, id).await?))
}

pub async fn update(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    Json(input): Json<UpdateBuild>,
) -> Result<Json<build::Model>> {
    let updated = svc::update(
        &state.db,
        vehicle_id,
        id,
        UpdateBuildInput {
            name: input.name,
            description: input.description,
            target_date: input.target_date,
            status: input.status,
        },
    )
    .await?;
    Ok(Json(updated))
}

pub async fn delete(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<serde_json::Value>> {
    svc::delete(&state.db, vehicle_id, id).await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}
