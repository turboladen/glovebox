use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(VehicleAttributes::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(VehicleAttributes::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(VehicleAttributes::VehicleId).integer().not_null())
                    .col(ColumnDef::new(VehicleAttributes::Key).text().not_null())
                    .col(ColumnDef::new(VehicleAttributes::Value).text().not_null())
                    .col(ColumnDef::new(VehicleAttributes::Source).text())
                    .col(ColumnDef::new(VehicleAttributes::SupersededBy).integer())
                    .col(ColumnDef::new(VehicleAttributes::CreatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .foreign_key(
                        ForeignKey::create()
                            .from(VehicleAttributes::Table, VehicleAttributes::VehicleId)
                            .to(Vehicles::Table, Vehicles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(VehicleAttributes::Table, VehicleAttributes::SupersededBy)
                            .to(VehicleAttributes::Table, VehicleAttributes::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_vehicle_attributes_unique")
                    .table(VehicleAttributes::Table)
                    .col(VehicleAttributes::VehicleId)
                    .col(VehicleAttributes::Key)
                    .col(VehicleAttributes::Source)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_vehicle_attributes_vehicle_key")
                    .table(VehicleAttributes::Table)
                    .col(VehicleAttributes::VehicleId)
                    .col(VehicleAttributes::Key)
                    .to_owned(),
            )
            .await?;

        // Partial unique index for NULL source (SQLite treats NULLs as distinct in unique indexes)
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_vehicle_attributes_unique_no_source
                 ON vehicle_attributes(vehicle_id, key)
                 WHERE source IS NULL"
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(VehicleAttributes::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum VehicleAttributes {
    Table,
    Id,
    VehicleId,
    Key,
    Value,
    Source,
    SupersededBy,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Vehicles {
    Table,
    Id,
}
