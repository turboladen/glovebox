use std::path::PathBuf;

use axum::extract::{Multipart, Path, Query, State};
use axum::Json;
use sea_orm::*;
use serde::Deserialize;

use crate::entities::{document, vehicle};
use crate::AppState;

use super::error::ApiError;

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct DocumentQuery {
    pub vehicle_id: Option<i32>,
    pub linked_entity_type: Option<String>,
    pub linked_entity_id: Option<i32>,
}

pub async fn list(
    State(state): State<AppState>,
    Query(query): Query<DocumentQuery>,
) -> Result<Json<Vec<document::Model>>> {
    let mut select = document::Entity::find();

    if let Some(vid) = query.vehicle_id {
        select = select.filter(document::Column::VehicleId.eq(vid));
    }
    if let Some(ref etype) = query.linked_entity_type {
        select = select.filter(document::Column::LinkedEntityType.eq(etype.as_str()));
    }
    if let Some(eid) = query.linked_entity_id {
        select = select.filter(document::Column::LinkedEntityId.eq(eid));
    }

    let docs = select
        .order_by_desc(document::Column::CreatedAt)
        .all(&state.db)
        .await?;
    Ok(Json(docs))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<document::Model>> {
    document::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("Document {id} not found")))
}

pub async fn upload(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<document::Model>> {
    let mut vehicle_id: Option<i32> = None;
    let mut title: Option<String> = None;
    let mut doc_type: Option<String> = None;
    let mut linked_entity_type: Option<String> = None;
    let mut linked_entity_id: Option<i32> = None;
    let mut notes: Option<String> = None;
    let mut file_name: Option<String> = None;
    let mut file_data: Option<Vec<u8>> = None;
    let mut mime_type: Option<String> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        ApiError::BadRequest(format!("Multipart error: {e}"))
    })? {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "vehicle_id" => {
                let text = field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?;
                vehicle_id = Some(text.parse().map_err(|_| ApiError::BadRequest("Invalid vehicle_id".into()))?);
            }
            "title" => {
                title = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?);
            }
            "doc_type" => {
                doc_type = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?);
            }
            "linked_entity_type" => {
                linked_entity_type = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?);
            }
            "linked_entity_id" => {
                let text = field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?;
                linked_entity_id = Some(text.parse().map_err(|_| ApiError::BadRequest("Invalid linked_entity_id".into()))?);
            }
            "notes" => {
                notes = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?);
            }
            "file" => {
                file_name = field.file_name().map(|s| s.to_string());
                mime_type = field.content_type().map(|s| s.to_string());
                file_data = Some(field.bytes().await.map_err(|e| ApiError::BadRequest(e.to_string()))?.to_vec());
            }
            _ => {}
        }
    }

    let file_data = file_data.ok_or_else(|| ApiError::BadRequest("No file provided".into()))?;
    let original_name = file_name.unwrap_or_else(|| "unnamed".into());
    let title = title.unwrap_or_else(|| original_name.clone());

    // Validate vehicle exists if provided
    if let Some(vid) = vehicle_id {
        vehicle::Entity::find_by_id(vid)
            .one(&state.db)
            .await?
            .ok_or_else(|| ApiError::NotFound(format!("Vehicle {vid} not found")))?;
    }

    // Build storage path: {files_dir}/{vehicle_id or "general"}/{doc_type or "other"}/
    let vid_dir = vehicle_id.map(|v| v.to_string()).unwrap_or_else(|| "general".into());
    let type_dir = doc_type.as_deref().unwrap_or("other");
    let dir: PathBuf = [&state.config.files_dir, &vid_dir, type_dir].iter().collect();
    tokio::fs::create_dir_all(&dir).await.map_err(|e| ApiError::Internal(e.to_string()))?;

    // Use timestamp + original name to avoid collisions
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let safe_name = sanitize_filename(&original_name);
    let stored_name = format!("{timestamp}_{safe_name}");
    let full_path = dir.join(&stored_name);

    tokio::fs::write(&full_path, &file_data).await.map_err(|e| ApiError::Internal(e.to_string()))?;

    // Store relative path (from files_dir root)
    let relative_path = format!("{vid_dir}/{type_dir}/{stored_name}");

    let model = document::ActiveModel {
        vehicle_id: Set(vehicle_id),
        title: Set(title),
        file_path: Set(relative_path),
        file_name: Set(original_name),
        mime_type: Set(mime_type),
        file_size_bytes: Set(Some(file_data.len() as i32)),
        doc_type: Set(doc_type),
        linked_entity_type: Set(linked_entity_type),
        linked_entity_id: Set(linked_entity_id),
        notes: Set(notes),
        ..Default::default()
    };
    let result = model.insert(&state.db).await?;
    Ok(Json(result))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>> {
    let doc = document::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Document {id} not found")))?;

    // Delete file from disk
    let full_path: PathBuf = [&state.config.files_dir, &doc.file_path].iter().collect();
    if full_path.exists() {
        tokio::fs::remove_file(&full_path).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    }

    document::Entity::delete_by_id(id).exec(&state.db).await?;
    Ok(Json(serde_json::json!({ "deleted": id })))
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect()
}
