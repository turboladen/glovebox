use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::{
    entities::model_template,
    inputs::model_template::{NewModelTemplate, UpdateModelTemplate as UpdateModelTemplateInput},
    services::model_template as svc,
};

use super::{error::ApiError, serde_helpers::deserialize_optional};

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
    Ok(Json(svc::list(&state.db).await?))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<model_template::Model>> {
    Ok(Json(svc::get(&state.db, id).await?))
}

async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateModelTemplate>,
) -> Result<Json<model_template::Model>> {
    let created = svc::create(
        &state.db,
        NewModelTemplate {
            platform_id: input.platform_id,
            platform_ref: input.platform_ref,
            year: input.year,
            make: input.make,
            model: input.model,
            trim_level: input.trim_level,
            body_style: input.body_style,
            engine: input.engine,
            transmission: input.transmission,
            drivetrain: input.drivetrain,
        },
    )
    .await?;
    Ok(Json(created))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(input): Json<UpdateModelTemplate>,
) -> Result<Json<model_template::Model>> {
    let updated = svc::update(
        &state.db,
        id,
        UpdateModelTemplateInput {
            platform_id: input.platform_id,
            platform_ref: input.platform_ref,
            year: input.year,
            make: input.make,
            model: input.model,
            trim_level: input.trim_level,
            body_style: input.body_style,
            engine: input.engine,
            transmission: input.transmission,
            drivetrain: input.drivetrain,
        },
    )
    .await?;
    Ok(Json(updated))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).put(update))
}
