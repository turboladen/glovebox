use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::{
    entities::part_slot,
    inputs::part_slot::{NewPartSlot, UpdatePartSlot as UpdatePartSlotInput},
    services::part_slot as svc,
};

use super::{error::ApiError, require_vehicle, serde_helpers::deserialize_optional};

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
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub category: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub oe_spec: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub oe_part_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub notes: Option<Option<String>>,
}

pub async fn list(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<Vec<part_slot::Model>>> {
    require_vehicle(&state.db, vehicle_id).await?;
    Ok(Json(svc::list(&state.db, vehicle_id).await?))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<part_slot::Model>> {
    Ok(Json(svc::get(&state.db, vehicle_id, id).await?))
}

pub async fn create(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(input): Json<CreatePartSlot>,
) -> Result<Json<part_slot::Model>> {
    require_vehicle(&state.db, vehicle_id).await?;
    let created = svc::create(
        &state.db,
        vehicle_id,
        NewPartSlot {
            name: input.name,
            category: input.category,
            oe_spec: input.oe_spec,
            oe_part_number: input.oe_part_number,
            notes: input.notes,
        },
    )
    .await?;
    Ok(Json(created))
}

pub async fn update(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    Json(input): Json<UpdatePartSlot>,
) -> Result<Json<part_slot::Model>> {
    let updated = svc::update(
        &state.db,
        vehicle_id,
        id,
        UpdatePartSlotInput {
            name: input.name,
            category: input.category,
            oe_spec: input.oe_spec,
            oe_part_number: input.oe_part_number,
            notes: input.notes,
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
