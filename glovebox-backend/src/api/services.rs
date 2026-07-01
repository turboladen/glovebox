use axum::{
    Json,
    extract::{Path, State},
};
use sea_orm::*;
use serde::{Deserialize, Serialize};

use crate::AppState;
use glovebox_shared::entities::{
    mileage_log, part, service_record, service_record_line_item, service_schedule_link,
};

use super::{error::ApiError, require_vehicle, serde_helpers::deserialize_optional};

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct CreateLineItem {
    pub description: String,
    pub category: Option<String>,
    pub quantity: Option<f64>,
    pub unit_cost_cents: Option<i32>,
    pub cost_cents: Option<i32>,
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
    pub schedule_item_ids: Option<Vec<i32>>,
    pub part_ids: Option<Vec<i32>>,
    pub line_items: Option<Vec<CreateLineItem>>,
}

#[derive(Serialize)]
pub struct ServiceRecordWithLinks {
    #[serde(flatten)]
    pub record: service_record::Model,
    pub schedule_item_ids: Vec<i32>,
    pub part_ids: Vec<i32>,
    pub line_items: Vec<service_record_line_item::Model>,
}

/// Load schedule link IDs, part IDs, and line items for a single service record.
async fn load_service_links(
    db: &impl ConnectionTrait,
    record_id: i32,
) -> Result<(Vec<i32>, Vec<i32>, Vec<service_record_line_item::Model>)> {
    let links = service_schedule_link::Entity::find()
        .filter(service_schedule_link::Column::ServiceRecordId.eq(record_id))
        .all(db)
        .await?;
    let schedule_item_ids = links.into_iter().map(|l| l.schedule_item_id).collect();

    let parts = part::Entity::find()
        .filter(part::Column::InstalledServiceId.eq(record_id))
        .all(db)
        .await?;
    let part_ids = parts.into_iter().map(|p| p.id).collect();

    let line_items = service_record_line_item::Entity::find()
        .filter(service_record_line_item::Column::ServiceRecordId.eq(record_id))
        .order_by_asc(service_record_line_item::Column::Id)
        .all(db)
        .await?;

    Ok((schedule_item_ids, part_ids, line_items))
}

/// Insert line items for a service record within a transaction.
async fn insert_line_items(
    txn: &impl ConnectionTrait,
    record_id: i32,
    items: Vec<CreateLineItem>,
) -> Result<()> {
    for item in items {
        let li = service_record_line_item::ActiveModel {
            service_record_id: Set(record_id),
            description: Set(item.description),
            category: Set(item.category),
            quantity: Set(item.quantity),
            unit_cost_cents: Set(item.unit_cost_cents),
            cost_cents: Set(item.cost_cents),
            ..Default::default()
        };
        li.insert(txn).await?;
    }
    Ok(())
}

