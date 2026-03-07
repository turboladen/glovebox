use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ModelTemplates::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ModelTemplates::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(ModelTemplates::PlatformId).integer())
                    .col(ColumnDef::new(ModelTemplates::PlatformRef).text())
                    .col(ColumnDef::new(ModelTemplates::Year).integer())
                    .col(ColumnDef::new(ModelTemplates::Make).text())
                    .col(ColumnDef::new(ModelTemplates::Model).text())
                    .col(ColumnDef::new(ModelTemplates::TrimLevel).text())
                    .col(ColumnDef::new(ModelTemplates::BodyStyle).text())
                    .col(ColumnDef::new(ModelTemplates::Engine).text())
                    .col(ColumnDef::new(ModelTemplates::Transmission).text())
                    .col(ColumnDef::new(ModelTemplates::Drivetrain).text())
                    .col(ColumnDef::new(ModelTemplates::CreatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .col(ColumnDef::new(ModelTemplates::UpdatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .foreign_key(
                        ForeignKey::create()
                            .from(ModelTemplates::Table, ModelTemplates::PlatformId)
                            .to(Platforms::Table, Platforms::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(ModelTemplates::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum ModelTemplates {
    Table,
    Id,
    PlatformId,
    PlatformRef,
    Year,
    Make,
    Model,
    TrimLevel,
    BodyStyle,
    Engine,
    Transmission,
    Drivetrain,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Platforms {
    Table,
    Id,
}
