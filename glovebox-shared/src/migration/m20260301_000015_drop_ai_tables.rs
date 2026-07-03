//! Drops the in-app AI tables (2hea unit A). The app never shipped; the data
//! is explicitly not worth keeping (chat history, provider configs).

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        // Children before parents (chat_messages FKs conversations).
        db.execute_unprepared("DROP TABLE IF EXISTS chat_messages")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS conversations")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS ai_providers")
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Intentionally irreversible: the feature is retired, not versioned.
        Err(DbErr::Migration(
            "2hea unit A drops the AI tables permanently; restore from a DB backup instead".into(),
        ))
    }
}