pub async fn list(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<Vec<ServiceRecordWithLinks>>> {
    require_vehicle(&state.db, vehicle_id).await?;

    let records = service_record::Entity::find()
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(service_record::Column::ServiceDate)
        .all(&state.db)
        .await?;

    // Batch-load all schedule links, parts, and line items for these records (avoids N+1)
    let record_ids: Vec<i32> = records.iter().map(|r| r.id).collect();

    let all_links = if record_ids.is_empty() {
        vec![]
    } else {
        service_schedule_link::Entity::find()
            .filter(service_schedule_link::Column::ServiceRecordId.is_in(record_ids.clone()))
            .all(&state.db)
            .await?
    };

    let all_parts = if record_ids.is_empty() {
        vec![]
    } else {
        part::Entity::find()
            .filter(part::Column::InstalledServiceId.is_in(record_ids.clone()))
            .all(&state.db)
            .await?
    };

    let all_line_items = if record_ids.is_empty() {
        vec![]
    } else {
        service_record_line_item::Entity::find()
            .filter(service_record_line_item::Column::ServiceRecordId.is_in(record_ids))
            .order_by_asc(service_record_line_item::Column::Id)
            .all(&state.db)
            .await?
    };

    let results = records
        .into_iter()
        .map(|record| {
            let schedule_item_ids = all_links
                .iter()
                .filter(|l| l.service_record_id == record.id)
                .map(|l| l.schedule_item_id)
                .collect();
            let part_ids = all_parts
                .iter()
                .filter(|p| p.installed_service_id == Some(record.id))
                .map(|p| p.id)
                .collect();
            let line_items = all_line_items
                .iter()
                .filter(|li| li.service_record_id == record.id)
                .cloned()
                .collect();
            ServiceRecordWithLinks {
                record,
                schedule_item_ids,
                part_ids,
                line_items,
            }
        })
        .collect();

    Ok(Json(results))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<ServiceRecordWithLinks>> {
    require_vehicle(&state.db, vehicle_id).await?;

    let record = service_record::Entity::find_by_id(id)
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Service record {id} not found")))?;

    let (schedule_item_ids, part_ids, line_items) =
        load_service_links(&state.db, record.id).await?;

    Ok(Json(ServiceRecordWithLinks {
        record,
        schedule_item_ids,
        part_ids,
        line_items,
    }))
}

pub async fn create(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(input): Json<CreateServiceRecord>,
) -> Result<Json<ServiceRecordWithLinks>> {
    require_vehicle(&state.db, vehicle_id).await?;

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
        shop_id: Set(input.shop_id),
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

    // Mark linked parts as installed
    let part_ids = input.part_ids.unwrap_or_default();
    for part_id in &part_ids {
        let existing_part = part::Entity::find_by_id(*part_id)
            .filter(part::Column::VehicleId.eq(vehicle_id))
            .one(&txn)
            .await?
            .ok_or_else(|| ApiError::NotFound(format!("Part {part_id} not found")))?;

        let mut active_part: part::ActiveModel = existing_part.into();
        active_part.status = Set("installed".to_string());
        active_part.installed_service_id = Set(Some(record.id));
        active_part.installed_date = Set(Some(input.service_date.clone()));
        if let Some(miles) = record.mileage {
            active_part.installed_odometer = Set(Some(miles));
        }
        active_part.update(&txn).await?;
    }

    // Insert line items
    let line_items_input = input.line_items.unwrap_or_default();
    insert_line_items(&txn, record.id, line_items_input).await?;

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

    // Load back the inserted line items to return
    let line_items = service_record_line_item::Entity::find()
        .filter(service_record_line_item::Column::ServiceRecordId.eq(record.id))
        .order_by_asc(service_record_line_item::Column::Id)
        .all(&txn)
        .await?;

    txn.commit().await?;

    Ok(Json(ServiceRecordWithLinks {
        record,
        schedule_item_ids,
        part_ids,
        line_items,
    }))
}

#[allow(clippy::too_many_lines)]
pub async fn update(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    Json(input): Json<UpdateServiceRecord>,
) -> Result<Json<ServiceRecordWithLinks>> {
    require_vehicle(&state.db, vehicle_id).await?;

    let existing = service_record::Entity::find_by_id(id)
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Service record {id} not found")))?;

    let txn = state.db.begin().await?;

    let mut active: service_record::ActiveModel = existing.into();

    if let Some(v) = input.service_date {
        active.service_date = Set(v);
    }
    if let Some(v) = input.mileage {
        active.mileage = Set(v);
    }
    if let Some(v) = input.description {
        active.description = Set(v);
    }
    if let Some(v) = input.parts_cost_cents {
        active.parts_cost_cents = Set(v);
    }
    if let Some(v) = input.parts_cost_currency {
        active.parts_cost_currency = Set(v);
    }
    if let Some(v) = input.labor_cost_cents {
        active.labor_cost_cents = Set(v);
    }
    if let Some(v) = input.labor_cost_currency {
        active.labor_cost_currency = Set(v);
    }
    if let Some(v) = input.total_cost_cents {
        active.total_cost_cents = Set(v);
    }
    if let Some(v) = input.total_cost_currency {
        active.total_cost_currency = Set(v);
    }
    if let Some(v) = input.shop_name {
        active.shop_name = Set(v);
    }
    if let Some(v) = input.shop_id {
        active.shop_id = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }

    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

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

    // Handle part linking on update
    let part_ids = if let Some(new_part_ids) = input.part_ids {
        // Unlink previously linked parts
        let old_parts = part::Entity::find()
            .filter(part::Column::InstalledServiceId.eq(record.id))
            .all(&txn)
            .await?;
        for old_part in old_parts {
            if !new_part_ids.contains(&old_part.id) {
                let mut active_part: part::ActiveModel = old_part.clone().into();
                // Only revert to purchased if the part was installed (not replaced/returned)
                if old_part.status == "installed" {
                    active_part.status = Set("purchased".to_string());
                }
                active_part.installed_service_id = Set(None);
                active_part.installed_date = Set(None);
                active_part.installed_odometer = Set(None);
                active_part.update(&txn).await?;
            }
        }
        // Link new parts
        for part_id in &new_part_ids {
            let existing_part = part::Entity::find_by_id(*part_id)
                .filter(part::Column::VehicleId.eq(vehicle_id))
                .one(&txn)
                .await?
                .ok_or_else(|| ApiError::NotFound(format!("Part {part_id} not found")))?;

            let mut active_part: part::ActiveModel = existing_part.into();
            active_part.status = Set("installed".to_string());
            active_part.installed_service_id = Set(Some(record.id));
            active_part.installed_date = Set(Some(record.service_date.clone()));
            active_part.installed_odometer = Set(record.mileage);
            active_part.update(&txn).await?;
        }
        new_part_ids
    } else {
        let parts = part::Entity::find()
            .filter(part::Column::InstalledServiceId.eq(record.id))
            .all(&txn)
            .await?;
        parts.into_iter().map(|p| p.id).collect()
    };

    // Handle line items: replace-all semantics when provided
    if let Some(new_line_items) = input.line_items {
        service_record_line_item::Entity::delete_many()
            .filter(service_record_line_item::Column::ServiceRecordId.eq(record.id))
            .exec(&txn)
            .await?;
        insert_line_items(&txn, record.id, new_line_items).await?;
    }

    let line_items = service_record_line_item::Entity::find()
        .filter(service_record_line_item::Column::ServiceRecordId.eq(record.id))
        .order_by_asc(service_record_line_item::Column::Id)
        .all(&txn)
        .await?;

    txn.commit().await?;

    Ok(Json(ServiceRecordWithLinks {
        record,
        schedule_item_ids,
        part_ids,
        line_items,
    }))
}

pub async fn delete(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<serde_json::Value>> {
    require_vehicle(&state.db, vehicle_id).await?;

    let existing = service_record::Entity::find_by_id(id)
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Service record {id} not found")))?;

    let txn = state.db.begin().await?;

    // Unlink parts installed during this service (revert to purchased)
    let linked_parts = part::Entity::find()
        .filter(part::Column::InstalledServiceId.eq(existing.id))
        .all(&txn)
        .await?;
    for p in linked_parts {
        let mut active_part: part::ActiveModel = p.into();
        active_part.status = Set("purchased".to_string());
        active_part.installed_service_id = Set(None);
        active_part.installed_date = Set(None);
        active_part.installed_odometer = Set(None);
        active_part.update(&txn).await?;
    }

    // Remove schedule links and line items
    service_schedule_link::Entity::delete_many()
        .filter(service_schedule_link::Column::ServiceRecordId.eq(existing.id))
        .exec(&txn)
        .await?;
    service_record_line_item::Entity::delete_many()
        .filter(service_record_line_item::Column::ServiceRecordId.eq(existing.id))
        .exec(&txn)
        .await?;

    // Delete the service record
    existing.delete(&txn).await?;

    txn.commit().await?;

    Ok(Json(serde_json::json!({ "deleted": id })))
}
