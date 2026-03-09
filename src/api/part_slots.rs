use axum::extract::{Path, State};
use axum::Json;
use sea_orm::{EntityTrait, QueryOrder, QueryFilter, ColumnTrait, Set, ActiveEnum, ActiveModelTrait, ModelTrait, ActiveModelBehavior};
use serde::Deserialize;

use crate::entities::{part_slot, vehicle};
use crate::AppState;

use super::error::ApiError;

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct CreatePartSlot {
    pub name: String,
    pub category: Option<String>,
    pub oe_spec: Option<String>,
    pub oe_part_number: Option<String>,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdatePartSlot {
    pub name: Option<String>,
    pub category: Option<Option<String>>,
    pub oe_spec: Option<Option<String>>,
    pub oe_part_number: Option<Option<String>>,
    pub notes: Option<Option<String>>,
}

pub async fn list(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<Vec<part_slot::Model>>> {
    vehicle::Entity::find_by_id(vehicle_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Vehicle {vehicle_id} not found")))?;

    let slots = part_slot::Entity::find()
        .filter(part_slot::Column::VehicleId.eq(vehicle_id))
        .order_by_asc(part_slot::Column::Category)
        .order_by_asc(part_slot::Column::Name)
        .all(&state.db)
        .await?;
    Ok(Json(slots))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<part_slot::Model>> {
    part_slot::Entity::find_by_id(id)
        .filter(part_slot::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("Part slot {id} not found")))
}

pub async fn create(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(input): Json<CreatePartSlot>,
) -> Result<Json<part_slot::Model>> {
    vehicle::Entity::find_by_id(vehicle_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Vehicle {vehicle_id} not found")))?;

    let model = part_slot::ActiveModel {
        vehicle_id: Set(vehicle_id),
        name: Set(input.name),
        category: Set(input.category),
        oe_spec: Set(input.oe_spec),
        oe_part_number: Set(input.oe_part_number),
        notes: Set(input.notes),
        ..Default::default()
    };

    let result = model.insert(&state.db).await?;
    Ok(Json(result))
}

pub async fn update(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    Json(input): Json<UpdatePartSlot>,
) -> Result<Json<part_slot::Model>> {
    let existing = part_slot::Entity::find_by_id(id)
        .filter(part_slot::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Part slot {id} not found")))?;

    let mut active: part_slot::ActiveModel = existing.into();

    if let Some(v) = input.name {
        active.name = Set(v);
    }
    if let Some(v) = input.category {
        active.category = Set(v);
    }
    if let Some(v) = input.oe_spec {
        active.oe_spec = Set(v);
    }
    if let Some(v) = input.oe_part_number {
        active.oe_part_number = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }

    let result = active.update(&state.db).await?;
    Ok(Json(result))
}

pub async fn delete(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<serde_json::Value>> {
    let existing = part_slot::Entity::find_by_id(id)
        .filter(part_slot::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Part slot {id} not found")))?;

    existing.delete(&state.db).await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}
