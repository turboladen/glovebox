use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::entities::ai_provider_config;

use super::{AiError, AiProvider};

/// Resolves AI providers on-demand from the database.
/// No caching — every call reads fresh from the DB so changes take effect immediately.
pub struct AiProviderRegistry {
    db: DatabaseConnection,
}

impl AiProviderRegistry {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// Resolve a provider by optional ID. If `None`, returns the default provider.
    pub async fn resolve(&self, provider_id: Option<i32>) -> Result<Box<dyn AiProvider>, AiError> {
        let row = match provider_id {
            Some(id) => ai_provider_config::Entity::find_by_id(id)
                .one(&self.db)
                .await
                .map_err(|e| AiError::ProviderError(format!("DB error: {e}")))?
                .ok_or_else(|| AiError::ProviderError(format!("AI provider {id} not found")))?,
            None => self.default_provider_row().await?,
        };

        if !row.enabled {
            return Err(AiError::ProviderError(format!(
                "AI provider '{}' is disabled",
                row.name
            )));
        }

        Ok(create_provider_from_row(&row))
    }

    /// Get the default provider row, or error if none is set.
    async fn default_provider_row(&self) -> Result<ai_provider_config::Model, AiError> {
        ai_provider_config::Entity::find()
            .filter(ai_provider_config::Column::IsDefault.eq(true))
            .filter(ai_provider_config::Column::Enabled.eq(true))
            .one(&self.db)
            .await
            .map_err(|e| AiError::ProviderError(format!("DB error: {e}")))?
            .ok_or(AiError::NotConfigured)
    }

    /// List all enabled providers.
    pub async fn list_enabled(&self) -> Result<Vec<ai_provider_config::Model>, AiError> {
        ai_provider_config::Entity::find()
            .filter(ai_provider_config::Column::Enabled.eq(true))
            .all(&self.db)
            .await
            .map_err(|e| AiError::ProviderError(format!("DB error: {e}")))
    }

    /// Check whether any provider is configured and enabled.
    pub async fn any_configured(&self) -> Result<bool, AiError> {
        let providers = self.list_enabled().await?;
        Ok(providers
            .iter()
            .any(|p| create_provider_from_row(p).is_configured()))
    }
}

/// Construct a concrete `AiProvider` from a database row.
pub fn create_provider_from_row(row: &ai_provider_config::Model) -> Box<dyn AiProvider> {
    // Build the settings HashMap that `create_provider` expects
    let mut settings = std::collections::HashMap::new();
    settings.insert("ai.provider".to_string(), row.provider_type.clone());

    match row.provider_type.as_str() {
        "claude" => {
            if let Some(ref key) = row.api_key {
                settings.insert("ai.claude_api_key".to_string(), key.clone());
            }
            if let Some(ref model) = row.model {
                settings.insert("ai.claude_model".to_string(), model.clone());
            }
        }
        "openai_compat" => {
            if let Some(ref base) = row.api_base {
                settings.insert("ai.openai_api_base".to_string(), base.clone());
            }
            if let Some(ref model) = row.model {
                settings.insert("ai.openai_model".to_string(), model.clone());
            }
            if let Some(ref key) = row.api_key {
                settings.insert("ai.openai_api_key".to_string(), key.clone());
            }
        }
        _ => {}
    }

    super::create_provider(&row.provider_type, &settings)
}
