use async_trait::async_trait;

use super::{AiError, AiProvider, AiRequest, AiResponse};

/// Provider used when AI is not configured. Returns `AiError::NotConfigured`
/// for all requests.
pub struct NoOpProvider;

#[async_trait]
impl AiProvider for NoOpProvider {
    async fn complete(&self, _request: AiRequest) -> Result<AiResponse, AiError> {
        Err(AiError::NotConfigured)
    }

    fn provider_name(&self) -> &str {
        "none"
    }

    fn is_configured(&self) -> bool {
        false
    }
}
