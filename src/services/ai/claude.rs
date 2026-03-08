use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};

use super::{AiError, AiProvider, AiRequest, AiResponse};

const CLAUDE_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct ClaudeProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl ClaudeProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }

    fn build_request_body(&self, request: &AiRequest) -> ClaudeRequestBody {
        let messages: Vec<ClaudeMessage> = request
            .messages
            .iter()
            .map(|msg| {
                let role = match msg.role {
                    super::Role::User => "user".to_string(),
                    super::Role::Assistant => "assistant".to_string(),
                    super::Role::System => "user".to_string(), // system handled separately
                };
                ClaudeMessage {
                    role,
                    content: vec![ClaudeContentBlock::Text {
                        text: msg.content.clone(),
                    }],
                }
            })
            .collect();

        // If there are attachments, prepend them to the first user message's content,
        // or create a new user message if none exist.
        let messages = if request.attachments.is_empty() {
            messages
        } else {
            let mut attachment_blocks: Vec<ClaudeContentBlock> = request
                .attachments
                .iter()
                .map(|att| ClaudeContentBlock::Document {
                    source: DocumentSource {
                        source_type: "base64".to_string(),
                        media_type: att.mime_type.clone(),
                        data: STANDARD.encode(&att.data),
                    },
                })
                .collect();

            let mut messages = messages;
            if let Some(first_user) = messages.iter_mut().find(|m| m.role == "user") {
                let mut new_content = Vec::new();
                new_content.append(&mut attachment_blocks);
                new_content.append(&mut first_user.content);
                first_user.content = new_content;
            } else {
                messages.insert(
                    0,
                    ClaudeMessage {
                        role: "user".to_string(),
                        content: attachment_blocks,
                    },
                );
            }
            messages
        };

        let system = if request.system_prompt.is_empty() {
            None
        } else {
            Some(request.system_prompt.clone())
        };

        ClaudeRequestBody {
            model: self.model.clone(),
            max_tokens: request.max_tokens.unwrap_or(4096),
            system,
            messages,
        }
    }
}

