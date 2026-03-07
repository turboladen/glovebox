use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ServiceScheduleLinks::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ServiceScheduleLinks::ServiceRecordId).integer().not_null())
                    .col(ColumnDef::new(ServiceScheduleLinks::ScheduleItemId).integer().not_null())
                    .primary_key(
                        Index::create()
                            .col(ServiceScheduleLinks::ServiceRecordId)
                            .col(ServiceScheduleLinks::ScheduleItemId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(ServiceScheduleLinks::Table, ServiceScheduleLinks::ServiceRecordId)
                            .to(ServiceRecords::Table, ServiceRecords::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(ServiceScheduleLinks::Table, ServiceScheduleLinks::ScheduleItemId)
                            .to(MaintenanceScheduleItems::Table, MaintenanceScheduleItems::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(ServiceScheduleLinks::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum ServiceScheduleLinks {
    Table,
    ServiceRecordId,
    ScheduleItemId,
}

#[derive(DeriveIden)]
enum ServiceRecords {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum MaintenanceScheduleItems {
    Table,
    Id,
}
