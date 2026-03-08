use axum::extract::{Path, State};
use axum::Json;
use sea_orm::*;
use serde::Deserialize;

use crate::entities::{observation, vehicle};
use crate::AppState;

use super::error::ApiError;

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct CreateObservation {
    pub category: String,
    pub title: String,
    pub description: Option<String>,
    pub odometer: Option<i32>,
    pub observed_at: Option<String>,
    pub obd_codes: Option<String>,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateObservation {
    pub category: Option<String>,
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub odometer: Option<Option<i32>>,
    pub observed_at: Option<String>,
    pub obd_codes: Option<Option<String>>,
    pub resolved: Option<bool>,
    pub resolved_service_id: Option<Option<i32>>,
    pub notes: Option<Option<String>>,
}

pub async fn list(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<Vec<observation::Model>>> {
    vehicle::Entity::find_by_id(vehicle_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Vehicle {vehicle_id} not found")))?;

    let observations = observation::Entity::find()
        .filter(observation::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(observation::Column::ObservedAt)
        .all(&state.db)
        .await?;
    Ok(Json(observations))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<observation::Model>> {
    observation::Entity::find_by_id(id)
        .filter(observation::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("Observation {id} not found")))
}

pub async fn create(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(input): Json<CreateObservation>,
) -> Result<Json<observation::Model>> {
    vehicle::Entity::find_by_id(vehicle_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Vehicle {vehicle_id} not found")))?;

    let mut model = observation::ActiveModel {
        vehicle_id: Set(vehicle_id),
        category: Set(input.category),
        title: Set(input.title),
        description: Set(input.description),
        odometer: Set(input.odometer),
        obd_codes: Set(input.obd_codes),
        notes: Set(input.notes),
        ..Default::default()
    };

    if let Some(observed_at) = input.observed_at {
        model.observed_at = Set(observed_at);
    }

    let result = model.insert(&state.db).await?;
    Ok(Json(result))
}

pub async fn update(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    Json(input): Json<UpdateObservation>,
) -> Result<Json<observation::Model>> {
    let existing = observation::Entity::find_by_id(id)
        .filter(observation::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Observation {id} not found")))?;

    let mut active: observation::ActiveModel = existing.into();

    if let Some(v) = input.category { active.category = Set(v); }
    if let Some(v) = input.title { active.title = Set(v); }
    if let Some(v) = input.description { active.description = Set(v); }
    if let Some(v) = input.odometer { active.odometer = Set(v); }
    if let Some(v) = input.observed_at { active.observed_at = Set(v); }
    if let Some(v) = input.obd_codes { active.obd_codes = Set(v); }
    if let Some(v) = input.resolved { active.resolved = Set(v); }
    if let Some(v) = input.resolved_service_id { active.resolved_service_id = Set(v); }
    if let Some(v) = input.notes { active.notes = Set(v); }

    let result = active.update(&state.db).await?;
    Ok(Json(result))
}
