use sea_orm::*;
use serde::{Deserialize, Serialize};

use crate::{
    config::AppConfig,
    entities::{ai_provider_config, chat_message, conversation, document},
    error::{DomainError, DomainResult},
    inputs::ai_provider::{ChatInput, NewProvider, UpdateProvider},
    services::ai::{
        AiRequest, Attachment, ChatMessage, Role, context, registry::AiProviderRegistry,
        strip_code_fences,
    },
};

// --- Status ---

#[derive(Debug, Serialize)]
pub struct AiStatusResponse {
    pub provider: String,
    pub configured: bool,
    pub default_provider_id: Option<i32>,
    pub providers: Vec<ProviderSummary>,
}

#[derive(Debug, Serialize)]
pub struct ProviderSummary {
    pub id: i32,
    pub name: String,
    pub provider_type: String,
    pub is_default: bool,
    pub enabled: bool,
}

pub async fn status(
    db: &impl ConnectionTrait,
    registry: &AiProviderRegistry,
) -> DomainResult<AiStatusResponse> {
    let all_providers = ai_provider_config::Entity::find().all(db).await?;

    let default_provider_id = all_providers
        .iter()
        .find(|p| p.is_default && p.enabled)
        .map(|p| p.id);

    let configured = registry
        .any_configured()
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

    let provider = match registry.resolve(None).await {
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

    Ok(AiStatusResponse {
        provider,
        configured,
        default_provider_id,
        providers,
    })
}

// --- Suggestions ---

#[derive(Debug, Serialize, Deserialize)]
pub struct AiSuggestion {
    #[serde(alias = "name", alias = "action", alias = "task")]
    pub title: String,
    #[serde(alias = "description", alias = "explanation")]
    pub reason: String,
    #[serde(default = "default_urgency")]
    pub urgency: String,
    pub estimated_cost_range: Option<String>,
}

fn default_urgency() -> String {
    "medium".to_string()
}

pub async fn suggestions(
    db: &DatabaseConnection,
    registry: &AiProviderRegistry,
    vehicle_id: i32,
    provider_id: Option<i32>,
) -> DomainResult<Vec<AiSuggestion>> {
    // Verify the vehicle exists before doing anything else — the handler layer
    // does not pre-check, and MCP callers hit this fn directly.
    crate::services::vehicle::require(db, vehicle_id).await?;

    let provider = registry
        .resolve(provider_id)
        .await
        .map_err(|e| DomainError::BadRequest(e.to_string()))?;

    let context = context::build_vehicle_context(db, vehicle_id)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

    let preamble = context::GLOVEBOX_PREAMBLE;
    let request = AiRequest {
        system_prompt: format!(
            "{preamble}\n\nBased on the vehicle data provided, suggest maintenance actions the \
             owner should prioritize in the next 3 months. Consider wear patterns, seasonal \
             factors, mileage-based intervals, and manufacturer recommendations.\nReturn ONLY a \
             valid JSON array (no markdown) where each object has exactly these fields:\n- \
             \"title\": string (short name of the maintenance action)\n- \"reason\": string (why \
             this is needed)\n- \"urgency\": string (\"high\", \"medium\", or \"low\")\n- \
             \"estimated_cost_range\": string or null (e.g. \"$50-$100\")"
        ),
        messages: vec![ChatMessage {
            role: Role::User,
            content: format!(
                "{context}\n\nBased on this vehicle data, what maintenance should I prioritize in \
                 the next 3 months? Return as a JSON array of objects with fields: title, reason, \
                 urgency, estimated_cost_range."
            ),
        }],
        attachments: vec![],
        max_tokens: None,
    };

    let response = provider
        .complete(request)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

    let cleaned = strip_code_fences(&response.content);
    let suggestions: Vec<AiSuggestion> = serde_json::from_str(cleaned)
        .map_err(|e| DomainError::Internal(format!("Failed to parse AI suggestions: {e}")))?;

    Ok(suggestions)
}

// --- Invoice parsing ---

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
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub quantity: Option<f64>,
    #[serde(default)]
    pub unit_cost_cents: Option<i32>,
    pub cost_cents: Option<i32>,
}

