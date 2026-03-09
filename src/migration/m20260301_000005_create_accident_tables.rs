use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // accidents
        manager
            .create_table(
                Table::create()
                    .table(Accidents::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Accidents::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Accidents::VehicleId).integer().not_null())
                    .col(ColumnDef::new(Accidents::OccurredAt).text().not_null())
                    .col(ColumnDef::new(Accidents::Odometer).integer())
                    .col(ColumnDef::new(Accidents::Description).text().not_null())
                    .col(ColumnDef::new(Accidents::Fault).text())
                    .col(ColumnDef::new(Accidents::OtherPartyName).text())
                    .col(ColumnDef::new(Accidents::OtherPartyPhone).text())
                    .col(ColumnDef::new(Accidents::OtherPartyEmail).text())
                    .col(ColumnDef::new(Accidents::OtherPartyInsurance).text())
                    .col(ColumnDef::new(Accidents::OtherPartyPolicyNumber).text())
                    .col(ColumnDef::new(Accidents::InsuranceClaimNumber).text())
                    .col(ColumnDef::new(Accidents::InsuranceAdjuster).text())
                    .col(ColumnDef::new(Accidents::InsuranceAdjusterPhone).text())
                    .col(ColumnDef::new(Accidents::TotalRepairCostCents).integer())
                    .col(ColumnDef::new(Accidents::TotalRepairCostCurrency).text())
                    .col(ColumnDef::new(Accidents::DeductibleCents).integer())
                    .col(ColumnDef::new(Accidents::DeductibleCurrency).text())
                    .col(ColumnDef::new(Accidents::InsurancePayoutCents).integer())
                    .col(ColumnDef::new(Accidents::InsurancePayoutCurrency).text())
                    .col(ColumnDef::new(Accidents::Resolved).boolean().not_null().default(false))
                    .col(ColumnDef::new(Accidents::Notes).text())
                    .col(ColumnDef::new(Accidents::CreatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .col(ColumnDef::new(Accidents::UpdatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .foreign_key(
                        ForeignKey::create()
                            .from(Accidents::Table, Accidents::VehicleId)
                            .to(Vehicles::Table, Vehicles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // accident_correspondence
        manager
            .create_table(
                Table::create()
                    .table(AccidentCorrespondence::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(AccidentCorrespondence::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(AccidentCorrespondence::AccidentId).integer().not_null())
                    .col(ColumnDef::new(AccidentCorrespondence::OccurredAt).text().not_null())
                    .col(ColumnDef::new(AccidentCorrespondence::ContactMethod).text())
                    .col(ColumnDef::new(AccidentCorrespondence::ContactWith).text())
                    .col(ColumnDef::new(AccidentCorrespondence::Summary).text().not_null())
                    .col(ColumnDef::new(AccidentCorrespondence::Notes).text())
                    .col(ColumnDef::new(AccidentCorrespondence::CreatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .foreign_key(
                        ForeignKey::create()
                            .from(AccidentCorrespondence::Table, AccidentCorrespondence::AccidentId)
                            .to(Accidents::Table, Accidents::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // accident_service_links
        manager
            .create_table(
                Table::create()
                    .table(AccidentServiceLinks::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(AccidentServiceLinks::AccidentId).integer().not_null())
                    .col(ColumnDef::new(AccidentServiceLinks::ServiceRecordId).integer().not_null())
                    .primary_key(
                        Index::create()
                            .col(AccidentServiceLinks::AccidentId)
                            .col(AccidentServiceLinks::ServiceRecordId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(AccidentServiceLinks::Table, AccidentServiceLinks::AccidentId)
                            .to(Accidents::Table, Accidents::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(AccidentServiceLinks::Table, AccidentServiceLinks::ServiceRecordId)
                            .to(ServiceRecords::Table, ServiceRecords::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(AccidentServiceLinks::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(AccidentCorrespondence::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Accidents::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Accidents {
    Table,
    Id,
    VehicleId,
    OccurredAt,
    Odometer,
    Description,
    Fault,
    OtherPartyName,
    OtherPartyPhone,
    OtherPartyEmail,
    OtherPartyInsurance,
    OtherPartyPolicyNumber,
    InsuranceClaimNumber,
    InsuranceAdjuster,
    InsuranceAdjusterPhone,
    TotalRepairCostCents,
    TotalRepairCostCurrency,
    DeductibleCents,
    DeductibleCurrency,
    InsurancePayoutCents,
    InsurancePayoutCurrency,
    Resolved,
    Notes,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum AccidentCorrespondence {
    Table,
    Id,
    AccidentId,
    OccurredAt,
    ContactMethod,
    ContactWith,
    Summary,
    Notes,
    CreatedAt,
}

#[derive(DeriveIden)]
enum AccidentServiceLinks {
    Table,
    AccidentId,
    ServiceRecordId,
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
