use axum::{
    Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

use crate::{
    AppState,
    api::{error::ApiError, serde_helpers::deserialize_optional},
};
use glovebox_shared::{
    entities::chat_message,
    inputs::ai_provider::{ChatInput, NewProvider, UpdateProvider as UpdateProviderInput},
    services::ai_ops::{
        self as svc, AiStatusResponse, AiSuggestion, ChatResult, ParsedInvoice, ProviderResponse,
    },
};

pub async fn status(State(state): State<AppState>) -> Result<Json<AiStatusResponse>, ApiError> {
    Ok(Json(svc::status(&state.db, &state.ai).await?))
}

#[derive(Debug, Deserialize)]
pub struct SuggestionsQuery {
    pub provider_id: Option<i32>,
}

pub async fn get_suggestions(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Query(query): Query<SuggestionsQuery>,
) -> Result<Json<Vec<AiSuggestion>>, ApiError> {
    Ok(Json(
        svc::suggestions(&state.db, &state.ai, vehicle_id, query.provider_id).await?,
    ))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ParseInvoiceRequest {
    pub document_id: i32,
    pub provider_id: Option<i32>,
}

pub async fn parse_invoice(
    State(state): State<AppState>,
    Json(body): Json<ParseInvoiceRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let parsed: ParsedInvoice = svc::parse_invoice(
        &state.db,
        &state.ai,
        &state.config,
        body.document_id,
        body.provider_id,
    )
    .await?;
    Ok(Json(parsed))
}

// --- Fetch models endpoint (pure HTTP passthrough; no persistence) ---

#[derive(Debug, Deserialize)]
pub struct FetchModelsRequest {
    pub api_key: String,
    pub provider: String,
    pub api_base: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: Option<String>,
}

pub async fn fetch_models(
    Json(body): Json<FetchModelsRequest>,
) -> Result<Json<Vec<ModelInfo>>, ApiError> {
    let client = reqwest::Client::new();

    match body.provider.as_str() {
        "claude" => {
            if body.api_key.is_empty() {
                return Err(ApiError::BadRequest("API key is required".to_string()));
            }
            let resp = client
                .get("https://api.anthropic.com/v1/models")
                .header("x-api-key", &body.api_key)
                .header("anthropic-version", "2023-06-01")
                .send()
                .await
                .map_err(|e| ApiError::Internal(format!("Request failed: {e}")))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body_text = resp.text().await.unwrap_or_default();
                tracing::error!("Anthropic models API returned {status}: {body_text}");
                return Err(ApiError::Internal(format!(
                    "Anthropic API returned status {status}"
                )));
            }

            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| ApiError::Internal(format!("Failed to parse response: {e}")))?;

            let models = json["data"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|m| ModelInfo {
                    id: m["id"].as_str().unwrap_or("").to_string(),
                    display_name: m["display_name"]
                        .as_str()
                        .map(std::string::ToString::to_string),
                })
                .filter(|m| !m.id.is_empty())
                .collect();

            Ok(Json(models))
        }
        "openai_compat" => {
            let base = body
                .api_base
                .as_deref()
                .unwrap_or("http://localhost:11434/v1");
            let url = format!("{}/models", base.trim_end_matches('/'));

            let mut req = client.get(&url);
            if !body.api_key.is_empty() {
                req = req.header("Authorization", format!("Bearer {}", body.api_key));
            }

            let resp = req
                .send()
                .await
                .map_err(|e| ApiError::Internal(format!("Request failed: {e}")))?;

            if !resp.status().is_success() {
                let status = resp.status();
                return Err(ApiError::Internal(format!("API returned status {status}")));
            }

            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| ApiError::Internal(format!("Failed to parse response: {e}")))?;

            let models = json["data"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|m| ModelInfo {
                    id: m["id"].as_str().unwrap_or("").to_string(),
                    display_name: None,
                })
                .filter(|m| !m.id.is_empty())
                .collect();

            Ok(Json(models))
        }
        _ => Err(ApiError::BadRequest(format!(
            "Unknown provider: {}",
            body.provider
        ))),
    }
}

// --- Chat endpoints ---

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub vehicle_id: Option<i32>,
    pub conversation_id: i32,
    pub message: String,
    pub provider_id: Option<i32>,
    pub document_id: Option<i32>,
}

pub async fn chat(
    State(state): State<AppState>,
    Json(body): Json<ChatRequest>,
) -> Result<Json<ChatResult>, ApiError> {
    let result = svc::chat(
        &state.db,
        &state.ai,
        &state.config,
        ChatInput {
            vehicle_id: body.vehicle_id,
            conversation_id: body.conversation_id,
            message: body.message,
            provider_id: body.provider_id,
            document_id: body.document_id,
        },
    )
    .await?;
    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
pub struct ChatHistoryQuery {
    pub vehicle_id: Option<i32>,
}

pub async fn chat_history(
    State(state): State<AppState>,
    Query(query): Query<ChatHistoryQuery>,
) -> Result<Json<Vec<chat_message::Model>>, ApiError> {
    Ok(Json(
        glovebox_shared::services::conversation::chat_history(&state.db, query.vehicle_id).await?,
    ))
}

// --- AI Provider CRUD ---

#[derive(Debug, Deserialize)]
pub struct CreateProvider {
    pub name: String,
    pub provider_type: String,
    pub api_key: Option<String>,
    pub api_base: Option<String>,
    pub model: Option<String>,
    pub is_default: Option<bool>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProvider {
    pub name: Option<String>,
    pub provider_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub api_key: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub api_base: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub model: Option<Option<String>>,
    pub is_default: Option<bool>,
    pub enabled: Option<bool>,
}

pub async fn list_providers(
    State(state): State<AppState>,
) -> Result<Json<Vec<ProviderResponse>>, ApiError> {
    Ok(Json(svc::list_providers(&state.db).await?))
}

pub async fn create_provider(
    State(state): State<AppState>,
    Json(body): Json<CreateProvider>,
) -> Result<Json<ProviderResponse>, ApiError> {
    let created = svc::create_provider(
        &state.db,
        NewProvider {
            name: body.name,
            provider_type: body.provider_type,
            api_key: body.api_key,
            api_base: body.api_base,
            model: body.model,
            is_default: body.is_default,
            enabled: body.enabled,
        },
    )
    .await?;
    Ok(Json(created))
}

pub async fn update_provider(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateProvider>,
) -> Result<Json<ProviderResponse>, ApiError> {
    let updated = svc::update_provider(
        &state.db,
        id,
        UpdateProviderInput {
            name: body.name,
            provider_type: body.provider_type,
            api_key: body.api_key,
            api_base: body.api_base,
            model: body.model,
            is_default: body.is_default,
            enabled: body.enabled,
        },
    )
    .await?;
    Ok(Json(updated))
}

pub async fn delete_provider(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, ApiError> {
    svc::delete_provider(&state.db, id).await?;
    Ok(Json(serde_json::json!({ "deleted": id })))
}
