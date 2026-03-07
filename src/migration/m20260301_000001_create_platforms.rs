use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Platforms::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Platforms::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Platforms::Name).text().not_null())
                    .col(ColumnDef::new(Platforms::WebsiteUrl).text())
                    .col(ColumnDef::new(Platforms::ApiBaseUrl).text())
                    .col(ColumnDef::new(Platforms::Notes).text())
                    .col(ColumnDef::new(Platforms::CreatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .col(ColumnDef::new(Platforms::UpdatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Platforms::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Platforms {
    Table,
    Id,
    Name,
    WebsiteUrl,
    ApiBaseUrl,
    Notes,
    CreatedAt,
    UpdatedAt,
}
