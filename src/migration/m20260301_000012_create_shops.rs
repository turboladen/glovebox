use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Shops::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Shops::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Shops::Name).text().not_null())
                    .col(ColumnDef::new(Shops::Address).text())
                    .col(ColumnDef::new(Shops::Phone).text())
                    .col(ColumnDef::new(Shops::Website).text())
                    .col(ColumnDef::new(Shops::Specialty).text())
                    .col(ColumnDef::new(Shops::Notes).text())
                    .col(ColumnDef::new(Shops::CreatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .col(ColumnDef::new(Shops::UpdatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .to_owned(),
            )
            .await?;

        // Add shop_id FK to service_records
        manager
            .alter_table(
                Table::alter()
                    .table(ServiceRecords::Table)
                    .add_column(ColumnDef::new(ServiceRecords::ShopId).integer())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite doesn't support DROP COLUMN in older versions, but SeaORM handles it
        manager
            .alter_table(
                Table::alter()
                    .table(ServiceRecords::Table)
                    .drop_column(ServiceRecords::ShopId)
                    .to_owned(),
            )
            .await?;
        manager.drop_table(Table::drop().table(Shops::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Shops {
    Table,
    Id,
    Name,
    Address,
    Phone,
    Website,
    Specialty,
    Notes,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum ServiceRecords {
    Table,
    ShopId,
}