const INVOICE_SYSTEM_PROMPT: &str = r#"You are analyzing an automotive service invoice or receipt. Extract the following fields and return ONLY valid JSON (no markdown, no explanation):
{
  "service_date": "YYYY-MM-DD or null",
  "shop_name": "string or null",
  "mileage": integer or null,
  "description": "brief summary of work performed",
  "line_items": [{"description": "string", "category": "part" | "labor" | "fee" | "tax" | "other" | null, "quantity": number or null, "unit_cost_cents": integer or null, "cost_cents": integer or null}],
  "parts_cost_cents": integer or null (total parts cost in cents),
  "labor_cost_cents": integer or null (total labor cost in cents),
  "total_cost_cents": integer or null (grand total in cents),
  "notes": "any other relevant information or null"
}
All costs should be in cents (multiply dollar amounts by 100). For each line item, classify its category as "part" (parts/materials), "labor" (work/time), "fee" (shop supplies, environmental fees, etc.), "tax" (sales tax, etc.), or "other". Return ONLY the JSON object."#;

pub async fn parse_invoice(
    db: &impl ConnectionTrait,
    registry: &AiProviderRegistry,
    config: &AppConfig,
    document_id: i32,
    provider_id: Option<i32>,
) -> DomainResult<ParsedInvoice> {
    let provider = registry
        .resolve(provider_id)
        .await
        .map_err(|e| DomainError::BadRequest(e.to_string()))?;

    // Look up document
    let doc = document::Entity::find_by_id(document_id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound("Document not found".to_string()))?;

    // Verify it's a PDF
    let mime = doc.mime_type.as_deref().unwrap_or("");
    if !mime.contains("pdf") {
        return Err(DomainError::BadRequest("Document is not a PDF".to_string()));
    }

    let file_data = read_document_file(config, &doc.file_path).await?;

    // Build AI request
    let request = AiRequest {
        system_prompt: format!("{}\n\n{INVOICE_SYSTEM_PROMPT}", context::GLOVEBOX_PREAMBLE),
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
        .map_err(|e| DomainError::Internal(format!("AI error: {e}")))?;

    // Parse AI response, stripping code fences if present
    let cleaned = strip_code_fences(&response.content);
    let parsed: ParsedInvoice = serde_json::from_str(cleaned).map_err(|e| {
        DomainError::Internal(format!(
            "Failed to parse AI response as invoice: {}. Raw response: {}",
            e, response.content
        ))
    })?;

    // Persist extracted text back to the document
    let mut active: document::ActiveModel = doc.into();
    active.extracted_text = Set(Some(response.content.clone()));
    active.update(db).await?;

    Ok(parsed)
}

// --- Chat ---

#[derive(Debug, Serialize)]
pub struct ChatResult {
    pub message: chat_message::Model,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

#[allow(clippy::too_many_lines)]
pub async fn chat(
    db: &DatabaseConnection,
    registry: &AiProviderRegistry,
    config: &AppConfig,
    input: ChatInput,
) -> DomainResult<ChatResult> {
    // Verify the conversation exists and belongs to the claimed vehicle BEFORE
    // anything else. A wrong-vehicle conversation must be indistinguishable
    // from a nonexistent one (no ownership oracle).
    let convo_check = conversation::Entity::find_by_id(input.conversation_id)
        .one(db)
        .await?
        .ok_or_else(|| {
            DomainError::NotFound(format!("Conversation {} not found", input.conversation_id))
        })?;
    if convo_check.vehicle_id != input.vehicle_id {
        return Err(DomainError::NotFound(format!(
            "Conversation {} not found",
            input.conversation_id
        )));
    }

    let provider = registry
        .resolve(input.provider_id)
        .await
        .map_err(|e| DomainError::BadRequest(e.to_string()))?;

    // Build vehicle context if vehicle_id is provided
    let preamble = context::GLOVEBOX_PREAMBLE;
    let data_entry_instructions = context::DATA_ENTRY_INSTRUCTIONS;
    let system_prompt = if let Some(vid) = input.vehicle_id {
        let ctx = context::build_vehicle_context(db, vid)
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;
        format!(
            "{preamble}\n\nAnswer questions about the owner's vehicle based on the data below. Be \
             concise and practical.\n\n{ctx}\n{data_entry_instructions}"
        )
    } else {
        format!(
            "{preamble}\n\nAnswer questions about car maintenance, repairs, and ownership. Be \
             concise and practical."
        )
    };

    // Load recent chat history (last 20 messages) scoped to conversation
    let history = chat_message::Entity::find()
        .filter(chat_message::Column::ConversationId.eq(input.conversation_id))
        .order_by_desc(chat_message::Column::CreatedAt)
        .limit(20)
        .all(db)
        .await?;

    // Convert to AI messages (reverse to oldest-first for conversation order)
    let is_first_exchange = history.is_empty();
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
        content: input.message.clone(),
    });

    // Load document attachment if document_id is provided
    let attachments = if let Some(doc_id) = input.document_id {
        let doc = document::Entity::find_by_id(doc_id)
            .one(db)
            .await?
            .ok_or_else(|| DomainError::NotFound(format!("Document {doc_id} not found")))?;

        let mime = doc
            .mime_type
            .as_deref()
            .unwrap_or("application/octet-stream")
            .to_string();
        let file_data = read_document_file(config, &doc.file_path).await?;

        vec![Attachment {
            mime_type: mime,
            data: file_data,
        }]
    } else {
        vec![]
    };

    let request = AiRequest {
        system_prompt,
        messages,
        attachments,
        max_tokens: None,
    };

    let response = provider
        .complete(request)
        .await
        .map_err(|e| DomainError::Internal(format!("AI error: {e}")))?;

    // Save user + assistant messages atomically
    let txn = db.begin().await?;

    let user_msg = chat_message::ActiveModel {
        vehicle_id: Set(input.vehicle_id),
        conversation_id: Set(Some(input.conversation_id)),
        role: Set("user".to_string()),
        content: Set(input.message.clone()),
        ..Default::default()
    };
    user_msg.insert(&txn).await?;

    let assistant_msg = chat_message::ActiveModel {
        vehicle_id: Set(input.vehicle_id),
        conversation_id: Set(Some(input.conversation_id)),
        role: Set("assistant".to_string()),
        content: Set(response.content.clone()),
        ..Default::default()
    };
    let saved = assistant_msg.insert(&txn).await?;

    // Auto-title: if this is the first exchange in a "New Chat" conversation,
    // set the title from the user's first message (truncated to 60 chars)
    if is_first_exchange {
        if let Some(convo) = conversation::Entity::find_by_id(input.conversation_id)
            .one(&txn)
            .await?
            && convo.title == "New Chat"
        {
            let auto_title = if input.message.chars().count() > 60 {
                let truncated: String = input.message.chars().take(57).collect();
                format!("{truncated}...")
            } else {
                input.message
            };
            let mut active: conversation::ActiveModel = convo.into();
            active.title = Set(auto_title);
            active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
            active.update(&txn).await?;
        }
    } else {
        // Touch updated_at on the conversation
        if let Some(convo) = conversation::Entity::find_by_id(input.conversation_id)
            .one(&txn)
            .await?
        {
            let mut active: conversation::ActiveModel = convo.into();
            active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
            active.update(&txn).await?;
        }
    }

    txn.commit().await?;

    Ok(ChatResult {
        message: saved,
        input_tokens: response.input_tokens,
        output_tokens: response.output_tokens,
    })
}

