use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // service_records (includes shop_id from former migration 12)
        manager
            .create_table(
                Table::create()
                    .table(ServiceRecords::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ServiceRecords::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ServiceRecords::VehicleId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ServiceRecords::ServiceDate)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ServiceRecords::Mileage).integer())
                    .col(ColumnDef::new(ServiceRecords::Description).text())
                    .col(ColumnDef::new(ServiceRecords::PartsCostCents).integer())
                    .col(ColumnDef::new(ServiceRecords::PartsCostCurrency).text())
                    .col(ColumnDef::new(ServiceRecords::LaborCostCents).integer())
                    .col(ColumnDef::new(ServiceRecords::LaborCostCurrency).text())
                    .col(ColumnDef::new(ServiceRecords::TotalCostCents).integer())
                    .col(ColumnDef::new(ServiceRecords::TotalCostCurrency).text())
                    .col(ColumnDef::new(ServiceRecords::ShopName).text())
                    .col(ColumnDef::new(ServiceRecords::Notes).text())
                    .col(
                        ColumnDef::new(ServiceRecords::CreatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .col(
                        ColumnDef::new(ServiceRecords::UpdatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .col(ColumnDef::new(ServiceRecords::ShopId).integer())
                    .foreign_key(
                        ForeignKey::create()
                            .from(ServiceRecords::Table, ServiceRecords::VehicleId)
                            .to(Vehicles::Table, Vehicles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(ServiceRecords::Table, ServiceRecords::ShopId)
                            .to(Shops::Table, Shops::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        // service_schedule_links
        manager
            .create_table(
                Table::create()
                    .table(ServiceScheduleLinks::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ServiceScheduleLinks::ServiceRecordId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ServiceScheduleLinks::ScheduleItemId)
                            .integer()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(ServiceScheduleLinks::ServiceRecordId)
                            .col(ServiceScheduleLinks::ScheduleItemId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(
                                ServiceScheduleLinks::Table,
                                ServiceScheduleLinks::ServiceRecordId,
                            )
                            .to(ServiceRecords::Table, ServiceRecords::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(
                                ServiceScheduleLinks::Table,
                                ServiceScheduleLinks::ScheduleItemId,
                            )
                            .to(
                                MaintenanceScheduleItems::Table,
                                MaintenanceScheduleItems::Id,
                            )
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ServiceScheduleLinks::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(ServiceRecords::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum ServiceRecords {
    Table,
    Id,
    VehicleId,
    ServiceDate,
    Mileage,
    Description,
    PartsCostCents,
    PartsCostCurrency,
    LaborCostCents,
    LaborCostCurrency,
    TotalCostCents,
    TotalCostCurrency,
    ShopName,
    Notes,
    CreatedAt,
    UpdatedAt,
    ShopId,
}

#[derive(DeriveIden)]
enum Vehicles {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Shops {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum ServiceScheduleLinks {
    Table,
    ServiceRecordId,
    ScheduleItemId,
}

#[derive(DeriveIden)]
enum MaintenanceScheduleItems {
    Table,
    Id,
}
