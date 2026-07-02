use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::{
    entities::part,
    inputs::part::{NewPart, PartFilter, UpdatePart as UpdatePartInput},
    services::part as svc,
};

use super::{error::ApiError, serde_helpers::deserialize_optional};

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct CreatePart {
    pub slot_id: Option<i32>,
    pub name: String,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub oe_part_number_replaced: Option<String>,
    pub seller: Option<String>,
    pub purchase_date: Option<String>,
    pub cost_cents: Option<i32>,
    pub cost_currency: Option<String>,
    pub invoice_url: Option<String>,
    pub manufacturer_url: Option<String>,
    pub retailer_url: Option<String>,
    pub status: Option<String>,
    pub installed_date: Option<String>,
    pub installed_odometer: Option<i32>,
    pub installed_service_id: Option<i32>,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdatePart {
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub slot_id: Option<Option<i32>>,
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub manufacturer: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub part_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub oe_part_number_replaced: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub seller: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub purchase_date: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub cost_cents: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub cost_currency: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub invoice_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub manufacturer_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub retailer_url: Option<Option<String>>,
    pub status: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub installed_date: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub installed_odometer: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub installed_service_id: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub replaced_date: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub replaced_odometer: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub notes: Option<Option<String>>,
}

#[derive(Deserialize)]
pub struct ListFilter {
    pub slot_id: Option<i32>,
    pub status: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Query(filter): Query<ListFilter>,
) -> Result<Json<Vec<part::Model>>> {
    glovebox_shared::services::vehicle::require(&state.db, vehicle_id).await?;
    let parts = svc::list(
        &state.db,
        vehicle_id,
        PartFilter {
            slot_id: filter.slot_id,
            status: filter.status,
        },
    )
    .await?;
    Ok(Json(parts))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<part::Model>> {
    Ok(Json(svc::get(&state.db, vehicle_id, id).await?))
}

pub async fn create(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(input): Json<CreatePart>,
) -> Result<Json<part::Model>> {
    glovebox_shared::services::vehicle::require(&state.db, vehicle_id).await?;
    let created = svc::create(
        &state.db,
        vehicle_id,
        NewPart {
            slot_id: input.slot_id,
            name: input.name,
            manufacturer: input.manufacturer,
            part_number: input.part_number,
            oe_part_number_replaced: input.oe_part_number_replaced,
            seller: input.seller,
            purchase_date: input.purchase_date,
            cost_cents: input.cost_cents,
            cost_currency: input.cost_currency,
            invoice_url: input.invoice_url,
            manufacturer_url: input.manufacturer_url,
            retailer_url: input.retailer_url,
            status: input.status,
            installed_date: input.installed_date,
            installed_odometer: input.installed_odometer,
            installed_service_id: input.installed_service_id,
            notes: input.notes,
        },
    )
    .await?;
    Ok(Json(created))
}

pub async fn update(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    Json(input): Json<UpdatePart>,
) -> Result<Json<part::Model>> {
    let updated = svc::update(
        &state.db,
        vehicle_id,
        id,
        UpdatePartInput {
            slot_id: input.slot_id,
            name: input.name,
            manufacturer: input.manufacturer,
            part_number: input.part_number,
            oe_part_number_replaced: input.oe_part_number_replaced,
            seller: input.seller,
            purchase_date: input.purchase_date,
            cost_cents: input.cost_cents,
            cost_currency: input.cost_currency,
            invoice_url: input.invoice_url,
            manufacturer_url: input.manufacturer_url,
            retailer_url: input.retailer_url,
            status: input.status,
            installed_date: input.installed_date,
            installed_odometer: input.installed_odometer,
            installed_service_id: input.installed_service_id,
            replaced_date: input.replaced_date,
            replaced_odometer: input.replaced_odometer,
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