#[async_trait]
impl AiProvider for ClaudeProvider {
    async fn complete(&self, request: AiRequest) -> Result<AiResponse, AiError> {
        if !self.is_configured() {
            return Err(AiError::NotConfigured);
        }

        let body = self.build_request_body(&request);

        let resp = self
            .client
            .post(CLAUDE_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AiError::ProviderError(format!("Request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "unable to read response body".to_string());
            tracing::error!("Claude API returned status {status}: {body_text}");
            return Err(AiError::ProviderError(format!(
                "Claude API returned status {status}"
            )));
        }

        let claude_resp: ClaudeApiResponse = resp
            .json()
            .await
            .map_err(|e| AiError::ParseError(format!("Failed to parse Claude response: {e}")))?;

        let content = claude_resp
            .content
            .into_iter()
            .filter_map(|block| match block {
                ClaudeResponseBlock::Text { text } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");

        Ok(AiResponse {
            content,
            input_tokens: Some(claude_resp.usage.input_tokens),
            output_tokens: Some(claude_resp.usage.output_tokens),
        })
    }

    fn provider_name(&self) -> &str {
        "claude"
    }

    fn is_configured(&self) -> bool {
        !self.api_key.is_empty()
    }
}

// --- Claude API types ---

#[derive(Debug, Serialize)]
struct ClaudeRequestBody {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<ClaudeMessage>,
}

#[derive(Debug, Serialize)]
struct ClaudeMessage {
    role: String,
    content: Vec<ClaudeContentBlock>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum ClaudeContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "document")]
    Document { source: DocumentSource },
}

#[derive(Debug, Serialize)]
struct DocumentSource {
    #[serde(rename = "type")]
    source_type: String,
    media_type: String,
    data: String,
}

#[derive(Debug, Deserialize)]
struct ClaudeApiResponse {
    content: Vec<ClaudeResponseBlock>,
    usage: ClaudeUsage,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClaudeResponseBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct ClaudeUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::{Attachment, ChatMessage, Role};

    #[test]
    fn provider_name_returns_claude() {
        let provider = ClaudeProvider::new("key".into(), "model".into());
        assert_eq!(provider.provider_name(), "claude");
    }

    #[test]
    fn is_configured_with_api_key() {
        let provider = ClaudeProvider::new("sk-test-key".into(), "model".into());
        assert!(provider.is_configured());
    }

    #[test]
    fn is_not_configured_with_empty_key() {
        let provider = ClaudeProvider::new("".into(), "model".into());
        assert!(!provider.is_configured());
    }

    #[test]
    fn request_body_basic_message() {
        let provider = ClaudeProvider::new("key".into(), "claude-sonnet-4-6".into());
        let request = AiRequest {
            system_prompt: "You are a car expert.".into(),
            messages: vec![
                ChatMessage {
                    role: Role::User,
                    content: "What oil do I need?".into(),
                },
                ChatMessage {
                    role: Role::Assistant,
                    content: "It depends on your car.".into(),
                },
            ],
            attachments: vec![],
            max_tokens: Some(1024),
        };

        let body = provider.build_request_body(&request);
        let json = serde_json::to_value(&body).unwrap();

        assert_eq!(json["model"], "claude-sonnet-4-6");
        assert_eq!(json["max_tokens"], 1024);
        assert_eq!(json["system"], "You are a car expert.");
        assert_eq!(json["messages"].as_array().unwrap().len(), 2);
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"][0]["type"], "text");
        assert_eq!(json["messages"][0]["content"][0]["text"], "What oil do I need?");
        assert_eq!(json["messages"][1]["role"], "assistant");
    }

    #[test]
    fn request_body_no_system_prompt() {
        let provider = ClaudeProvider::new("key".into(), "model".into());
        let request = AiRequest {
            system_prompt: "".into(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: "hello".into(),
            }],
            attachments: vec![],
            max_tokens: None,
        };

        let body = provider.build_request_body(&request);
        let json = serde_json::to_value(&body).unwrap();

        // system field should be absent when empty
        assert!(json.get("system").is_none());
        // default max_tokens
        assert_eq!(json["max_tokens"], 4096);
    }

    #[test]
    fn request_body_with_attachments() {
        let provider = ClaudeProvider::new("key".into(), "model".into());
        let request = AiRequest {
            system_prompt: "".into(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: "What is this document about?".into(),
            }],
            attachments: vec![Attachment {
                mime_type: "application/pdf".into(),
                data: b"fake pdf data".to_vec(),
            }],
            max_tokens: None,
        };

        let body = provider.build_request_body(&request);
        let json = serde_json::to_value(&body).unwrap();

        let content = json["messages"][0]["content"].as_array().unwrap();
        // attachment comes before text
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "document");
        assert_eq!(content[0]["source"]["type"], "base64");
        assert_eq!(content[0]["source"]["media_type"], "application/pdf");
        // Verify data is base64-encoded
        let encoded = content[0]["source"]["data"].as_str().unwrap();
        let decoded = STANDARD.decode(encoded).unwrap();
        assert_eq!(decoded, b"fake pdf data");
        // text follows
        assert_eq!(content[1]["type"], "text");
        assert_eq!(content[1]["text"], "What is this document about?");
    }

    #[test]
    fn response_deserialization() {
        let json = r#"{
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "The answer is 42."}
            ],
            "model": "claude-sonnet-4-6",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        }"#;

        let resp: ClaudeApiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 5);
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            ClaudeResponseBlock::Text { text } => assert_eq!(text, "The answer is 42."),
            _ => panic!("Expected text block"),
        }
    }

    #[tokio::test]
    async fn complete_returns_not_configured_when_empty_key() {
        let provider = ClaudeProvider::new("".into(), "model".into());
        let req = AiRequest {
            system_prompt: "test".into(),
            messages: vec![],
            attachments: vec![],
            max_tokens: None,
        };
        let result = provider.complete(req).await;
        assert!(matches!(result.unwrap_err(), AiError::NotConfigured));
    }
}
