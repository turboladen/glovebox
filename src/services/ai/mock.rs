use async_trait::async_trait;
use std::sync::Mutex;

use super::{AiError, AiProvider, AiRequest, AiResponse};

/// Test-only provider that returns configurable responses and records calls.
pub struct MockProvider {
    response: Result<String, String>,
    calls: Mutex<Vec<AiRequest>>,
}

impl MockProvider {
    /// Create a mock that always returns the given content.
    pub fn new(response: &str) -> Self {
        Self {
            response: Ok(response.to_string()),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Create a mock that always returns an error.
    pub fn with_error(err: AiError) -> Self {
        Self {
            response: Err(err.to_string()),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Get a clone of all requests sent to this provider.
    pub fn calls(&self) -> Vec<AiRequest> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl AiProvider for MockProvider {
    async fn complete(&self, request: AiRequest) -> Result<AiResponse, AiError> {
        self.calls.lock().unwrap().push(request);

        match &self.response {
            Ok(content) => Ok(AiResponse {
                content: content.clone(),
                input_tokens: Some(10),
                output_tokens: Some(20),
            }),
            Err(msg) => Err(AiError::ProviderError(msg.clone())),
        }
    }

    fn provider_name(&self) -> &str {
        "mock"
    }

    fn is_configured(&self) -> bool {
        self.response.is_ok()
    }
}
