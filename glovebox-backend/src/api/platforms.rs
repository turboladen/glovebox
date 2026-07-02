use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::{
    entities::platform,
    inputs::platform::{NewPlatform, UpdatePlatform as UpdatePlatformInput},
    services::platform as svc,
};

use super::{error::ApiError, serde_helpers::deserialize_optional};

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
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub website_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub api_base_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub notes: Option<Option<String>>,
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<platform::Model>>> {
    Ok(Json(svc::list(&state.db).await?))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<platform::Model>> {
    Ok(Json(svc::get(&state.db, id).await?))
}

async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreatePlatform>,
) -> Result<Json<platform::Model>> {
    let created = svc::create(
        &state.db,
        NewPlatform {
            name: input.name,
            website_url: input.website_url,
            api_base_url: input.api_base_url,
            notes: input.notes,
        },
    )
    .await?;
    Ok(Json(created))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(input): Json<UpdatePlatform>,
) -> Result<Json<platform::Model>> {
    let updated = svc::update(
        &state.db,
        id,
        UpdatePlatformInput {
            name: input.name,
            website_url: input.website_url,
            api_base_url: input.api_base_url,
            notes: input.notes,
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