// --- AI Provider CRUD ---

#[derive(Debug, Serialize)]
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

pub async fn list_providers(db: &impl ConnectionTrait) -> DomainResult<Vec<ProviderResponse>> {
    let providers = ai_provider_config::Entity::find().all(db).await?;
    Ok(providers.into_iter().map(ProviderResponse::from).collect())
}

pub async fn create_provider<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    input: NewProvider,
) -> DomainResult<ProviderResponse> {
    let is_default = input.is_default.unwrap_or(false);
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let txn = db.begin().await?;

    // If setting as default, clear any existing default
    if is_default {
        clear_default_provider(&txn).await?;
    }

    let model = ai_provider_config::ActiveModel {
        name: Set(input.name),
        provider_type: Set(input.provider_type),
        api_key: Set(input.api_key),
        api_base: Set(input.api_base),
        model: Set(input.model),
        is_default: Set(is_default),
        enabled: Set(input.enabled.unwrap_or(true)),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    };

    let saved = model.insert(&txn).await?;
    txn.commit().await?;

    Ok(ProviderResponse::from(saved))
}

pub async fn update_provider<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    id: i32,
    input: UpdateProvider,
) -> DomainResult<ProviderResponse> {
    let txn = db.begin().await?;

    let existing = ai_provider_config::Entity::find_by_id(id)
        .one(&txn)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("AI provider {id} not found")))?;

    // If setting as default, clear any existing default
    if input.is_default == Some(true) {
        clear_default_provider(&txn).await?;
    }

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut active: ai_provider_config::ActiveModel = existing.into();

    if let Some(name) = input.name {
        active.name = Set(name);
    }
    if let Some(provider_type) = input.provider_type {
        active.provider_type = Set(provider_type);
    }
    if let Some(api_key) = input.api_key {
        active.api_key = Set(api_key);
    }
    if let Some(api_base) = input.api_base {
        active.api_base = Set(api_base);
    }
    if let Some(model) = input.model {
        active.model = Set(model);
    }
    if let Some(is_default) = input.is_default {
        active.is_default = Set(is_default);
    }
    if let Some(enabled) = input.enabled {
        active.enabled = Set(enabled);
    }
    active.updated_at = Set(now);

    let updated = active.update(&txn).await?;
    txn.commit().await?;

    Ok(ProviderResponse::from(updated))
}

