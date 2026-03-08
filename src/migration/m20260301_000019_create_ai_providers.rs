use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AiProviders::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(AiProviders::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(AiProviders::Name).text().not_null())
                    .col(ColumnDef::new(AiProviders::ProviderType).text().not_null())
                    .col(ColumnDef::new(AiProviders::ApiKey).text())
                    .col(ColumnDef::new(AiProviders::ApiBase).text())
                    .col(ColumnDef::new(AiProviders::Model).text())
                    .col(ColumnDef::new(AiProviders::IsDefault).boolean().not_null().default(false))
                    .col(ColumnDef::new(AiProviders::Enabled).boolean().not_null().default(true))
                    .col(ColumnDef::new(AiProviders::CreatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .col(ColumnDef::new(AiProviders::UpdatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .to_owned(),
            )
            .await?;

        // Auto-migrate existing ai.* settings into the new table.
        // Uses raw SQL to read from settings and INSERT conditionally.
        let db = manager.get_connection();

        db.execute_unprepared(
            r#"
            INSERT INTO ai_providers (name, provider_type, api_key, api_base, model, is_default, enabled)
            SELECT
                CASE
                    WHEN s_provider.value = 'claude' THEN 'Claude'
                    WHEN s_provider.value = 'openai_compat' THEN 'Local LLM'
                    ELSE s_provider.value
                END,
                s_provider.value,
                COALESCE(
                    CASE WHEN s_provider.value = 'claude' THEN s_claude_key.value
                         WHEN s_provider.value = 'openai_compat' THEN s_openai_key.value
                    END,
                    NULL
                ),
                CASE WHEN s_provider.value = 'openai_compat' THEN s_openai_base.value
                     ELSE NULL
                END,
                CASE WHEN s_provider.value = 'claude' THEN COALESCE(s_claude_model.value, 'claude-sonnet-4-6')
                     WHEN s_provider.value = 'openai_compat' THEN COALESCE(s_openai_model.value, 'llama3')
                     ELSE NULL
                END,
                1,
                1
            FROM settings s_provider
            LEFT JOIN settings s_claude_key ON s_claude_key.key = 'ai.claude_api_key'
            LEFT JOIN settings s_claude_model ON s_claude_model.key = 'ai.claude_model'
            LEFT JOIN settings s_openai_key ON s_openai_key.key = 'ai.openai_api_key'
            LEFT JOIN settings s_openai_base ON s_openai_base.key = 'ai.openai_api_base'
            LEFT JOIN settings s_openai_model ON s_openai_model.key = 'ai.openai_model'
            WHERE s_provider.key = 'ai.provider'
              AND s_provider.value != 'none'
              AND s_provider.value != ''
              AND NOT EXISTS (SELECT 1 FROM ai_providers)
            "#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AiProviders::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum AiProviders {
    Table,
    Id,
    Name,
    ProviderType,
    ApiKey,
    ApiBase,
    Model,
    IsDefault,
    Enabled,
    CreatedAt,
    UpdatedAt,
}
