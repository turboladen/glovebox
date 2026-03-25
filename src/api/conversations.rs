use axum::extract::{Path, State};
use axum::Json;
use sea_orm::*;
use serde::Deserialize;

use crate::api::error::ApiError;
use crate::api::require_vehicle;
use crate::entities::{chat_message, conversation};
use crate::AppState;

type Result<T> = std::result::Result<T, ApiError>;

pub async fn list(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<Vec<conversation::Model>>> {
    require_vehicle(&state.db, vehicle_id).await?;

    let convos = conversation::Entity::find()
        .filter(conversation::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(conversation::Column::UpdatedAt)
        .all(&state.db)
        .await?;

    Ok(Json(convos))
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
    require_vehicle(&state.db, vehicle_id).await?;

    let model = conversation::ActiveModel {
        vehicle_id: Set(Some(vehicle_id)),
        title: Set(input.title.unwrap_or_else(|| "New Chat".to_string())),
        ..Default::default()
    };

    let result = model.insert(&state.db).await?;
    Ok(Json(result))
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
    require_vehicle(&state.db, vehicle_id).await?;

    let existing = conversation::Entity::find_by_id(id)
        .filter(conversation::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Conversation {id} not found")))?;

    let mut active: conversation::ActiveModel = existing.into();
    active.title = Set(input.title);
    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

    let result = active.update(&state.db).await?;
    Ok(Json(result))
}

pub async fn delete(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<serde_json::Value>> {
    require_vehicle(&state.db, vehicle_id).await?;

    // Delete associated chat messages first (no FK cascade on ALTER TABLE ADD COLUMN in SQLite)
    chat_message::Entity::delete_many()
        .filter(chat_message::Column::ConversationId.eq(id))
        .exec(&state.db)
        .await?;

    let result = conversation::Entity::delete_many()
        .filter(conversation::Column::Id.eq(id))
        .filter(conversation::Column::VehicleId.eq(vehicle_id))
        .exec(&state.db)
        .await?;

    if result.rows_affected == 0 {
        return Err(ApiError::NotFound(format!("Conversation {id} not found")));
    }

    Ok(Json(serde_json::json!({ "deleted": id })))
}

pub async fn messages(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<Vec<chat_message::Model>>> {
    require_vehicle(&state.db, vehicle_id).await?;

    // Verify conversation belongs to this vehicle
    conversation::Entity::find_by_id(id)
        .filter(conversation::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Conversation {id} not found")))?;

    let msgs = chat_message::Entity::find()
        .filter(chat_message::Column::ConversationId.eq(id))
        .order_by_asc(chat_message::Column::CreatedAt)
        .all(&state.db)
        .await?;

    Ok(Json(msgs))
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
    require_vehicle(&state.db, vehicle_id).await?;

    // Verify conversation belongs to vehicle
    conversation::Entity::find_by_id(id)
        .filter(conversation::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Conversation {id} not found")))?;

    if input.role != "user" && input.role != "assistant" {
        return Err(ApiError::BadRequest(
            "role must be 'user' or 'assistant'".into(),
        ));
    }

    let now = chrono::Utc::now()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    let msg = chat_message::ActiveModel {
        vehicle_id: Set(Some(vehicle_id)),
        conversation_id: Set(Some(id)),
        role: Set(input.role),
        content: Set(input.content),
        created_at: Set(now.clone()),
        ..Default::default()
    };
    let saved = msg.insert(&state.db).await?;

    // Touch conversation updated_at
    let mut convo_active = conversation::ActiveModel {
        id: Set(id),
        ..Default::default()
    };
    convo_active.updated_at = Set(now);
    convo_active.update(&state.db).await?;

    Ok(Json(saved))
}