pub async fn delete_provider(db: &impl ConnectionTrait, id: i32) -> DomainResult<()> {
    let result = ai_provider_config::Entity::delete_by_id(id)
        .exec(db)
        .await?;

    if result.rows_affected == 0 {
        return Err(DomainError::NotFound(format!("AI provider {id} not found")));
    }

    Ok(())
}

async fn clear_default_provider(db: &impl ConnectionTrait) -> DomainResult<()> {
    use sea_orm::sea_query::Expr;

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    ai_provider_config::Entity::update_many()
        .col_expr(ai_provider_config::Column::IsDefault, Expr::value(false))
        .col_expr(ai_provider_config::Column::UpdatedAt, Expr::value(now))
        .filter(ai_provider_config::Column::IsDefault.eq(true))
        .exec(db)
        .await?;
    Ok(())
}

// --- Helpers ---

/// Read a document's file from disk, validating the resolved path stays within `files_dir`.
async fn read_document_file(config: &AppConfig, file_path: &str) -> DomainResult<Vec<u8>> {
    let files_dir = std::path::Path::new(&config.files_dir)
        .canonicalize()
        .map_err(|e| DomainError::Internal(format!("Invalid files_dir: {e}")))?;
    let joined = files_dir.join(file_path);
    let resolved = joined
        .canonicalize()
        .map_err(|_| DomainError::NotFound("Document file not found".to_string()))?;
    if !resolved.starts_with(&files_dir) {
        return Err(DomainError::BadRequest("Invalid file path".to_string()));
    }
    tokio::fs::read(&resolved)
        .await
        .map_err(|e| DomainError::Internal(format!("Failed to read file: {e}")))
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
        assert!(item.category.is_none());
        assert!(item.quantity.is_none());
        assert!(item.unit_cost_cents.is_none());
    }

    #[test]
    fn line_item_with_enriched_fields() {
        let json = r#"{
            "description": "Oil filter",
            "category": "part",
            "quantity": 1.0,
            "unit_cost_cents": 1299,
            "cost_cents": 1299
        }"#;
        let item: LineItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.description, "Oil filter");
        assert_eq!(item.category.as_deref(), Some("part"));
        assert_eq!(item.quantity, Some(1.0));
        assert_eq!(item.unit_cost_cents, Some(1299));
        assert_eq!(item.cost_cents, Some(1299));
    }

    #[test]
    fn line_item_backward_compat_no_new_fields() {
        let json = r#"{"description": "Brake pads", "cost_cents": 5000}"#;
        let item: LineItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.description, "Brake pads");
        assert_eq!(item.cost_cents, Some(5000));
        assert!(item.category.is_none());
        assert!(item.quantity.is_none());
        assert!(item.unit_cost_cents.is_none());
    }

    #[test]
    fn parse_invoice_with_enriched_line_items() {
        let json = r#"{
            "service_date": "2024-06-15",
            "shop_name": "AutoZone Service",
            "description": "Brake job",
            "line_items": [
                {"description": "Brake pads (front)", "category": "part", "quantity": 1, "unit_cost_cents": 4500, "cost_cents": 4500},
                {"description": "Labor - brake replacement", "category": "labor", "cost_cents": 12000},
                {"description": "Shop supplies", "category": "fee", "cost_cents": 500}
            ],
            "total_cost_cents": 17000
        }"#;
        let parsed: ParsedInvoice = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.line_items.len(), 3);
        assert_eq!(parsed.line_items[0].category.as_deref(), Some("part"));
        assert_eq!(parsed.line_items[0].quantity, Some(1.0));
        assert_eq!(parsed.line_items[1].category.as_deref(), Some("labor"));
        assert!(parsed.line_items[1].quantity.is_none());
        assert_eq!(parsed.line_items[2].category.as_deref(), Some("fee"));
    }

    #[tokio::test]
    async fn suggestions_missing_vehicle_is_not_found() {
        use crate::test_support::test_db;
        let db = test_db().await;
        let registry = AiProviderRegistry::new(db.clone());

        // The vehicle check must fire before provider resolution, so this 404s
        // even with no AI provider configured.
        assert!(matches!(
            suggestions(&db, &registry, 999, None).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn chat_wrong_vehicle_conversation_is_not_found() {
        use crate::test_support::test_db;
        let db = test_db().await;
        let registry = AiProviderRegistry::new(db.clone());
        let config = AppConfig {
            db_path: "unused".into(),
            listen: "unused".into(),
            files_dir: "unused".into(),
        };

        use crate::entities::vehicle;
        let owner = vehicle::ActiveModel {
            name: Set("A".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        let other = vehicle::ActiveModel {
            name: Set("B".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        let convo = crate::services::conversation::create(&db, owner, None)
            .await
            .unwrap();

        // Chatting into another vehicle's conversation must be indistinguishable
        // from a nonexistent conversation (NotFound, not a BadRequest oracle).
        let err = chat(
            &db,
            &registry,
            &config,
            ChatInput {
                vehicle_id: Some(other),
                conversation_id: convo.id,
                message: "hi".into(),
                provider_id: None,
                document_id: None,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)));

        // No message was persisted to the conversation.
        let msgs = crate::services::conversation::messages(&db, owner, convo.id)
            .await
            .unwrap();
        assert!(msgs.is_empty());
    }

    #[tokio::test]
    async fn provider_crud_round_trip_and_default_switch() {
        use crate::test_support::test_db;
        let db = test_db().await;

        let a = create_provider(
            &db,
            NewProvider {
                name: "A".into(),
                provider_type: "claude".into(),
                api_key: Some("sk-x".into()),
                api_base: None,
                model: None,
                is_default: Some(true),
                enabled: Some(true),
            },
        )
        .await
        .unwrap();
        assert!(a.is_default);
        assert!(a.api_key_set);

        let b = create_provider(
            &db,
            NewProvider {
                name: "B".into(),
                provider_type: "openai_compat".into(),
                api_key: None,
                api_base: Some("http://localhost:11434/v1".into()),
                model: None,
                is_default: Some(true),
                enabled: Some(true),
            },
        )
        .await
        .unwrap();
        assert!(b.is_default);

        // Creating B as default cleared A's default flag
        let listed = list_providers(&db).await.unwrap();
        let a_now = listed.iter().find(|p| p.id == a.id).unwrap();
        assert!(!a_now.is_default);

        delete_provider(&db, a.id).await.unwrap();
        assert!(matches!(
            delete_provider(&db, a.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }
}
