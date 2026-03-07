use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Vehicles::Table).to_owned()).await
    }
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
enum ModelTemplates {
    Table,
    Id,
}
