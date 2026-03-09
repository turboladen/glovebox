use axum::extract::{Path, Query, State};
use axum::Json;
use sea_orm::*;
use serde::Deserialize;

use crate::entities::part;
use crate::AppState;

use super::error::ApiError;
use super::require_vehicle;

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
    pub slot_id: Option<Option<i32>>,
    pub name: Option<String>,
    pub manufacturer: Option<Option<String>>,
    pub part_number: Option<Option<String>>,
    pub oe_part_number_replaced: Option<Option<String>>,
    pub seller: Option<Option<String>>,
    pub purchase_date: Option<Option<String>>,
    pub cost_cents: Option<Option<i32>>,
    pub cost_currency: Option<Option<String>>,
    pub invoice_url: Option<Option<String>>,
    pub manufacturer_url: Option<Option<String>>,
    pub retailer_url: Option<Option<String>>,
    pub status: Option<String>,
    pub installed_date: Option<Option<String>>,
    pub installed_odometer: Option<Option<i32>>,
    pub installed_service_id: Option<Option<i32>>,
    pub replaced_date: Option<Option<String>>,
    pub replaced_odometer: Option<Option<i32>>,
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
    require_vehicle(&state.db, vehicle_id).await?;

    let mut query = part::Entity::find().filter(part::Column::VehicleId.eq(vehicle_id));

    if let Some(slot_id) = filter.slot_id {
        query = query.filter(part::Column::SlotId.eq(slot_id));
    }
    if let Some(status) = filter.status {
        query = query.filter(part::Column::Status.eq(status));
    }

    let parts = query
        .order_by_desc(part::Column::CreatedAt)
        .all(&state.db)
        .await?;
    Ok(Json(parts))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<part::Model>> {
    part::Entity::find_by_id(id)
        .filter(part::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("Part {id} not found")))
}

pub async fn create(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(input): Json<CreatePart>,
) -> Result<Json<part::Model>> {
    require_vehicle(&state.db, vehicle_id).await?;

    let model = part::ActiveModel {
        vehicle_id: Set(vehicle_id),
        slot_id: Set(input.slot_id),
        name: Set(input.name),
        manufacturer: Set(input.manufacturer),
        part_number: Set(input.part_number),
        oe_part_number_replaced: Set(input.oe_part_number_replaced),
        seller: Set(input.seller),
        purchase_date: Set(input.purchase_date),
        cost_cents: Set(input.cost_cents),
        cost_currency: Set(input.cost_currency),
        invoice_url: Set(input.invoice_url),
        manufacturer_url: Set(input.manufacturer_url),
        retailer_url: Set(input.retailer_url),
        status: Set(input.status.unwrap_or_else(|| "purchased".to_string())),
        installed_date: Set(input.installed_date),
        installed_odometer: Set(input.installed_odometer),
        installed_service_id: Set(input.installed_service_id),
        notes: Set(input.notes),
        ..Default::default()
    };

    let result = model.insert(&state.db).await?;
    Ok(Json(result))
}

pub async fn update(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    Json(input): Json<UpdatePart>,
) -> Result<Json<part::Model>> {
    let existing = part::Entity::find_by_id(id)
        .filter(part::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Part {id} not found")))?;

    let mut active: part::ActiveModel = existing.into();

    if let Some(v) = input.slot_id {
        active.slot_id = Set(v);
    }
    if let Some(v) = input.name {
        active.name = Set(v);
    }
    if let Some(v) = input.manufacturer {
        active.manufacturer = Set(v);
    }
    if let Some(v) = input.part_number {
        active.part_number = Set(v);
    }
    if let Some(v) = input.oe_part_number_replaced {
        active.oe_part_number_replaced = Set(v);
    }
    if let Some(v) = input.seller {
        active.seller = Set(v);
    }
    if let Some(v) = input.purchase_date {
        active.purchase_date = Set(v);
    }
    if let Some(v) = input.cost_cents {
        active.cost_cents = Set(v);
    }
    if let Some(v) = input.cost_currency {
        active.cost_currency = Set(v);
    }
    if let Some(v) = input.invoice_url {
        active.invoice_url = Set(v);
    }
    if let Some(v) = input.manufacturer_url {
        active.manufacturer_url = Set(v);
    }
    if let Some(v) = input.retailer_url {
        active.retailer_url = Set(v);
    }
    if let Some(v) = input.status {
        active.status = Set(v);
    }
    if let Some(v) = input.installed_date {
        active.installed_date = Set(v);
    }
    if let Some(v) = input.installed_odometer {
        active.installed_odometer = Set(v);
    }
    if let Some(v) = input.installed_service_id {
        active.installed_service_id = Set(v);
    }
    if let Some(v) = input.replaced_date {
        active.replaced_date = Set(v);
    }
    if let Some(v) = input.replaced_odometer {
        active.replaced_odometer = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }

    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

    let result = active.update(&state.db).await?;
    Ok(Json(result))
}

pub async fn delete(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<serde_json::Value>> {
    let existing = part::Entity::find_by_id(id)
        .filter(part::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Part {id} not found")))?;

    existing.delete(&state.db).await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}
