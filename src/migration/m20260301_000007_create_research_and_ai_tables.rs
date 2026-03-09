use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // research_reports
        manager
            .create_table(
                Table::create()
                    .table(ResearchReports::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ResearchReports::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(ResearchReports::VehicleId).integer().not_null())
                    .col(ColumnDef::new(ResearchReports::ReportType).text())
                    .col(ColumnDef::new(ResearchReports::Summary).text())
                    .col(ColumnDef::new(ResearchReports::RawData).text())
                    .col(ColumnDef::new(ResearchReports::Notes).text())
                    .col(ColumnDef::new(ResearchReports::GeneratedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .col(ColumnDef::new(ResearchReports::CreatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .foreign_key(
                        ForeignKey::create()
                            .from(ResearchReports::Table, ResearchReports::VehicleId)
                            .to(Vehicles::Table, Vehicles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .table(ResearchReports::Table)
                    .name("idx_research_reports_vehicle")
                    .col(ResearchReports::VehicleId)
                    .to_owned(),
            )
            .await?;

        // research_findings
        manager
            .create_table(
                Table::create()
                    .table(ResearchFindings::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ResearchFindings::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(ResearchFindings::ReportId).integer().not_null())
                    .col(ColumnDef::new(ResearchFindings::Category).text().not_null())
                    .col(ColumnDef::new(ResearchFindings::Title).text().not_null())
                    .col(ColumnDef::new(ResearchFindings::Description).text())
                    .col(ColumnDef::new(ResearchFindings::SourceUrl).text())
                    .col(ColumnDef::new(ResearchFindings::Severity).text())
                    .col(ColumnDef::new(ResearchFindings::Status).text().not_null().default("new"))
                    .col(ColumnDef::new(ResearchFindings::LinkedEntityType).text())
                    .col(ColumnDef::new(ResearchFindings::LinkedEntityId).integer())
                    .col(ColumnDef::new(ResearchFindings::CreatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .col(ColumnDef::new(ResearchFindings::UpdatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .foreign_key(
                        ForeignKey::create()
                            .from(ResearchFindings::Table, ResearchFindings::ReportId)
                            .to(ResearchReports::Table, ResearchReports::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .table(ResearchFindings::Table)
                    .name("idx_research_findings_report")
                    .col(ResearchFindings::ReportId)
                    .to_owned(),
            )
            .await?;

        // ai_providers
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
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(AiProviders::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(ResearchFindings::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(ResearchReports::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum ResearchReports {
    Table,
    Id,
    VehicleId,
    ReportType,
    Summary,
    RawData,
    Notes,
    GeneratedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum ResearchFindings {
    Table,
    Id,
    ReportId,
    Category,
    Title,
    Description,
    SourceUrl,
    Severity,
    Status,
    LinkedEntityType,
    LinkedEntityId,
    CreatedAt,
    UpdatedAt,
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

#[derive(DeriveIden)]
enum Vehicles {
    Table,
    Id,
}
