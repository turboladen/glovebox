use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use sea_orm::*;
use serde::Deserialize;

use crate::entities::platform;
use crate::AppState;

use super::error::ApiError;

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct CreatePlatform {
    pub name: String,
    pub website_url: Option<String>,
    pub api_base_url: Option<String>,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdatePlatform {
    pub name: Option<String>,
    pub website_url: Option<Option<String>>,
    pub api_base_url: Option<Option<String>>,
    pub notes: Option<Option<String>>,
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<platform::Model>>> {
    let items = platform::Entity::find().all(&state.db).await?;
    Ok(Json(items))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<platform::Model>> {
    platform::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("Platform {id} not found")))
}

async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreatePlatform>,
) -> Result<Json<platform::Model>> {
    let model = platform::ActiveModel {
        name: Set(input.name),
        website_url: Set(input.website_url),
        api_base_url: Set(input.api_base_url),
        notes: Set(input.notes),
        ..Default::default()
    };
    let result = model.insert(&state.db).await?;
    Ok(Json(result))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(input): Json<UpdatePlatform>,
) -> Result<Json<platform::Model>> {
    let existing = platform::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Platform {id} not found")))?;

    let mut active: platform::ActiveModel = existing.into();

    if let Some(v) = input.name { active.name = Set(v); }
    if let Some(v) = input.website_url { active.website_url = Set(v); }
    if let Some(v) = input.api_base_url { active.api_base_url = Set(v); }
    if let Some(v) = input.notes { active.notes = Set(v); }

    let result = active.update(&state.db).await?;
    Ok(Json(result))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).put(update))
}
