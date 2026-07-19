use axum::{
    Json,
    extract::{Multipart, Path, Query, State},
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::{
    config::AppConfig,
    entities::document,
    inputs::document::{DocumentDisposition, DocumentFilter, DocumentSource, StoreDocument},
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
    extracted_text: Option<String>,
    file_name: Option<String>,
    file_data: Vec<u8>,
    mime_type: Option<String>,
}

/// Read a multipart part as text, mapping read errors to 400s.
async fn text_part(field: axum::extract::multipart::Field<'_>) -> Result<String> {
    field
        .text()
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))
}

async fn parse_multipart(mut multipart: Multipart) -> Result<ParsedUpload> {
    let mut vehicle_id: Option<i32> = None;
    let mut title: Option<String> = None;
    let mut doc_type: Option<String> = None;
    let mut linked_entity_type: Option<String> = None;
    let mut linked_entity_id: Option<i32> = None;
    let mut notes: Option<String> = None;
    let mut extracted_text: Option<String> = None;
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
                vehicle_id = Some(
                    text_part(field)
                        .await?
                        .parse()
                        .map_err(|_| ApiError::BadRequest("Invalid vehicle_id".into()))?,
                );
            }
            "title" => title = Some(text_part(field).await?),
            "doc_type" => doc_type = Some(text_part(field).await?),
            "linked_entity_type" => linked_entity_type = Some(text_part(field).await?),
            "linked_entity_id" => {
                linked_entity_id = Some(
                    text_part(field)
                        .await?
                        .parse()
                        .map_err(|_| ApiError::BadRequest("Invalid linked_entity_id".into()))?,
                );
            }
            "notes" => notes = Some(text_part(field).await?),
            "extracted_text" => extracted_text = Some(text_part(field).await?),
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
        extracted_text,
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
            extracted_text: parsed.extracted_text,
        },
    )
    .await?;
    // The HTTP surface returns the stored document row (stable DTO); the
    // idempotency signal (`already_present`) is surfaced only over MCP.
    Ok(Json(result.document))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>> {
    let doc = svc::get(&state.db, id).await?;
    // Row first, then file: a file-removal failure after this point leaves an
    // orphaned file on disk (harmless), whereas the old file-first order could
    // leave a surviving row pointing at a deleted file. Best-effort like the
    // cascade path — the row is the invariant, so a file error must not turn
    // an already-committed delete into a 4xx/5xx the client would retry.
    svc::delete(&state.db, id).await?;
    remove_files_best_effort(&state.config, std::slice::from_ref(&doc.file_path)).await;
    Ok(Json(serde_json::json!({ "deleted": id })))
}

/// Clear a document's entity link (`DocumentsTab` "Unlink", incl. orphan
/// cleanup). Returns the updated row.
pub async fn unlink(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<document::Model>> {
    Ok(Json(svc::unlink(&state.db, id).await?))
}

/// `?documents=keep|delete` on entity DELETE endpoints. Absent means `Keep`
/// (unlink + provenance note) — dangling links must never be produced.
#[derive(Deserialize)]
pub struct DeleteDocsQuery {
    #[serde(default)]
    pub documents: DocumentDisposition,
}

/// Remove cascade-deleted document files after the owning transaction has
/// committed. Best-effort by design: the rows are already gone, so a failed
/// unlink only strands a file on disk — log it, never fail the request.
pub(crate) async fn remove_files_best_effort(config: &AppConfig, paths: &[String]) {
    for path in paths {
        if let Err(e) = svc::remove_stored_file(config, path).await {
            tracing::warn!("failed to remove document file {path}: {e:?}");
        }
    }
}
