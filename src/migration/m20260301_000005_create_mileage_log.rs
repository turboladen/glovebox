use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(MileageLog::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(MileageLog::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(MileageLog::VehicleId).integer().not_null())
                    .col(ColumnDef::new(MileageLog::Mileage).integer().not_null())
                    .col(ColumnDef::new(MileageLog::RecordedAt).text().not_null())
                    .col(ColumnDef::new(MileageLog::Source).text())
                    .col(ColumnDef::new(MileageLog::Notes).text())
                    .col(ColumnDef::new(MileageLog::CreatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .foreign_key(
                        ForeignKey::create()
                            .from(MileageLog::Table, MileageLog::VehicleId)
                            .to(Vehicles::Table, Vehicles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_mileage_log_vehicle_recorded")
                    .table(MileageLog::Table)
                    .col(MileageLog::VehicleId)
                    .col(MileageLog::RecordedAt)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(MileageLog::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum MileageLog {
    Table,
    Id,
    VehicleId,
    Mileage,
    RecordedAt,
    Source,
    Notes,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Vehicles {
    Table,
    Id,
}
