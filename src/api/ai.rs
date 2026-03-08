use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::api::error::ApiError;
use crate::entities::{chat_message, document};
use crate::services::ai::{AiRequest, Attachment, ChatMessage, Role};

#[derive(Serialize)]
pub struct AiStatusResponse {
    pub provider: String,
    pub configured: bool,
}

pub async fn status(State(state): State<AppState>) -> Json<AiStatusResponse> {
    Json(AiStatusResponse {
        provider: state.ai.provider_name().to_string(),
        configured: state.ai.is_configured(),
    })
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

pub async fn get_suggestions(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<Vec<AiSuggestion>>, ApiError> {
    if !state.ai.is_configured() {
        return Err(ApiError::BadRequest("AI is not configured".to_string()));
    }

    let context = crate::services::ai::context::build_vehicle_context(&state.db, vehicle_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let request = AiRequest {
        system_prompt: "You are an expert automotive maintenance advisor. Based on the vehicle \
            data provided, suggest maintenance actions the owner should prioritize in the next \
            3 months. Consider wear patterns, seasonal factors, mileage-based intervals, and \
            manufacturer recommendations. Return ONLY a valid JSON array (no markdown)."
            .to_string(),
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

    let response = state
        .ai
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
    if !state.ai.is_configured() {
        return Err(ApiError::BadRequest("AI is not configured".to_string()));
    }

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
    let file_data = tokio::fs::read(&file_path).await.map_err(|e| {
        ApiError::Internal(format!("Failed to read file: {e}"))
    })?;

    // Build AI request
    let request = AiRequest {
        system_prompt: INVOICE_SYSTEM_PROMPT.to_string(),
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

    let response = state
        .ai
        .complete(request)
        .await
        .map_err(|e| ApiError::Internal(format!("AI error: {}", e)))?;

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

// --- Chat endpoints ---

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub vehicle_id: Option<i32>,
    pub message: String,
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
    if !state.ai.is_configured() {
        return Err(ApiError::BadRequest("AI is not configured".to_string()));
    }

    // Build vehicle context if vehicle_id is provided
    let system_prompt = if let Some(vid) = body.vehicle_id {
        let context = crate::services::ai::context::build_vehicle_context(&state.db, vid)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        format!(
            "You are a knowledgeable automotive assistant. Answer questions about the owner's \
            vehicle based on the data below. Be concise and practical.\n\n{context}"
        )
    } else {
        "You are a knowledgeable automotive assistant. Answer questions about car \
        maintenance, repairs, and ownership. Be concise and practical."
            .to_string()
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

    let response = state
        .ai
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
        .limit(200)
        .all(&state.db)
        .await?;
    Ok(Json(messages))
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
        let with_fence =
            "```json\n{\"service_date\": \"2024-01-01\", \"line_items\": []}\n```";
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
