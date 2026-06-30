use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use sea_orm::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{
    AppState,
    entities::{maintenance_schedule_item, model_template, vehicle},
};

use super::{error::ApiError, serde_helpers::deserialize_optional};

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct CreateScheduleItem {
    pub platform_id: Option<i32>,
    pub model_template_id: Option<i32>,
    pub vehicle_id: Option<i32>,
    pub overrides_item_id: Option<i32>,
    pub name: String,
    pub description: Option<String>,
    pub interval_miles: Option<i32>,
    pub interval_months: Option<i32>,
    pub warning_miles: Option<i32>,
    pub warning_days: Option<i32>,
    pub enabled: Option<bool>,
    pub source: Option<String>,
    pub notes: Option<String>,
    pub is_factory_recommended: Option<bool>,
    pub labor_categories: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateScheduleItem {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub interval_miles: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub interval_months: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub warning_miles: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub warning_days: Option<Option<i32>>,
    pub enabled: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub source: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub notes: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub is_factory_recommended: Option<Option<bool>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub labor_categories: Option<Option<String>>,
}

#[derive(Serialize)]
pub struct ResolvedScheduleItem {
    pub effective_item: maintenance_schedule_item::Model,
    pub inherited_from: Option<String>,
}

#[derive(Deserialize)]
#[allow(clippy::struct_field_names)]
pub struct ListQuery {
    pub platform_id: Option<i32>,
    pub model_template_id: Option<i32>,
    pub vehicle_id: Option<i32>,
}

/// List raw schedule items, optionally filtered by owner
async fn list(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<maintenance_schedule_item::Model>>> {
    let mut select = maintenance_schedule_item::Entity::find();

    if let Some(pid) = query.platform_id {
        select = select.filter(maintenance_schedule_item::Column::PlatformId.eq(pid));
    }
    if let Some(mtid) = query.model_template_id {
        select = select.filter(maintenance_schedule_item::Column::ModelTemplateId.eq(mtid));
    }
    if let Some(vid) = query.vehicle_id {
        select = select.filter(maintenance_schedule_item::Column::VehicleId.eq(vid));
    }

    let items = select.all(&state.db).await?;
    Ok(Json(items))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<maintenance_schedule_item::Model>> {
    maintenance_schedule_item::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("Schedule item {id} not found")))
}

async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateScheduleItem>,
) -> Result<Json<maintenance_schedule_item::Model>> {
    // Validate exactly one owner is set
    let owner_count = [
        input.platform_id.is_some(),
        input.model_template_id.is_some(),
        input.vehicle_id.is_some(),
    ]
    .iter()
    .filter(|&&b| b)
    .count();

    if owner_count != 1 {
        return Err(ApiError::BadRequest(
            "Exactly one of platform_id, model_template_id, or vehicle_id must be set".to_string(),
        ));
    }

    let model = maintenance_schedule_item::ActiveModel {
        platform_id: Set(input.platform_id),
        model_template_id: Set(input.model_template_id),
        vehicle_id: Set(input.vehicle_id),
        overrides_item_id: Set(input.overrides_item_id),
        name: Set(input.name),
        description: Set(input.description),
        interval_miles: Set(input.interval_miles),
        interval_months: Set(input.interval_months),
        warning_miles: Set(input.warning_miles),
        warning_days: Set(input.warning_days),
        enabled: Set(input.enabled.unwrap_or(true)),
        source: Set(input.source),
        notes: Set(input.notes),
        is_factory_recommended: Set(input.is_factory_recommended),
        labor_categories: Set(input.labor_categories),
        ..Default::default()
    };
    let result = model.insert(&state.db).await?;
    Ok(Json(result))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(input): Json<UpdateScheduleItem>,
) -> Result<Json<maintenance_schedule_item::Model>> {
    let existing = maintenance_schedule_item::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Schedule item {id} not found")))?;

    let mut active: maintenance_schedule_item::ActiveModel = existing.into();

    if let Some(v) = input.name {
        active.name = Set(v);
    }
    if let Some(v) = input.description {
        active.description = Set(v);
    }
    if let Some(v) = input.interval_miles {
        active.interval_miles = Set(v);
    }
    if let Some(v) = input.interval_months {
        active.interval_months = Set(v);
    }
    if let Some(v) = input.warning_miles {
        active.warning_miles = Set(v);
    }
    if let Some(v) = input.warning_days {
        active.warning_days = Set(v);
    }
    if let Some(v) = input.enabled {
        active.enabled = Set(v);
    }
    if let Some(v) = input.source {
        active.source = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }
    if let Some(v) = input.is_factory_recommended {
        active.is_factory_recommended = Set(v);
    }
    if let Some(v) = input.labor_categories {
        active.labor_categories = Set(v);
    }

    let result = active.update(&state.db).await?;
    Ok(Json(result))
}

async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>> {
    let result = maintenance_schedule_item::Entity::delete_by_id(id)
        .exec(&state.db)
        .await?;

    if result.rows_affected == 0 {
        return Err(ApiError::NotFound(format!("Schedule item {id} not found")));
    }
    Ok(Json(serde_json::json!({"deleted": id})))
}

/// Resolve the effective maintenance schedule for a vehicle.
/// Implements the 3-level inheritance: Platform → Model Template → Vehicle.
pub async fn resolve(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<Vec<ResolvedScheduleItem>>> {
    let v = vehicle::Entity::find_by_id(vehicle_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Vehicle {vehicle_id} not found")))?;

    // Layer 1: Platform items (via model template → platform)
    let mut schedule: HashMap<String, ResolvedScheduleItem> = HashMap::new();

    if let Some(mt_id) = v.model_template_id {
        let mt = model_template::Entity::find_by_id(mt_id)
            .one(&state.db)
            .await?;

        if let Some(mt) = &mt {
            if let Some(platform_id) = mt.platform_id {
                let platform_items = maintenance_schedule_item::Entity::find()
                    .filter(maintenance_schedule_item::Column::PlatformId.eq(platform_id))
                    .all(&state.db)
                    .await?;

                for item in platform_items {
                    let name = item.name.clone();
                    schedule.insert(
                        name,
                        ResolvedScheduleItem {
                            effective_item: item,
                            inherited_from: Some("platform".to_string()),
                        },
                    );
                }
            }

            // Layer 2: Model template items override platform items by name
            let template_items = maintenance_schedule_item::Entity::find()
                .filter(maintenance_schedule_item::Column::ModelTemplateId.eq(mt_id))
                .all(&state.db)
                .await?;

            for item in template_items {
                let name = item.name.clone();
                schedule.insert(
                    name,
                    ResolvedScheduleItem {
                        effective_item: item,
                        inherited_from: Some("model_template".to_string()),
                    },
                );
            }
        }
    }

    // Layer 3: Vehicle-level items override everything
    let vehicle_items = maintenance_schedule_item::Entity::find()
        .filter(maintenance_schedule_item::Column::VehicleId.eq(vehicle_id))
        .all(&state.db)
        .await?;

    for item in vehicle_items {
        let name = item.name.clone();
        schedule.insert(
            name,
            ResolvedScheduleItem {
                effective_item: item,
                inherited_from: None,
            },
        );
    }

    // Filter out disabled items and sort by name for stable output
    let mut result: Vec<ResolvedScheduleItem> = schedule
        .into_values()
        .filter(|r| r.effective_item.enabled)
        .collect();
    result.sort_by(|a, b| a.effective_item.name.cmp(&b.effective_item.name));

    Ok(Json(result))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).put(update).delete(delete))
}
