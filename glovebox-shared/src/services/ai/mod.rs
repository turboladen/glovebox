pub mod claude;
pub mod context;
pub mod noop;
pub mod openai_compat;
pub mod registry;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A message in a conversation with an AI provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

/// A binary attachment (PDF, image) to send alongside a message.
#[derive(Debug, Clone)]
pub struct Attachment {
    pub mime_type: String,
    pub data: Vec<u8>,
}

/// Request to an AI provider.
#[derive(Debug, Clone)]
pub struct AiRequest {
    pub system_prompt: String,
    pub messages: Vec<ChatMessage>,
    pub attachments: Vec<Attachment>,
    pub max_tokens: Option<u32>,
}

/// Response from an AI provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiResponse {
    pub content: String,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("AI is not configured")]
    NotConfigured,
    #[error("AI provider error: {0}")]
    ProviderError(String),
    #[error("Failed to parse AI response: {0}")]
    ParseError(String),
}

/// Pluggable AI provider trait. Implementations exist for Claude API,
/// OpenAI-compatible APIs (Ollama, LM Studio, etc.), and test mocks.
#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn complete(&self, request: AiRequest) -> Result<AiResponse, AiError>;

    /// Human-readable provider name (e.g. "claude", "`openai_compat`", "none").
    fn provider_name(&self) -> &str;

    /// Whether the provider is properly configured and ready to use.
    fn is_configured(&self) -> bool;
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

/// Construct the appropriate AI provider based on the `ai.provider` setting value.
/// Additional settings (API keys, URLs, models) are passed as key-value pairs.
pub fn create_provider(
    provider_name: &str,
    settings: &std::collections::HashMap<String, String>,
) -> Box<dyn AiProvider> {
    match provider_name {
        "none" | "" => Box::new(noop::NoOpProvider),
        "claude" => {
            let api_key = settings
                .get("ai.claude_api_key")
                .cloned()
                .unwrap_or_default();
            let model = settings
                .get("ai.claude_model")
                .cloned()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "claude-sonnet-4-6".to_string());
            Box::new(claude::ClaudeProvider::new(api_key, model))
        }
        "openai_compat" => {
            let api_base = settings
                .get("ai.openai_api_base")
                .cloned()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "http://localhost:11434/v1".to_string());
            let model = settings
                .get("ai.openai_model")
                .cloned()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "llama3".to_string());
            let api_key = settings
                .get("ai.openai_api_key")
                .cloned()
                .filter(|s| !s.is_empty());
            Box::new(openai_compat::OpenAiCompatProvider::new(
                api_base, model, api_key,
            ))
        }
        _ => {
            tracing::warn!(
                "Unknown AI provider '{}', falling back to none",
                provider_name
            );
            Box::new(noop::NoOpProvider)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn create_provider_none() {
        let settings = HashMap::new();
        let provider = create_provider("none", &settings);
        assert_eq!(provider.provider_name(), "none");
        assert!(!provider.is_configured());
    }

    #[test]
    fn create_provider_empty_string() {
        let settings = HashMap::new();
        let provider = create_provider("", &settings);
        assert_eq!(provider.provider_name(), "none");
    }

    #[test]
    fn create_provider_unknown_falls_back() {
        let settings = HashMap::new();
        let provider = create_provider("nonexistent", &settings);
        assert_eq!(provider.provider_name(), "none");
    }

    #[test]
    fn strip_code_fences_json_block() {
        let input = "```json\n[{\"title\": \"test\"}]\n```";
        assert_eq!(strip_code_fences(input), "[{\"title\": \"test\"}]");
    }

    #[test]
    fn strip_code_fences_plain_block() {
        let input = "```\n[{\"title\": \"test\"}]\n```";
        assert_eq!(strip_code_fences(input), "[{\"title\": \"test\"}]");
    }

    #[test]
    fn strip_code_fences_no_fences() {
        let input = "[{\"title\": \"test\"}]";
        assert_eq!(strip_code_fences(input), "[{\"title\": \"test\"}]");
    }

    #[test]
    fn strip_code_fences_with_whitespace() {
        let input = "  ```json\n[{\"title\": \"test\"}]\n```  ";
        assert_eq!(strip_code_fences(input), "[{\"title\": \"test\"}]");
    }

    #[tokio::test]
    async fn noop_returns_not_configured() {
        let provider = noop::NoOpProvider;
        let req = AiRequest {
            system_prompt: "test".into(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: "hello".into(),
            }],
            attachments: vec![],
            max_tokens: None,
        };
        let result = provider.complete(req).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AiError::NotConfigured));
    }
}
