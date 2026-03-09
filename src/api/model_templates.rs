use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use sea_orm::*;
use serde::Deserialize;

use crate::entities::model_template;
use crate::AppState;

use super::error::ApiError;
use super::serde_helpers::deserialize_optional;

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct CreateModelTemplate {
    pub platform_id: Option<i32>,
    pub platform_ref: Option<String>,
    pub year: Option<i32>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub trim_level: Option<String>,
    pub body_style: Option<String>,
    pub engine: Option<String>,
    pub transmission: Option<String>,
    pub drivetrain: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateModelTemplate {
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub platform_id: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub platform_ref: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub year: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub make: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub model: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub trim_level: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub body_style: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub engine: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub transmission: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub drivetrain: Option<Option<String>>,
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<model_template::Model>>> {
    let items = model_template::Entity::find().all(&state.db).await?;
    Ok(Json(items))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<model_template::Model>> {
    model_template::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("Model template {id} not found")))
}

async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateModelTemplate>,
) -> Result<Json<model_template::Model>> {
    let model = model_template::ActiveModel {
        platform_id: Set(input.platform_id),
        platform_ref: Set(input.platform_ref),
        year: Set(input.year),
        make: Set(input.make),
        model: Set(input.model),
        trim_level: Set(input.trim_level),
        body_style: Set(input.body_style),
        engine: Set(input.engine),
        transmission: Set(input.transmission),
        drivetrain: Set(input.drivetrain),
        ..Default::default()
    };
    let result = model.insert(&state.db).await?;
    Ok(Json(result))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(input): Json<UpdateModelTemplate>,
) -> Result<Json<model_template::Model>> {
    let existing = model_template::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Model template {id} not found")))?;

    let mut active: model_template::ActiveModel = existing.into();

    if let Some(v) = input.platform_id {
        active.platform_id = Set(v);
    }
    if let Some(v) = input.platform_ref {
        active.platform_ref = Set(v);
    }
    if let Some(v) = input.year {
        active.year = Set(v);
    }
    if let Some(v) = input.make {
        active.make = Set(v);
    }
    if let Some(v) = input.model {
        active.model = Set(v);
    }
    if let Some(v) = input.trim_level {
        active.trim_level = Set(v);
    }
    if let Some(v) = input.body_style {
        active.body_style = Set(v);
    }
    if let Some(v) = input.engine {
        active.engine = Set(v);
    }
    if let Some(v) = input.transmission {
        active.transmission = Set(v);
    }
    if let Some(v) = input.drivetrain {
        active.drivetrain = Set(v);
    }

    let result = active.update(&state.db).await?;
    Ok(Json(result))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).put(update))
}
