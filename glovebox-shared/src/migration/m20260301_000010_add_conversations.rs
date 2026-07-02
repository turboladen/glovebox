use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create conversations table
        manager
            .create_table(
                Table::create()
                    .table(Conversations::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Conversations::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Conversations::VehicleId).integer())
                    .col(
                        ColumnDef::new(Conversations::Title)
                            .text()
                            .not_null()
                            .default("New Chat"),
                    )
                    .col(
                        ColumnDef::new(Conversations::CreatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .col(
                        ColumnDef::new(Conversations::UpdatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Conversations::Table, Conversations::VehicleId)
                            .to(Vehicles::Table, Vehicles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .table(Conversations::Table)
                    .name("idx_conversations_vehicle")
                    .col(Conversations::VehicleId)
                    .to_owned(),
            )
            .await?;

        // Add conversation_id column to chat_messages
        // (SQLite ALTER TABLE ADD COLUMN appends after existing columns)
        manager
            .alter_table(
                Table::alter()
                    .table(ChatMessages::Table)
                    .add_column(ColumnDef::new(ChatMessages::ConversationId).integer())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .table(ChatMessages::Table)
                    .name("idx_chat_messages_conversation")
                    .col(ChatMessages::ConversationId)
                    .to_owned(),
            )
            .await?;

        // Data migration: create a "Previous Chat" conversation for each distinct
        // vehicle_id that has existing chat messages, then backfill conversation_id.
        let db = manager.get_connection();

        // Create conversations for each vehicle with messages
        db.execute_unprepared(
            "INSERT INTO conversations (vehicle_id, title, created_at, updated_at)
             SELECT DISTINCT vehicle_id, 'Previous Chat',
                    MIN(created_at), MAX(created_at)
             FROM chat_messages
             GROUP BY vehicle_id",
        )
        .await?;

        // Backfill conversation_id on existing messages
        db.execute_unprepared(
            "UPDATE chat_messages
             SET conversation_id = (
                 SELECT c.id FROM conversations c
                 WHERE c.title = 'Previous Chat'
                   AND (c.vehicle_id = chat_messages.vehicle_id
                        OR (c.vehicle_id IS NULL AND chat_messages.vehicle_id IS NULL))
             )
             WHERE conversation_id IS NULL",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite can't drop columns, so we recreate chat_messages without conversation_id
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE chat_messages_backup (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                vehicle_id INTEGER,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                FOREIGN KEY (vehicle_id) REFERENCES vehicles(id) ON DELETE CASCADE
            )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO chat_messages_backup (id, vehicle_id, role, content, created_at)
             SELECT id, vehicle_id, role, content, created_at FROM chat_messages",
        )
        .await?;

        manager
            .drop_table(Table::drop().table(ChatMessages::Table).to_owned())
            .await?;

        db.execute_unprepared("ALTER TABLE chat_messages_backup RENAME TO chat_messages")
            .await?;

        manager
            .create_index(
                Index::create()
                    .table(ChatMessages::Table)
                    .name("idx_chat_messages_vehicle")
                    .col(ChatMessages::VehicleId)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(Conversations::Table).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Conversations {
    Table,
    Id,
    VehicleId,
    Title,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum ChatMessages {
    Table,
    ConversationId,
    VehicleId,
}

#[derive(DeriveIden)]
enum Vehicles {
    Table,
    Id,
}
