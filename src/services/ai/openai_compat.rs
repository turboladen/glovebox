use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{AiError, AiProvider, AiRequest, AiResponse, Role};

/// An AI provider that talks to any OpenAI-compatible chat completions API
/// (e.g. Ollama, LM Studio, vLLM, or OpenAI itself).
pub struct OpenAiCompatProvider {
    api_base: String,
    model: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct CompletionRequest {
    model: String,
    messages: Vec<ApiMessage>,
    max_tokens: u32,
}

#[derive(Debug, Serialize)]
struct ApiMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct CompletionResponse {
    choices: Vec<Choice>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
}

impl OpenAiCompatProvider {
    pub fn new(api_base: String, model: String, api_key: Option<String>) -> Self {
        Self {
            api_base,
            model,
            api_key,
            client: reqwest::Client::new(),
        }
    }

    fn build_messages(&self, request: &AiRequest) -> Vec<ApiMessage> {
        let mut messages = Vec::new();

        // System prompt as first message
        if !request.system_prompt.is_empty() {
            messages.push(ApiMessage {
                role: "system".to_string(),
                content: request.system_prompt.clone(),
            });
        }

        // Convert chat messages, appending attachment notes to user messages
        for msg in &request.messages {
            let role = match msg.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
            };

            let mut content = msg.content.clone();

            // For user messages, append attachment info if present
            if msg.role == Role::User && !request.attachments.is_empty() {
                for attachment in &request.attachments {
                    content.push_str(&format!(
                        "\n[Attachment: {}, {} bytes — content not available for this provider]",
                        attachment.mime_type,
                        attachment.data.len()
                    ));
                }
            }

            messages.push(ApiMessage {
                role: role.to_string(),
                content,
            });
        }

        messages
    }
}

#[async_trait]
impl AiProvider for OpenAiCompatProvider {
    async fn complete(&self, request: AiRequest) -> Result<AiResponse, AiError> {
        if !self.is_configured() {
            return Err(AiError::NotConfigured);
        }

        let max_tokens = request.max_tokens.unwrap_or(4096);
        let api_messages = self.build_messages(&request);

        let body = CompletionRequest {
            model: self.model.clone(),
            messages: api_messages,
            max_tokens,
        };

        let url = format!("{}/chat/completions", self.api_base.trim_end_matches('/'));

        let mut req = self
            .client
            .post(&url)
            .header("content-type", "application/json");

        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }

        let resp = req
            .json(&body)
            .send()
            .await
            .map_err(|e| AiError::ProviderError(format!("Request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "unable to read body".to_string());
            tracing::error!("OpenAI-compatible API returned {status}: {body_text}");
            return Err(AiError::ProviderError(format!(
                "AI provider returned status {status}"
            )));
        }

        let data: CompletionResponse = resp
            .json()
            .await
            .map_err(|e| AiError::ParseError(format!("Failed to parse response: {e}")))?;

        let content = data
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or_else(|| AiError::ParseError("No content in response".to_string()))?;

        let (input_tokens, output_tokens) = match data.usage {
            Some(u) => (u.prompt_tokens, u.completion_tokens),
            None => (None, None),
        };

        Ok(AiResponse {
            content,
            input_tokens,
            output_tokens,
        })
    }

    fn provider_name(&self) -> &str {
        "openai_compat"
    }

    fn is_configured(&self) -> bool {
        !self.model.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::{Attachment, ChatMessage};

    #[test]
    fn provider_name_returns_openai_compat() {
        let provider = OpenAiCompatProvider::new(
            "http://localhost:11434/v1".to_string(),
            "llama3".to_string(),
            None,
        );
        assert_eq!(provider.provider_name(), "openai_compat");
    }

    #[test]
    fn is_configured_with_model() {
        let provider = OpenAiCompatProvider::new(
            "http://localhost:11434/v1".to_string(),
            "llama3".to_string(),
            None,
        );
        assert!(provider.is_configured());
    }

    #[test]
    fn is_configured_empty_model() {
        let provider = OpenAiCompatProvider::new(
            "http://localhost:11434/v1".to_string(),
            "".to_string(),
            None,
        );
        assert!(!provider.is_configured());
    }

    #[test]
    fn request_body_serialization() {
        let provider = OpenAiCompatProvider::new(
            "http://localhost:11434/v1".to_string(),
            "llama3".to_string(),
            None,
        );

        let request = AiRequest {
            system_prompt: "You are helpful.".to_string(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: "Hello".to_string(),
            }],
            attachments: vec![],
            max_tokens: Some(1024),
        };

        let messages = provider.build_messages(&request);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[0].content, "You are helpful.");
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[1].content, "Hello");

        // Verify full body serialization
        let body = CompletionRequest {
            model: provider.model.clone(),
            messages,
            max_tokens: 1024,
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["model"], "llama3");
        assert_eq!(json["max_tokens"], 1024);
        assert_eq!(json["messages"][0]["role"], "system");
        assert_eq!(json["messages"][1]["role"], "user");
    }

    #[test]
    fn request_body_with_attachments() {
        let provider = OpenAiCompatProvider::new(
            "http://localhost:11434/v1".to_string(),
            "llama3".to_string(),
            None,
        );

        let request = AiRequest {
            system_prompt: "sys".to_string(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: "Check this".to_string(),
            }],
            attachments: vec![Attachment {
                mime_type: "application/pdf".to_string(),
                data: vec![0u8; 100],
            }],
            max_tokens: None,
        };

        let messages = provider.build_messages(&request);
        assert_eq!(messages.len(), 2);
        assert!(messages[1]
            .content
            .contains("[Attachment: application/pdf, 100 bytes"));
    }

    #[test]
    fn authorization_header_only_with_api_key() {
        // With API key
        let provider = OpenAiCompatProvider::new(
            "http://localhost:11434/v1".to_string(),
            "llama3".to_string(),
            Some("sk-test-key".to_string()),
        );

        let req = provider
            .client
            .post("http://localhost/test")
            .header("content-type", "application/json");
        let req_with_auth = req.header("Authorization", format!("Bearer {}", "sk-test-key"));
        let built = req_with_auth.build().unwrap();
        assert_eq!(
            built.headers().get("Authorization").unwrap(),
            "Bearer sk-test-key"
        );

        // Without API key — no Authorization header
        let provider_no_key = OpenAiCompatProvider::new(
            "http://localhost:11434/v1".to_string(),
            "llama3".to_string(),
            None,
        );
        assert!(provider_no_key.api_key.is_none());

        let req_no_auth = provider_no_key
            .client
            .post("http://localhost/test")
            .header("content-type", "application/json")
            .build()
            .unwrap();
        assert!(req_no_auth.headers().get("Authorization").is_none());
    }

    #[test]
    fn empty_system_prompt_omitted() {
        let provider = OpenAiCompatProvider::new(
            "http://localhost:11434/v1".to_string(),
            "llama3".to_string(),
            None,
        );

        let request = AiRequest {
            system_prompt: "".to_string(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: "Hello".to_string(),
            }],
            attachments: vec![],
            max_tokens: None,
        };

        let messages = provider.build_messages(&request);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
    }

    #[tokio::test]
    async fn complete_returns_not_configured_when_model_empty() {
        let provider = OpenAiCompatProvider::new(
            "http://localhost:11434/v1".to_string(),
            "".to_string(),
            None,
        );

        let request = AiRequest {
            system_prompt: "sys".to_string(),
            messages: vec![],
            attachments: vec![],
            max_tokens: None,
        };

        let result = provider.complete(request).await;
        assert!(matches!(result.unwrap_err(), AiError::NotConfigured));
    }
}
