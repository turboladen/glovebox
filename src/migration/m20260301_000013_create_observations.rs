use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
enum Vehicles {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum ServiceRecords {
    Table,
    Id,
}
