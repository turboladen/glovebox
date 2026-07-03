use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::{
    entities::maintenance_schedule_item,
    inputs::schedule::{
        NewScheduleItem, ScheduleFilter, UpdateScheduleItem as UpdateScheduleItemInput,
    },
    services::schedule::{self as svc, ResolvedScheduleItem},
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
    pub est_cost_cents: Option<i32>,
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
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub est_cost_cents: Option<Option<i32>>,
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
    let items = svc::list(
        &state.db,
        ScheduleFilter {
            platform_id: query.platform_id,
            model_template_id: query.model_template_id,
            vehicle_id: query.vehicle_id,
        },
    )
    .await?;
    Ok(Json(items))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<maintenance_schedule_item::Model>> {
    Ok(Json(svc::get(&state.db, id).await?))
}

async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateScheduleItem>,
) -> Result<Json<maintenance_schedule_item::Model>> {
    let created = svc::create(
        &state.db,
        NewScheduleItem {
            platform_id: input.platform_id,
            model_template_id: input.model_template_id,
            vehicle_id: input.vehicle_id,
            overrides_item_id: input.overrides_item_id,
            name: input.name,
            description: input.description,
            interval_miles: input.interval_miles,
            interval_months: input.interval_months,
            warning_miles: input.warning_miles,
            warning_days: input.warning_days,
            enabled: input.enabled,
            source: input.source,
            notes: input.notes,
            is_factory_recommended: input.is_factory_recommended,
            labor_categories: input.labor_categories,
            est_cost_cents: input.est_cost_cents,
        },
    )
    .await?;
    Ok(Json(created))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(input): Json<UpdateScheduleItem>,
) -> Result<Json<maintenance_schedule_item::Model>> {
    let updated = svc::update(
        &state.db,
        id,
        UpdateScheduleItemInput {
            name: input.name,
            description: input.description,
            interval_miles: input.interval_miles,
            interval_months: input.interval_months,
            warning_miles: input.warning_miles,
            warning_days: input.warning_days,
            enabled: input.enabled,
            source: input.source,
            notes: input.notes,
            is_factory_recommended: input.is_factory_recommended,
            labor_categories: input.labor_categories,
            est_cost_cents: input.est_cost_cents,
        },
    )
    .await?;
    Ok(Json(updated))
}

async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>> {
    svc::delete(&state.db, id).await?;
    Ok(Json(serde_json::json!({"deleted": id})))
}

#[derive(Deserialize, Default)]
pub struct DismissScheduleItem {
    pub reason: Option<String>,
}

/// Dismiss a schedule item for one vehicle (vehicle-level `enabled = false`
/// override). Thin: ownership/inheritance mechanics live in
/// `schedule::dismiss_for_vehicle`. The JSON body (`{"reason": …}`) is
/// optional.
pub async fn dismiss(
    State(state): State<AppState>,
    Path((vehicle_id, item_id)): Path<(i32, i32)>,
    body: Option<Json<DismissScheduleItem>>,
) -> Result<Json<maintenance_schedule_item::Model>> {
    let reason = body.and_then(|Json(b)| b.reason);
    Ok(Json(
        svc::dismiss_for_vehicle(&state.db, vehicle_id, item_id, reason).await?,
    ))
}

/// Reverse a dismissal: re-enable the vehicle-owned override row.
pub async fn undismiss(
    State(state): State<AppState>,
    Path((vehicle_id, item_id)): Path<(i32, i32)>,
) -> Result<Json<maintenance_schedule_item::Model>> {
    Ok(Json(
        svc::undismiss_for_vehicle(&state.db, vehicle_id, item_id).await?,
    ))
}

/// Resolve the effective maintenance schedule for a vehicle.
/// Implements the 3-level inheritance: Platform → Model Template → Vehicle.
pub async fn resolve(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<Vec<ResolvedScheduleItem>>> {
    Ok(Json(svc::resolve(&state.db, vehicle_id).await?))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).put(update).delete(delete))
}
