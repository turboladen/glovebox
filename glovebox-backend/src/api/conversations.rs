use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;

use crate::{AppState, api::error::ApiError};
use glovebox_shared::{
    entities::{chat_message, conversation},
    services::{conversation as svc, vehicle as vehicle_svc},
};

type Result<T> = std::result::Result<T, ApiError>;

pub async fn list(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<Vec<conversation::Model>>> {
    vehicle_svc::require(&state.db, vehicle_id).await?;
    Ok(Json(svc::list(&state.db, vehicle_id).await?))
}

#[derive(Debug, Deserialize)]
pub struct CreateConversation {
    pub title: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(input): Json<CreateConversation>,
) -> Result<Json<conversation::Model>> {
    vehicle_svc::require(&state.db, vehicle_id).await?;
    Ok(Json(svc::create(&state.db, vehicle_id, input.title).await?))
}

#[derive(Debug, Deserialize)]
pub struct RenameConversation {
    pub title: String,
}

pub async fn rename(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    Json(input): Json<RenameConversation>,
) -> Result<Json<conversation::Model>> {
    vehicle_svc::require(&state.db, vehicle_id).await?;
    Ok(Json(
        svc::rename(&state.db, vehicle_id, id, input.title).await?,
    ))
}

pub async fn delete(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<serde_json::Value>> {
    vehicle_svc::require(&state.db, vehicle_id).await?;
    svc::delete(&state.db, vehicle_id, id).await?;
    Ok(Json(serde_json::json!({ "deleted": id })))
}

pub async fn messages(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<Vec<chat_message::Model>>> {
    vehicle_svc::require(&state.db, vehicle_id).await?;
    Ok(Json(svc::messages(&state.db, vehicle_id, id).await?))
}

#[derive(Debug, Deserialize)]
pub struct AddMessage {
    pub role: String,
    pub content: String,
}

pub async fn add_message(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    Json(input): Json<AddMessage>,
) -> Result<Json<chat_message::Model>> {
    vehicle_svc::require(&state.db, vehicle_id).await?;
    let saved = svc::add_message(&state.db, vehicle_id, id, input.role, input.content).await?;
    Ok(Json(saved))
}
