use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // observations
        manager
            .create_table(
                Table::create()
                    .table(Observations::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Observations::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Observations::VehicleId).integer().not_null())
                    .col(ColumnDef::new(Observations::Category).text().not_null())
                    .col(ColumnDef::new(Observations::Title).text().not_null())
                    .col(ColumnDef::new(Observations::Description).text())
                    .col(ColumnDef::new(Observations::Odometer).integer())
                    .col(ColumnDef::new(Observations::ObservedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .col(ColumnDef::new(Observations::ObdCodes).text())
                    .col(ColumnDef::new(Observations::Resolved).boolean().not_null().default(false))
                    .col(ColumnDef::new(Observations::ResolvedServiceId).integer())
                    .col(ColumnDef::new(Observations::Notes).text())
                    .col(ColumnDef::new(Observations::CreatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .col(ColumnDef::new(Observations::UpdatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .foreign_key(
                        ForeignKey::create()
                            .from(Observations::Table, Observations::VehicleId)
                            .to(Vehicles::Table, Vehicles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Observations::Table, Observations::ResolvedServiceId)
                            .to(ServiceRecords::Table, ServiceRecords::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        // documents
        manager
            .create_table(
                Table::create()
                    .table(Documents::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Documents::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Documents::VehicleId).integer())
                    .col(ColumnDef::new(Documents::Title).text().not_null())
                    .col(ColumnDef::new(Documents::FilePath).text().not_null())
                    .col(ColumnDef::new(Documents::FileName).text().not_null())
                    .col(ColumnDef::new(Documents::MimeType).text())
                    .col(ColumnDef::new(Documents::FileSizeBytes).integer())
                    .col(ColumnDef::new(Documents::DocType).text())
                    .col(ColumnDef::new(Documents::LinkedEntityType).text())
                    .col(ColumnDef::new(Documents::LinkedEntityId).integer())
                    .col(ColumnDef::new(Documents::Notes).text())
                    .col(ColumnDef::new(Documents::ExtractedText).text())
                    .col(ColumnDef::new(Documents::CreatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .foreign_key(
                        ForeignKey::create()
                            .from(Documents::Table, Documents::VehicleId)
                            .to(Vehicles::Table, Vehicles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // chat_messages
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
        manager.drop_table(Table::drop().table(ChatMessages::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Documents::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Observations::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Observations {
    Table,
    Id,
    VehicleId,
    Category,
    Title,
    Description,
    Odometer,
    ObservedAt,
    ObdCodes,
    Resolved,
    ResolvedServiceId,
    Notes,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Documents {
    Table,
    Id,
    VehicleId,
    Title,
    FilePath,
    FileName,
    MimeType,
    FileSizeBytes,
    DocType,
    LinkedEntityType,
    LinkedEntityId,
    Notes,
    ExtractedText,
    CreatedAt,
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

#[derive(DeriveIden)]
enum ServiceRecords {
    Table,
    Id,
}
