use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::{
    inputs::service_record::{
        NewLineItem, NewServiceRecord, UpdateServiceRecord as UpdateServiceRecordInput,
    },
    services::service_record::{self as svc, ServiceRecordWithLinks},
};

use super::{error::ApiError, serde_helpers::deserialize_optional};

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct CreateLineItem {
    pub description: String,
    pub category: Option<String>,
    pub quantity: Option<f64>,
    pub unit_cost_cents: Option<i32>,
    pub cost_cents: Option<i32>,
}

impl From<CreateLineItem> for NewLineItem {
    fn from(item: CreateLineItem) -> Self {
        NewLineItem {
            description: item.description,
            category: item.category,
            quantity: item.quantity,
            unit_cost_cents: item.unit_cost_cents,
            cost_cents: item.cost_cents,
        }
    }
}

#[derive(Deserialize)]
pub struct CreateServiceRecord {
    pub service_date: String,
    pub mileage: Option<i32>,
    pub description: Option<String>,
    pub parts_cost_cents: Option<i32>,
    pub parts_cost_currency: Option<String>,
    pub labor_cost_cents: Option<i32>,
    pub labor_cost_currency: Option<String>,
    pub total_cost_cents: Option<i32>,
    pub total_cost_currency: Option<String>,
    pub shop_name: Option<String>,
    pub shop_id: Option<i32>,
    pub notes: Option<String>,
    pub build_id: Option<i32>,
    pub schedule_item_ids: Option<Vec<i32>>,
    pub part_ids: Option<Vec<i32>>,
    pub line_items: Option<Vec<CreateLineItem>>,
}

#[derive(Deserialize)]
pub struct UpdateServiceRecord {
    pub service_date: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub mileage: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub parts_cost_cents: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub parts_cost_currency: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub labor_cost_cents: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub labor_cost_currency: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub total_cost_cents: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub total_cost_currency: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub shop_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub shop_id: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub notes: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub build_id: Option<Option<i32>>,
    pub schedule_item_ids: Option<Vec<i32>>,
    pub part_ids: Option<Vec<i32>>,
    pub line_items: Option<Vec<CreateLineItem>>,
}

pub async fn list(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<Vec<ServiceRecordWithLinks>>> {
    glovebox_shared::services::vehicle::require(&state.db, vehicle_id).await?;
    Ok(Json(svc::list(&state.db, vehicle_id).await?))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<ServiceRecordWithLinks>> {
    glovebox_shared::services::vehicle::require(&state.db, vehicle_id).await?;
    Ok(Json(svc::get(&state.db, vehicle_id, id).await?))
}

pub async fn create(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(input): Json<CreateServiceRecord>,
) -> Result<Json<ServiceRecordWithLinks>> {
    glovebox_shared::services::vehicle::require(&state.db, vehicle_id).await?;
    let created = svc::create(
        &state.db,
        vehicle_id,
        NewServiceRecord {
            service_date: input.service_date,
            mileage: input.mileage,
            description: input.description,
            parts_cost_cents: input.parts_cost_cents,
            parts_cost_currency: input.parts_cost_currency,
            labor_cost_cents: input.labor_cost_cents,
            labor_cost_currency: input.labor_cost_currency,
            total_cost_cents: input.total_cost_cents,
            total_cost_currency: input.total_cost_currency,
            shop_name: input.shop_name,
            shop_id: input.shop_id,
            notes: input.notes,
            build_id: input.build_id,
            schedule_item_ids: input.schedule_item_ids,
            part_ids: input.part_ids,
            line_items: input
                .line_items
                .map(|items| items.into_iter().map(Into::into).collect()),
        },
    )
    .await?;
    Ok(Json(created))
}

pub async fn update(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    Json(input): Json<UpdateServiceRecord>,
) -> Result<Json<ServiceRecordWithLinks>> {
    glovebox_shared::services::vehicle::require(&state.db, vehicle_id).await?;
    let updated = svc::update(
        &state.db,
        vehicle_id,
        id,
        UpdateServiceRecordInput {
            service_date: input.service_date,
            mileage: input.mileage,
            description: input.description,
            parts_cost_cents: input.parts_cost_cents,
            parts_cost_currency: input.parts_cost_currency,
            labor_cost_cents: input.labor_cost_cents,
            labor_cost_currency: input.labor_cost_currency,
            total_cost_cents: input.total_cost_cents,
            total_cost_currency: input.total_cost_currency,
            shop_name: input.shop_name,
            shop_id: input.shop_id,
            notes: input.notes,
            build_id: input.build_id,
            schedule_item_ids: input.schedule_item_ids,
            part_ids: input.part_ids,
            line_items: input
                .line_items
                .map(|items| items.into_iter().map(Into::into).collect()),
        },
    )
    .await?;
    Ok(Json(updated))
}

pub async fn delete(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<serde_json::Value>> {
    glovebox_shared::services::vehicle::require(&state.db, vehicle_id).await?;
    svc::delete(&state.db, vehicle_id, id).await?;
    Ok(Json(serde_json::json!({ "deleted": id })))
}
