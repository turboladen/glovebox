use axum::{
    Json,
    extract::{Path, State},
};
use sea_orm::*;
use serde::Deserialize;

use crate::{AppState, entities::mileage_log};

use super::{error::ApiError, require_vehicle};

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
    require_vehicle(&state.db, vehicle_id).await?;

    let entries = mileage_log::Entity::find()
        .filter(mileage_log::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(mileage_log::Column::RecordedAt)
        .all(&state.db)
        .await?;
    Ok(Json(entries))
}

pub async fn create(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(input): Json<CreateMileageEntry>,
) -> Result<Json<mileage_log::Model>> {
    require_vehicle(&state.db, vehicle_id).await?;

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let model = mileage_log::ActiveModel {
        vehicle_id: Set(vehicle_id),
        mileage: Set(input.mileage),
        recorded_at: Set(input.recorded_at.unwrap_or(now)),
        source: Set(input.source.or(Some("manual".to_string()))),
        notes: Set(input.notes),
        ..Default::default()
    };
    let result = model.insert(&state.db).await?;
    Ok(Json(result))
}
