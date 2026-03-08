use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Documents::Table).to_owned()).await
    }
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
enum Vehicles {
    Table,
    Id,
}
