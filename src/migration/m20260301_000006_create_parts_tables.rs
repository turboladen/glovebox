use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // part_slots
        manager
            .create_table(
                Table::create()
                    .table(PartSlots::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(PartSlots::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(PartSlots::VehicleId).integer().not_null())
                    .col(ColumnDef::new(PartSlots::Name).text().not_null())
                    .col(ColumnDef::new(PartSlots::Category).text())
                    .col(ColumnDef::new(PartSlots::OeSpec).text())
                    .col(ColumnDef::new(PartSlots::OePartNumber).text())
                    .col(ColumnDef::new(PartSlots::Notes).text())
                    .col(ColumnDef::new(PartSlots::CreatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .foreign_key(
                        ForeignKey::create()
                            .from(PartSlots::Table, PartSlots::VehicleId)
                            .to(Vehicles::Table, Vehicles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .table(PartSlots::Table)
                    .name("idx_part_slots_vehicle")
                    .col(PartSlots::VehicleId)
                    .to_owned(),
            )
            .await?;

        // parts
        manager
            .create_table(
                Table::create()
                    .table(Parts::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Parts::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Parts::SlotId).integer())
                    .col(ColumnDef::new(Parts::VehicleId).integer().not_null())
                    .col(ColumnDef::new(Parts::Name).text().not_null())
                    .col(ColumnDef::new(Parts::Manufacturer).text())
                    .col(ColumnDef::new(Parts::PartNumber).text())
                    .col(ColumnDef::new(Parts::OePartNumberReplaced).text())
                    .col(ColumnDef::new(Parts::Seller).text())
                    .col(ColumnDef::new(Parts::PurchaseDate).text())
                    .col(ColumnDef::new(Parts::CostCents).integer())
                    .col(ColumnDef::new(Parts::CostCurrency).text().default("USD"))
                    .col(ColumnDef::new(Parts::InvoiceUrl).text())
                    .col(ColumnDef::new(Parts::Status).text().not_null().default("purchased"))
                    .col(ColumnDef::new(Parts::InstalledDate).text())
                    .col(ColumnDef::new(Parts::InstalledOdometer).integer())
                    .col(ColumnDef::new(Parts::InstalledServiceId).integer())
                    .col(ColumnDef::new(Parts::ReplacedDate).text())
                    .col(ColumnDef::new(Parts::ReplacedOdometer).integer())
                    .col(ColumnDef::new(Parts::Notes).text())
                    .col(ColumnDef::new(Parts::CreatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .col(ColumnDef::new(Parts::UpdatedAt).text().not_null().default(Expr::cust("(datetime('now'))")))
                    .foreign_key(
                        ForeignKey::create()
                            .from(Parts::Table, Parts::SlotId)
                            .to(PartSlots::Table, PartSlots::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Parts::Table, Parts::VehicleId)
                            .to(Vehicles::Table, Vehicles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Parts::Table, Parts::InstalledServiceId)
                            .to(ServiceRecords::Table, ServiceRecords::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .table(Parts::Table)
                    .name("idx_parts_vehicle")
                    .col(Parts::VehicleId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .table(Parts::Table)
                    .name("idx_parts_slot")
                    .col(Parts::SlotId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Parts::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(PartSlots::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum PartSlots {
    Table,
    Id,
    VehicleId,
    Name,
    Category,
    OeSpec,
    OePartNumber,
    Notes,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Parts {
    Table,
    Id,
    SlotId,
    VehicleId,
    Name,
    Manufacturer,
    PartNumber,
    OePartNumberReplaced,
    Seller,
    PurchaseDate,
    CostCents,
    CostCurrency,
    InvoiceUrl,
    Status,
    InstalledDate,
    InstalledOdometer,
    InstalledServiceId,
    ReplacedDate,
    ReplacedOdometer,
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
