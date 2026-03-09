use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // platforms
        manager
            .create_table(
                Table::create()
                    .table(Platforms::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Platforms::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Platforms::Name).text().not_null())
                    .col(ColumnDef::new(Platforms::WebsiteUrl).text())
                    .col(ColumnDef::new(Platforms::ApiBaseUrl).text())
                    .col(ColumnDef::new(Platforms::Notes).text())
                    .col(ColumnDef::new(Platforms::CreatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .col(ColumnDef::new(Platforms::UpdatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .to_owned(),
            )
            .await?;

        // model_templates
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
            .await?;

        // vehicles
        manager
            .create_table(
                Table::create()
                    .table(Vehicles::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Vehicles::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Vehicles::ModelTemplateId).integer())
                    .col(ColumnDef::new(Vehicles::Name).text().not_null())
                    .col(ColumnDef::new(Vehicles::Year).integer())
                    .col(ColumnDef::new(Vehicles::Make).text())
                    .col(ColumnDef::new(Vehicles::Model).text())
                    .col(ColumnDef::new(Vehicles::TrimLevel).text())
                    .col(ColumnDef::new(Vehicles::BodyStyle).text())
                    .col(ColumnDef::new(Vehicles::Engine).text())
                    .col(ColumnDef::new(Vehicles::Transmission).text())
                    .col(ColumnDef::new(Vehicles::Drivetrain).text())
                    .col(ColumnDef::new(Vehicles::Vin).text())
                    .col(ColumnDef::new(Vehicles::LicensePlate).text())
                    .col(ColumnDef::new(Vehicles::Color).text())
                    .col(ColumnDef::new(Vehicles::PurchaseDate).text())
                    .col(ColumnDef::new(Vehicles::PurchasePriceCents).integer())
                    .col(ColumnDef::new(Vehicles::PurchasePriceCurrency).text())
                    .col(ColumnDef::new(Vehicles::PurchaseMileage).integer())
                    .col(ColumnDef::new(Vehicles::SoldDate).text())
                    .col(ColumnDef::new(Vehicles::SoldPriceCents).integer())
                    .col(ColumnDef::new(Vehicles::SoldPriceCurrency).text())
                    .col(ColumnDef::new(Vehicles::SoldMileage).integer())
                    .col(ColumnDef::new(Vehicles::PhotoPath).text())
                    .col(ColumnDef::new(Vehicles::Notes).text())
                    .col(ColumnDef::new(Vehicles::CreatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .col(ColumnDef::new(Vehicles::UpdatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .foreign_key(
                        ForeignKey::create()
                            .from(Vehicles::Table, Vehicles::ModelTemplateId)
                            .to(ModelTemplates::Table, ModelTemplates::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        // vehicle_attributes
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

        // mileage_log
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
        manager.drop_table(Table::drop().table(MileageLog::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(VehicleAttributes::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Vehicles::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(ModelTemplates::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Platforms::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Platforms {
    Table,
    Id,
    Name,
    WebsiteUrl,
    ApiBaseUrl,
    Notes,
    CreatedAt,
    UpdatedAt,
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
enum Vehicles {
    Table,
    Id,
    ModelTemplateId,
    Name,
    Year,
    Make,
    Model,
    TrimLevel,
    BodyStyle,
    Engine,
    Transmission,
    Drivetrain,
    Vin,
    LicensePlate,
    Color,
    PurchaseDate,
    PurchasePriceCents,
    PurchasePriceCurrency,
    PurchaseMileage,
    SoldDate,
    SoldPriceCents,
    SoldPriceCurrency,
    SoldMileage,
    PhotoPath,
    Notes,
    CreatedAt,
    UpdatedAt,
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
