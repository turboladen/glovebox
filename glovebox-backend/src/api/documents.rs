use axum::{
    Json,
    extract::{Multipart, Path, Query, State},
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::{
    entities::document,
    inputs::document::{DocumentFilter, DocumentSource, StoreDocument},
    services::document as svc,
};

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
    let docs = svc::list(
        &state.db,
        DocumentFilter {
            vehicle_id: query.vehicle_id,
            linked_entity_type: query.linked_entity_type,
            linked_entity_id: query.linked_entity_id,
        },
    )
    .await?;
    Ok(Json(docs))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<document::Model>> {
    Ok(Json(svc::get(&state.db, id).await?))
}

struct ParsedUpload {
    vehicle_id: Option<i32>,
    title: Option<String>,
    doc_type: Option<String>,
    linked_entity_type: Option<String>,
    linked_entity_id: Option<i32>,
    notes: Option<String>,
    file_name: Option<String>,
    file_data: Vec<u8>,
    mime_type: Option<String>,
}

async fn parse_multipart(mut multipart: Multipart) -> Result<ParsedUpload> {
    let mut vehicle_id: Option<i32> = None;
    let mut title: Option<String> = None;
    let mut doc_type: Option<String> = None;
    let mut linked_entity_type: Option<String> = None;
    let mut linked_entity_id: Option<i32> = None;
    let mut notes: Option<String> = None;
    let mut file_name: Option<String> = None;
    let mut file_data: Option<Vec<u8>> = None;
    let mut mime_type: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::BadRequest(format!("Multipart error: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "vehicle_id" => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
                vehicle_id = Some(
                    text.parse()
                        .map_err(|_| ApiError::BadRequest("Invalid vehicle_id".into()))?,
                );
            }
            "title" => {
                title = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| ApiError::BadRequest(e.to_string()))?,
                );
            }
            "doc_type" => {
                doc_type = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| ApiError::BadRequest(e.to_string()))?,
                );
            }
            "linked_entity_type" => {
                linked_entity_type = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| ApiError::BadRequest(e.to_string()))?,
                );
            }
            "linked_entity_id" => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
                linked_entity_id = Some(
                    text.parse()
                        .map_err(|_| ApiError::BadRequest("Invalid linked_entity_id".into()))?,
                );
            }
            "notes" => {
                notes = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| ApiError::BadRequest(e.to_string()))?,
                );
            }
            "file" => {
                file_name = field.file_name().map(std::string::ToString::to_string);
                mime_type = field.content_type().map(std::string::ToString::to_string);
                file_data = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| ApiError::BadRequest(e.to_string()))?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }

    Ok(ParsedUpload {
        vehicle_id,
        title,
        doc_type,
        linked_entity_type,
        linked_entity_id,
        notes,
        file_name,
        file_data: file_data.ok_or_else(|| ApiError::BadRequest("No file provided".into()))?,
        mime_type,
    })
}

pub async fn upload(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<Json<document::Model>> {
    let parsed = parse_multipart(multipart).await?;

    // Validation, disk placement, and the DB row all live in the shared
    // service (the MCP attach_document tool shares the same path).
    let result = svc::store(
        &state.db,
        &state.config,
        StoreDocument {
            vehicle_id: parsed.vehicle_id,
            title: parsed.title,
            file_name: Some(parsed.file_name.unwrap_or_else(|| "unnamed".into())),
            source: DocumentSource::Bytes(parsed.file_data),
            mime_type: parsed.mime_type,
            doc_type: parsed.doc_type,
            linked_entity_type: parsed.linked_entity_type,
            linked_entity_id: parsed.linked_entity_id,
            notes: parsed.notes,
            extracted_text: None,
        },
    )
    .await?;
    Ok(Json(result))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>> {
    let doc = svc::get(&state.db, id).await?;

    // Delete file from disk (with path traversal check)
    let files_dir = std::path::Path::new(&state.config.files_dir)
        .canonicalize()
        .map_err(|e| ApiError::Internal(format!("Invalid files_dir: {e}")))?;
    let full_path = files_dir.join(&doc.file_path);
    if full_path.exists() {
        let full_path = full_path
            .canonicalize()
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        if !full_path.starts_with(&files_dir) {
            return Err(ApiError::BadRequest("Invalid file path".into()));
        }
        tokio::fs::remove_file(&full_path)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
    }

    svc::delete(&state.db, id).await?;
    Ok(Json(serde_json::json!({ "deleted": id })))
}
