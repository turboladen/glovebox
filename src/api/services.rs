use axum::extract::{Path, State};
use axum::Json;
use sea_orm::*;
use serde::{Deserialize, Serialize};

use crate::entities::{mileage_log, service_record, service_schedule_link, vehicle};
use crate::AppState;

use super::error::ApiError;

type Result<T> = std::result::Result<T, ApiError>;

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
    pub notes: Option<String>,
    pub schedule_item_ids: Option<Vec<i32>>,
}

#[derive(Deserialize)]
pub struct UpdateServiceRecord {
    pub service_date: Option<String>,
    pub mileage: Option<Option<i32>>,
    pub description: Option<Option<String>>,
    pub parts_cost_cents: Option<Option<i32>>,
    pub parts_cost_currency: Option<Option<String>>,
    pub labor_cost_cents: Option<Option<i32>>,
    pub labor_cost_currency: Option<Option<String>>,
    pub total_cost_cents: Option<Option<i32>>,
    pub total_cost_currency: Option<Option<String>>,
    pub shop_name: Option<Option<String>>,
    pub notes: Option<Option<String>>,
    pub schedule_item_ids: Option<Vec<i32>>,
}

#[derive(Serialize)]
pub struct ServiceRecordWithLinks {
    #[serde(flatten)]
    pub record: service_record::Model,
    pub schedule_item_ids: Vec<i32>,
}

pub async fn list(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<Vec<ServiceRecordWithLinks>>> {
    vehicle::Entity::find_by_id(vehicle_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Vehicle {vehicle_id} not found")))?;

    let records = service_record::Entity::find()
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(service_record::Column::ServiceDate)
        .all(&state.db)
        .await?;

    let mut results = Vec::with_capacity(records.len());
    for record in records {
        let links = service_schedule_link::Entity::find()
            .filter(service_schedule_link::Column::ServiceRecordId.eq(record.id))
            .all(&state.db)
            .await?;
        let schedule_item_ids = links.into_iter().map(|l| l.schedule_item_id).collect();
        results.push(ServiceRecordWithLinks {
            record,
            schedule_item_ids,
        });
    }
    Ok(Json(results))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<ServiceRecordWithLinks>> {
    let record = service_record::Entity::find_by_id(id)
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Service record {id} not found")))?;

    let links = service_schedule_link::Entity::find()
        .filter(service_schedule_link::Column::ServiceRecordId.eq(record.id))
        .all(&state.db)
        .await?;
    let schedule_item_ids = links.into_iter().map(|l| l.schedule_item_id).collect();

    Ok(Json(ServiceRecordWithLinks {
        record,
        schedule_item_ids,
    }))
}

pub async fn create(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(input): Json<CreateServiceRecord>,
) -> Result<Json<ServiceRecordWithLinks>> {
    vehicle::Entity::find_by_id(vehicle_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Vehicle {vehicle_id} not found")))?;

    let txn = state.db.begin().await?;

    let record = service_record::ActiveModel {
        vehicle_id: Set(vehicle_id),
        service_date: Set(input.service_date.clone()),
        mileage: Set(input.mileage),
        description: Set(input.description),
        parts_cost_cents: Set(input.parts_cost_cents),
        parts_cost_currency: Set(input.parts_cost_currency),
        labor_cost_cents: Set(input.labor_cost_cents),
        labor_cost_currency: Set(input.labor_cost_currency),
        total_cost_cents: Set(input.total_cost_cents),
        total_cost_currency: Set(input.total_cost_currency),
        shop_name: Set(input.shop_name),
        notes: Set(input.notes),
        ..Default::default()
    };
    let record = record.insert(&txn).await?;

    let schedule_item_ids = input.schedule_item_ids.unwrap_or_default();
    for item_id in &schedule_item_ids {
        let link = service_schedule_link::ActiveModel {
            service_record_id: Set(record.id),
            schedule_item_id: Set(*item_id),
        };
        link.insert(&txn).await?;
    }

    // Also create a mileage log entry if mileage was provided
    if let Some(miles) = record.mileage {
        let mileage_entry = mileage_log::ActiveModel {
            vehicle_id: Set(vehicle_id),
            mileage: Set(miles),
            recorded_at: Set(record.service_date.clone()),
            source: Set(Some("service".to_string())),
            notes: Set(None),
            ..Default::default()
        };
        mileage_entry.insert(&txn).await?;
    }

    txn.commit().await?;

    Ok(Json(ServiceRecordWithLinks {
        record,
        schedule_item_ids,
    }))
}

pub async fn update(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    Json(input): Json<UpdateServiceRecord>,
) -> Result<Json<ServiceRecordWithLinks>> {
    let existing = service_record::Entity::find_by_id(id)
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Service record {id} not found")))?;

    let txn = state.db.begin().await?;

    let mut active: service_record::ActiveModel = existing.into();

    if let Some(v) = input.service_date { active.service_date = Set(v); }
    if let Some(v) = input.mileage { active.mileage = Set(v); }
    if let Some(v) = input.description { active.description = Set(v); }
    if let Some(v) = input.parts_cost_cents { active.parts_cost_cents = Set(v); }
    if let Some(v) = input.parts_cost_currency { active.parts_cost_currency = Set(v); }
    if let Some(v) = input.labor_cost_cents { active.labor_cost_cents = Set(v); }
    if let Some(v) = input.labor_cost_currency { active.labor_cost_currency = Set(v); }
    if let Some(v) = input.total_cost_cents { active.total_cost_cents = Set(v); }
    if let Some(v) = input.total_cost_currency { active.total_cost_currency = Set(v); }
    if let Some(v) = input.shop_name { active.shop_name = Set(v); }
    if let Some(v) = input.notes { active.notes = Set(v); }

    let record = active.update(&txn).await?;

    let schedule_item_ids = if let Some(item_ids) = input.schedule_item_ids {
        service_schedule_link::Entity::delete_many()
            .filter(service_schedule_link::Column::ServiceRecordId.eq(record.id))
            .exec(&txn)
            .await?;

        for item_id in &item_ids {
            let link = service_schedule_link::ActiveModel {
                service_record_id: Set(record.id),
                schedule_item_id: Set(*item_id),
            };
            link.insert(&txn).await?;
        }
        item_ids
    } else {
        let links = service_schedule_link::Entity::find()
            .filter(service_schedule_link::Column::ServiceRecordId.eq(record.id))
            .all(&txn)
            .await?;
        links.into_iter().map(|l| l.schedule_item_id).collect()
    };

    txn.commit().await?;

    Ok(Json(ServiceRecordWithLinks {
        record,
        schedule_item_ids,
    }))
}
