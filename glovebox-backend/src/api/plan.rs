//! Planning HTTP surface (2hea unit F): work items + visits, thin over
//! `glovebox_shared::services::{work_item, visit}`. Routes are flat in
//! `main.rs` (vehicle sub-resource convention).

use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::{
    entities::work_item,
    inputs::{
        visit::{CompleteVisit, NewVisit, UpdateVisit},
        work_item::{NewWorkItem, UpdateWorkItem},
    },
    services::{
        visit as visit_svc,
        visit::{CompletedVisit, VisitWithItems},
        work_item as work_item_svc,
    },
};

use super::{error::ApiError, serde_helpers::deserialize_optional};

type Result<T> = std::result::Result<T, ApiError>;

// ─── Work items ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ListWorkItemsQuery {
    /// Also include done/dropped history. Default false.
    #[serde(default)]
    pub include_done: bool,
}

#[derive(Deserialize)]
pub struct CreateWorkItem {
    pub title: String,
    pub notes: Option<String>,
    pub schedule_item_id: Option<i32>,
    pub research_finding_id: Option<i32>,
    pub incident_id: Option<i32>,
    pub build_id: Option<i32>,
    pub est_cost_cents: Option<i32>,
    pub visit_id: Option<i32>,
}

#[derive(Deserialize)]
pub struct UpdateWorkItemDto {
    pub title: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub notes: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub schedule_item_id: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub research_finding_id: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub incident_id: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub build_id: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub est_cost_cents: Option<Option<i32>>,
    pub status: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub visit_id: Option<Option<i32>>,
}

pub async fn list_work_items(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Query(q): Query<ListWorkItemsQuery>,
) -> Result<Json<Vec<work_item::Model>>> {
    Ok(Json(
        work_item_svc::list(&state.db, vehicle_id, q.include_done).await?,
    ))
}

pub async fn create_work_item(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(input): Json<CreateWorkItem>,
) -> Result<Json<work_item::Model>> {
    let created = work_item_svc::create(
        &state.db,
        vehicle_id,
        NewWorkItem {
            title: input.title,
            notes: input.notes,
            schedule_item_id: input.schedule_item_id,
            research_finding_id: input.research_finding_id,
            incident_id: input.incident_id,
            build_id: input.build_id,
            est_cost_cents: input.est_cost_cents,
            visit_id: input.visit_id,
        },
    )
    .await?;
    Ok(Json(created))
}

pub async fn update_work_item(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    Json(input): Json<UpdateWorkItemDto>,
) -> Result<Json<work_item::Model>> {
    let updated = work_item_svc::update(
        &state.db,
        vehicle_id,
        id,
        UpdateWorkItem {
            title: input.title,
            notes: input.notes,
            schedule_item_id: input.schedule_item_id,
            research_finding_id: input.research_finding_id,
            incident_id: input.incident_id,
            build_id: input.build_id,
            est_cost_cents: input.est_cost_cents,
            status: input.status,
            visit_id: input.visit_id,
        },
    )
    .await?;
    Ok(Json(updated))
}

pub async fn delete_work_item(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<serde_json::Value>> {
    work_item_svc::delete(&state.db, vehicle_id, id).await?;
    Ok(Json(serde_json::json!({ "deleted": id })))
}

// ─── Visits ─────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ListVisitsQuery {
    /// Also include completed/canceled history. Default false.
    #[serde(default)]
    pub include_closed: bool,
}

#[derive(Deserialize)]
pub struct CreateVisit {
    pub planned_date: Option<String>,
    pub shop_name: Option<String>,
    pub shop_id: Option<i32>,
    pub notes: Option<String>,
    pub work_item_ids: Option<Vec<i32>>,
}

#[derive(Deserialize)]
pub struct UpdateVisitDto {
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub planned_date: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub shop_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub shop_id: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub notes: Option<Option<String>>,
    pub status: Option<String>,
    pub work_item_ids: Option<Vec<i32>>,
}

#[derive(Deserialize)]
pub struct CompleteVisitDto {
    pub service_date: String,
    pub mileage: Option<i32>,
    pub description: Option<String>,
    pub total_cost_cents: Option<i32>,
    pub parts_cost_cents: Option<i32>,
    pub labor_cost_cents: Option<i32>,
    pub paid_by: Option<String>,
    pub payer_note: Option<String>,
    pub notes: Option<String>,
}

pub async fn list_visits(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Query(q): Query<ListVisitsQuery>,
) -> Result<Json<Vec<VisitWithItems>>> {
    Ok(Json(
        visit_svc::list(&state.db, vehicle_id, q.include_closed).await?,
    ))
}

pub async fn create_visit(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(input): Json<CreateVisit>,
) -> Result<Json<VisitWithItems>> {
    let created = visit_svc::create(
        &state.db,
        vehicle_id,
        NewVisit {
            planned_date: input.planned_date,
            shop_name: input.shop_name,
            shop_id: input.shop_id,
            notes: input.notes,
            work_item_ids: input.work_item_ids,
        },
    )
    .await?;
    Ok(Json(created))
}

pub async fn update_visit(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    Json(input): Json<UpdateVisitDto>,
) -> Result<Json<VisitWithItems>> {
    let updated = visit_svc::update(
        &state.db,
        vehicle_id,
        id,
        UpdateVisit {
            planned_date: input.planned_date,
            shop_name: input.shop_name,
            shop_id: input.shop_id,
            notes: input.notes,
            status: input.status,
            work_item_ids: input.work_item_ids,
        },
    )
    .await?;
    Ok(Json(updated))
}

pub async fn complete_visit(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    Json(input): Json<CompleteVisitDto>,
) -> Result<Json<CompletedVisit>> {
    let done = visit_svc::complete(
        &state.db,
        vehicle_id,
        id,
        CompleteVisit {
            service_date: input.service_date,
            mileage: input.mileage,
            description: input.description,
            total_cost_cents: input.total_cost_cents,
            parts_cost_cents: input.parts_cost_cents,
            labor_cost_cents: input.labor_cost_cents,
            paid_by: input.paid_by,
            payer_note: input.payer_note,
            notes: input.notes,
        },
    )
    .await?;
    Ok(Json(done))
}

pub async fn cancel_visit(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<VisitWithItems>> {
    Ok(Json(visit_svc::cancel(&state.db, vehicle_id, id).await?))
}

pub async fn delete_visit(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<serde_json::Value>> {
    visit_svc::delete(&state.db, vehicle_id, id).await?;
    Ok(Json(serde_json::json!({ "deleted": id })))
}
