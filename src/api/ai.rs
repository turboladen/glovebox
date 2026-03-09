use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::entities::{ai_provider_config, chat_message, document};
use crate::services::ai::{AiRequest, Attachment, ChatMessage, Role};
use crate::AppState;

#[derive(Serialize)]
pub struct AiStatusResponse {
    pub provider: String,
    pub configured: bool,
    pub default_provider_id: Option<i32>,
    pub providers: Vec<ProviderSummary>,
}

#[derive(Serialize)]
pub struct ProviderSummary {
    pub id: i32,
    pub name: String,
    pub provider_type: String,
    pub is_default: bool,
    pub enabled: bool,
}

pub async fn status(State(state): State<AppState>) -> Result<Json<AiStatusResponse>, ApiError> {
    let all_providers = ai_provider_config::Entity::find().all(&state.db).await?;

    let default_provider_id = all_providers
        .iter()
        .find(|p| p.is_default && p.enabled)
        .map(|p| p.id);

    let configured = state
        .ai
        .any_configured()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let provider = match state.ai.resolve(None).await {
        Ok(p) => p.provider_name().to_string(),
        Err(_) => "none".to_string(),
    };

    let providers = all_providers
        .into_iter()
        .map(|p| ProviderSummary {
            id: p.id,
            name: p.name,
            provider_type: p.provider_type,
            is_default: p.is_default,
            enabled: p.enabled,
        })
        .collect();

    Ok(Json(AiStatusResponse {
        provider,
        configured,
        default_provider_id,
        providers,
    }))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AiSuggestion {
    pub title: String,
    pub reason: String,
    pub urgency: String,
    pub estimated_cost_range: Option<String>,
}

/// Strip markdown code fences from AI responses (shared helper).
pub fn strip_code_fences(s: &str) -> &str {
    let s = s.trim();
    let s = s
        .strip_prefix("```json")
        .or_else(|| s.strip_prefix("```"))
        .unwrap_or(s);
    let s = s.strip_suffix("```").unwrap_or(s);
    s.trim()
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
    let provider = state
        .ai
        .resolve(query.provider_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let context = crate::services::ai::context::build_vehicle_context(&state.db, vehicle_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let preamble = crate::services::ai::context::GLOVEBOX_PREAMBLE;
    let request = AiRequest {
        system_prompt: format!(
            "{preamble}\n\nBased on the vehicle data provided, suggest maintenance actions the \
            owner should prioritize in the next 3 months. Consider wear patterns, seasonal \
            factors, mileage-based intervals, and manufacturer recommendations. \
            Return ONLY a valid JSON array (no markdown)."
        ),
        messages: vec![ChatMessage {
            role: Role::User,
            content: format!(
                "{context}\n\nBased on this vehicle data, what maintenance should I prioritize \
                in the next 3 months? Return as a JSON array of objects."
            ),
        }],
        attachments: vec![],
        max_tokens: None,
    };

    let response = provider
        .complete(request)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let cleaned = strip_code_fences(&response.content);
    let suggestions: Vec<AiSuggestion> = serde_json::from_str(cleaned)
        .map_err(|e| ApiError::Internal(format!("Failed to parse AI suggestions: {e}")))?;

    Ok(Json(suggestions))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ParseInvoiceRequest {
    pub document_id: i32,
    pub provider_id: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ParsedInvoice {
    pub service_date: Option<String>,
    pub shop_name: Option<String>,
    pub mileage: Option<i32>,
    pub description: Option<String>,
    pub line_items: Vec<LineItem>,
    pub parts_cost_cents: Option<i32>,
    pub labor_cost_cents: Option<i32>,
    pub total_cost_cents: Option<i32>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LineItem {
    pub description: String,
    pub cost_cents: Option<i32>,
}

const INVOICE_SYSTEM_PROMPT: &str = r#"You are analyzing an automotive service invoice or receipt. Extract the following fields and return ONLY valid JSON (no markdown, no explanation):
{
  "service_date": "YYYY-MM-DD or null",
  "shop_name": "string or null",
  "mileage": integer or null,
  "description": "brief summary of work performed",
  "line_items": [{"description": "string", "cost_cents": integer or null}],
  "parts_cost_cents": integer or null (total parts cost in cents),
  "labor_cost_cents": integer or null (total labor cost in cents),
  "total_cost_cents": integer or null (grand total in cents),
  "notes": "any other relevant information or null"
}
All costs should be in cents (multiply dollar amounts by 100). Return ONLY the JSON object."#;

pub async fn parse_invoice(
    State(state): State<AppState>,
    Json(body): Json<ParseInvoiceRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let provider = state
        .ai
        .resolve(body.provider_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    // Look up document
    let doc = document::Entity::find_by_id(body.document_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound("Document not found".to_string()))?;

    // Verify it's a PDF
    let mime = doc.mime_type.as_deref().unwrap_or("");
    if !mime.contains("pdf") {
        return Err(ApiError::BadRequest("Document is not a PDF".to_string()));
    }

    // Read file from disk (validate path stays within files_dir)
    let files_dir = std::path::Path::new(&state.config.files_dir)
        .canonicalize()
        .map_err(|e| ApiError::Internal(format!("Invalid files_dir: {e}")))?;
    let file_path = files_dir.join(&doc.file_path);
    let file_path = file_path
        .canonicalize()
        .map_err(|_| ApiError::NotFound("Document file not found".to_string()))?;
    if !file_path.starts_with(&files_dir) {
        return Err(ApiError::BadRequest("Invalid file path".to_string()));
    }
    let file_data = tokio::fs::read(&file_path)
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to read file: {e}")))?;

    // Build AI request
    let request = AiRequest {
        system_prompt: format!(
            "{}\n\n{INVOICE_SYSTEM_PROMPT}",
            crate::services::ai::context::GLOVEBOX_PREAMBLE
        ),
        messages: vec![ChatMessage {
            role: Role::User,
            content: "Please extract the service record data from this attached invoice/receipt."
                .to_string(),
        }],
        attachments: vec![Attachment {
            mime_type: "application/pdf".to_string(),
            data: file_data,
        }],
        max_tokens: Some(4096),
    };

    let response = provider
        .complete(request)
        .await
        .map_err(|e| ApiError::Internal(format!("AI error: {e}")))?;

    // Parse AI response, stripping code fences if present
    let cleaned = strip_code_fences(&response.content);
    let parsed: ParsedInvoice = serde_json::from_str(cleaned).map_err(|e| {
        ApiError::Internal(format!(
            "Failed to parse AI response as invoice: {}. Raw response: {}",
            e, response.content
        ))
    })?;

    Ok(Json(parsed))
}

// --- Fetch models endpoint ---

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
    pub message: String,
    pub provider_id: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct ChatResponseBody {
    pub message: chat_message::Model,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct ChatHistoryQuery {
    pub vehicle_id: Option<i32>,
}

pub async fn chat(
    State(state): State<AppState>,
    Json(body): Json<ChatRequest>,
) -> Result<Json<ChatResponseBody>, ApiError> {
    let provider = state
        .ai
        .resolve(body.provider_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    // Build vehicle context if vehicle_id is provided
    let preamble = crate::services::ai::context::GLOVEBOX_PREAMBLE;
    let system_prompt = if let Some(vid) = body.vehicle_id {
        let context = crate::services::ai::context::build_vehicle_context(&state.db, vid)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        format!(
            "{preamble}\n\nAnswer questions about the owner's vehicle based on the data below. \
            Be concise and practical.\n\n{context}"
        )
    } else {
        format!(
            "{preamble}\n\nAnswer questions about car maintenance, repairs, and ownership. \
            Be concise and practical."
        )
    };

    // Load recent chat history (last 20 messages)
    let mut history_query = chat_message::Entity::find();
    if let Some(vid) = body.vehicle_id {
        history_query = history_query.filter(chat_message::Column::VehicleId.eq(vid));
    } else {
        history_query = history_query.filter(chat_message::Column::VehicleId.is_null());
    }
    let history = history_query
        .order_by_desc(chat_message::Column::CreatedAt)
        .limit(20)
        .all(&state.db)
        .await?;

    // Convert to AI messages (reverse to oldest-first for conversation order)
    let mut messages: Vec<ChatMessage> = history
        .into_iter()
        .rev()
        .map(|m| ChatMessage {
            role: if m.role == "user" {
                Role::User
            } else {
                Role::Assistant
            },
            content: m.content,
        })
        .collect();

    // Add the new user message
    messages.push(ChatMessage {
        role: Role::User,
        content: body.message.clone(),
    });

    let request = AiRequest {
        system_prompt,
        messages,
        attachments: vec![],
        max_tokens: None,
    };

    let response = provider
        .complete(request)
        .await
        .map_err(|e| ApiError::Internal(format!("AI error: {e}")))?;

    // Save user + assistant messages atomically
    let txn = state.db.begin().await?;

    let user_msg = chat_message::ActiveModel {
        vehicle_id: Set(body.vehicle_id),
        role: Set("user".to_string()),
        content: Set(body.message),
        ..Default::default()
    };
    user_msg.insert(&txn).await?;

    let assistant_msg = chat_message::ActiveModel {
        vehicle_id: Set(body.vehicle_id),
        role: Set("assistant".to_string()),
        content: Set(response.content.clone()),
        ..Default::default()
    };
    let saved = assistant_msg.insert(&txn).await?;

    txn.commit().await?;

    Ok(Json(ChatResponseBody {
        message: saved,
        input_tokens: response.input_tokens,
        output_tokens: response.output_tokens,
    }))
}

pub async fn chat_history(
    State(state): State<AppState>,
    Query(query): Query<ChatHistoryQuery>,
) -> Result<Json<Vec<chat_message::Model>>, ApiError> {
    let mut q = chat_message::Entity::find();
    if let Some(vid) = query.vehicle_id {
        q = q.filter(chat_message::Column::VehicleId.eq(vid));
    }
    let messages = q
        .order_by_asc(chat_message::Column::CreatedAt)
        .limit(100)
        .all(&state.db)
        .await?;
    Ok(Json(messages))
}

// --- AI Provider CRUD ---

#[derive(Serialize)]
pub struct ProviderResponse {
    pub id: i32,
    pub name: String,
    pub provider_type: String,
    pub api_key_set: bool,
    pub api_base: Option<String>,
    pub model: Option<String>,
    pub is_default: bool,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<ai_provider_config::Model> for ProviderResponse {
    fn from(p: ai_provider_config::Model) -> Self {
        Self {
            id: p.id,
            name: p.name,
            provider_type: p.provider_type,
            api_key_set: p.api_key.is_some() && !p.api_key.as_ref().unwrap().is_empty(),
            api_base: p.api_base,
            model: p.model,
            is_default: p.is_default,
            enabled: p.enabled,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

pub async fn list_providers(
    State(state): State<AppState>,
) -> Result<Json<Vec<ProviderResponse>>, ApiError> {
    let providers = ai_provider_config::Entity::find().all(&state.db).await?;
    Ok(Json(
        providers.into_iter().map(ProviderResponse::from).collect(),
    ))
}

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

pub async fn create_provider(
    State(state): State<AppState>,
    Json(body): Json<CreateProvider>,
) -> Result<Json<ProviderResponse>, ApiError> {
    let is_default = body.is_default.unwrap_or(false);
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let txn = state.db.begin().await?;

    // If setting as default, clear any existing default
    if is_default {
        clear_default_provider(&txn).await?;
    }

    let model = ai_provider_config::ActiveModel {
        name: Set(body.name),
        provider_type: Set(body.provider_type),
        api_key: Set(body.api_key),
        api_base: Set(body.api_base),
        model: Set(body.model),
        is_default: Set(is_default),
        enabled: Set(body.enabled.unwrap_or(true)),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    };

    let saved = model.insert(&txn).await?;
    txn.commit().await?;

    Ok(Json(ProviderResponse::from(saved)))
}

#[derive(Debug, Deserialize)]
pub struct UpdateProvider {
    pub name: Option<String>,
    pub provider_type: Option<String>,
    pub api_key: Option<Option<String>>,
    pub api_base: Option<Option<String>>,
    pub model: Option<Option<String>>,
    pub is_default: Option<bool>,
    pub enabled: Option<bool>,
}

pub async fn update_provider(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateProvider>,
) -> Result<Json<ProviderResponse>, ApiError> {
    let txn = state.db.begin().await?;

    let existing = ai_provider_config::Entity::find_by_id(id)
        .one(&txn)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("AI provider {id} not found")))?;

    // If setting as default, clear any existing default
    if body.is_default == Some(true) {
        clear_default_provider(&txn).await?;
    }

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut active: ai_provider_config::ActiveModel = existing.into();

    if let Some(name) = body.name {
        active.name = Set(name);
    }
    if let Some(provider_type) = body.provider_type {
        active.provider_type = Set(provider_type);
    }
    if let Some(api_key) = body.api_key {
        active.api_key = Set(api_key);
    }
    if let Some(api_base) = body.api_base {
        active.api_base = Set(api_base);
    }
    if let Some(model) = body.model {
        active.model = Set(model);
    }
    if let Some(is_default) = body.is_default {
        active.is_default = Set(is_default);
    }
    if let Some(enabled) = body.enabled {
        active.enabled = Set(enabled);
    }
    active.updated_at = Set(now);

    let updated = active.update(&txn).await?;
    txn.commit().await?;

    Ok(Json(ProviderResponse::from(updated)))
}

pub async fn delete_provider(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let result = ai_provider_config::Entity::delete_by_id(id)
        .exec(&state.db)
        .await?;

    if result.rows_affected == 0 {
        return Err(ApiError::NotFound(format!("AI provider {id} not found")));
    }

    Ok(Json(serde_json::json!({ "deleted": id })))
}

async fn clear_default_provider(txn: &sea_orm::DatabaseTransaction) -> Result<(), ApiError> {
    use sea_orm::sea_query::Expr;

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    ai_provider_config::Entity::update_many()
        .col_expr(ai_provider_config::Column::IsDefault, Expr::value(false))
        .col_expr(ai_provider_config::Column::UpdatedAt, Expr::value(now))
        .filter(ai_provider_config::Column::IsDefault.eq(true))
        .exec(txn)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_ai_suggestion() {
        let json = r#"[
            {
                "title": "Oil Change",
                "reason": "Due based on mileage interval",
                "urgency": "high",
                "estimated_cost_range": "$40-$80"
            },
            {
                "title": "Tire Rotation",
                "reason": "Recommended every 5,000 miles",
                "urgency": "medium",
                "estimated_cost_range": null
            }
        ]"#;
        let suggestions: Vec<AiSuggestion> = serde_json::from_str(json).unwrap();
        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].title, "Oil Change");
        assert_eq!(suggestions[0].urgency, "high");
        assert_eq!(
            suggestions[0].estimated_cost_range,
            Some("$40-$80".to_string())
        );
        assert_eq!(suggestions[1].title, "Tire Rotation");
        assert!(suggestions[1].estimated_cost_range.is_none());
    }

    #[test]
    fn strip_code_fences_json_block() {
        let input = "```json\n[{\"title\": \"test\"}]\n```";
        let result = strip_code_fences(input);
        assert_eq!(result, "[{\"title\": \"test\"}]");
    }

    #[test]
    fn strip_code_fences_plain_block() {
        let input = "```\n[{\"title\": \"test\"}]\n```";
        let result = strip_code_fences(input);
        assert_eq!(result, "[{\"title\": \"test\"}]");
    }

    #[test]
    fn strip_code_fences_no_fences() {
        let input = "[{\"title\": \"test\"}]";
        let result = strip_code_fences(input);
        assert_eq!(result, "[{\"title\": \"test\"}]");
    }

    #[test]
    fn strip_code_fences_with_whitespace() {
        let input = "  ```json\n[{\"title\": \"test\"}]\n```  ";
        let result = strip_code_fences(input);
        assert_eq!(result, "[{\"title\": \"test\"}]");
    }

    #[test]
    fn parse_complete_invoice_json() {
        let json = r#"{
            "service_date": "2024-03-15",
            "shop_name": "Joe's Auto",
            "mileage": 45000,
            "description": "Oil change and tire rotation",
            "line_items": [
                {"description": "Oil change", "cost_cents": 4999},
                {"description": "Tire rotation", "cost_cents": 2500}
            ],
            "parts_cost_cents": 2999,
            "labor_cost_cents": 4500,
            "total_cost_cents": 7499,
            "notes": "Next service at 50000 miles"
        }"#;
        let parsed: ParsedInvoice = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.service_date.as_deref(), Some("2024-03-15"));
        assert_eq!(parsed.shop_name.as_deref(), Some("Joe's Auto"));
        assert_eq!(parsed.mileage, Some(45000));
        assert_eq!(parsed.line_items.len(), 2);
        assert_eq!(parsed.line_items[0].description, "Oil change");
        assert_eq!(parsed.line_items[0].cost_cents, Some(4999));
        assert_eq!(parsed.total_cost_cents, Some(7499));
    }

    #[test]
    fn parse_minimal_invoice_json() {
        let json = r#"{
            "service_date": null,
            "shop_name": null,
            "mileage": null,
            "description": null,
            "line_items": [],
            "parts_cost_cents": null,
            "labor_cost_cents": null,
            "total_cost_cents": null,
            "notes": null
        }"#;
        let parsed: ParsedInvoice = serde_json::from_str(json).unwrap();
        assert!(parsed.service_date.is_none());
        assert!(parsed.line_items.is_empty());
    }

    #[test]
    fn parse_invoice_missing_optional_fields() {
        let json = r#"{
            "description": "Brake pad replacement",
            "line_items": [{"description": "Brake pads", "cost_cents": 12000}],
            "total_cost_cents": 35000
        }"#;
        let parsed: ParsedInvoice = serde_json::from_str(json).unwrap();
        assert!(parsed.service_date.is_none());
        assert_eq!(parsed.description.as_deref(), Some("Brake pad replacement"));
        assert_eq!(parsed.total_cost_cents, Some(35000));
    }

    #[test]
    fn parse_invoice_from_code_fenced_response() {
        let with_fence = "```json\n{\"service_date\": \"2024-01-01\", \"line_items\": []}\n```";
        let cleaned = strip_code_fences(with_fence);
        let parsed: ParsedInvoice = serde_json::from_str(cleaned).unwrap();
        assert_eq!(parsed.service_date.as_deref(), Some("2024-01-01"));
    }

    #[test]
    fn parse_invoice_from_plain_code_fenced_response() {
        let with_fence = "```\n{\"line_items\": [], \"total_cost_cents\": 100}\n```";
        let cleaned = strip_code_fences(with_fence);
        let parsed: ParsedInvoice = serde_json::from_str(cleaned).unwrap();
        assert_eq!(parsed.total_cost_cents, Some(100));
    }

    #[test]
    fn line_item_with_null_cost() {
        let json = r#"{"description": "Unknown part", "cost_cents": null}"#;
        let item: LineItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.description, "Unknown part");
        assert!(item.cost_cents.is_none());
    }
}
