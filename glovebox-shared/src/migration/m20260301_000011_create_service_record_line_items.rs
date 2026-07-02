use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ServiceRecordLineItems::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ServiceRecordLineItems::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ServiceRecordLineItems::ServiceRecordId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ServiceRecordLineItems::Description)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ServiceRecordLineItems::Category).text())
                    .col(ColumnDef::new(ServiceRecordLineItems::Quantity).double())
                    .col(ColumnDef::new(ServiceRecordLineItems::UnitCostCents).integer())
                    .col(ColumnDef::new(ServiceRecordLineItems::CostCents).integer())
                    .col(
                        ColumnDef::new(ServiceRecordLineItems::CreatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .col(
                        ColumnDef::new(ServiceRecordLineItems::UpdatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(
                                ServiceRecordLineItems::Table,
                                ServiceRecordLineItems::ServiceRecordId,
                            )
                            .to(ServiceRecords::Table, ServiceRecords::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .table(ServiceRecordLineItems::Table)
                    .name("idx_service_record_line_items_service_record_id")
                    .col(ServiceRecordLineItems::ServiceRecordId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(ServiceRecordLineItems::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum ServiceRecordLineItems {
    Table,
    Id,
    ServiceRecordId,
    Description,
    Category,
    Quantity,
    UnitCostCents,
    CostCents,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum ServiceRecords {
    Table,
    Id,
}
