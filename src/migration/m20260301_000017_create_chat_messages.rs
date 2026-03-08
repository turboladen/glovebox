use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ChatMessages::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ChatMessages::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(ChatMessages::VehicleId).integer())
                    .col(ColumnDef::new(ChatMessages::Role).text().not_null())
                    .col(ColumnDef::new(ChatMessages::Content).text().not_null())
                    .col(ColumnDef::new(ChatMessages::CreatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .foreign_key(
                        ForeignKey::create()
                            .from(ChatMessages::Table, ChatMessages::VehicleId)
                            .to(Vehicles::Table, Vehicles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .table(ChatMessages::Table)
                    .name("idx_chat_messages_vehicle")
                    .col(ChatMessages::VehicleId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(ChatMessages::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum ChatMessages {
    Table,
    Id,
    VehicleId,
    Role,
    Content,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Vehicles {
    Table,
    Id,
}
