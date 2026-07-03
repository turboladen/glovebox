use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Who paid for the service: 'self' (default), 'insurance', or
        // 'third_party'. The column default covers all existing rows.
        manager
            .alter_table(
                Table::alter()
                    .table(ServiceRecords::Table)
                    .add_column(
                        ColumnDef::new(ServiceRecords::PaidBy)
                            .text()
                            .not_null()
                            .default("self"),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(ServiceRecords::Table)
                    .add_column(ColumnDef::new(ServiceRecords::PayerNote).text())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ServiceRecords::Table)
                    .drop_column(ServiceRecords::PayerNote)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(ServiceRecords::Table)
                    .drop_column(ServiceRecords::PaidBy)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum ServiceRecords {
    Table,
    PaidBy,
    PayerNote,
}
