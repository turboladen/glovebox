use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // builds: one-shot upgrade/restoration targets (the car-native Goal form).
        // Lightweight and not event-sourced — progress is derived at query time
        // from linked service_records/parts/observations via their build_id FKs.
        manager
            .create_table(
                Table::create()
                    .table(Builds::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Builds::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Builds::VehicleId).integer().not_null())
                    .col(ColumnDef::new(Builds::Name).text().not_null())
                    .col(ColumnDef::new(Builds::Description).text())
                    .col(
                        ColumnDef::new(Builds::Status)
                            .text()
                            .not_null()
                            .default("planned"),
                    )
                    .col(ColumnDef::new(Builds::TargetDate).text())
                    .col(ColumnDef::new(Builds::CompletedAt).text())
                    .col(
                        ColumnDef::new(Builds::CreatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .col(
                        ColumnDef::new(Builds::UpdatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Builds::Table, Builds::VehicleId)
                            .to(Vehicles::Table, Vehicles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_builds_vehicle")
                    .table(Builds::Table)
                    .col(Builds::VehicleId)
                    .to_owned(),
            )
            .await?;

        // Single-FK links: a record belongs to at most one build. Plain nullable
        // INT columns (SQLite ALTER TABLE ADD COLUMN cannot add FK constraints);
        // the service layer enforces build ownership, matching 000009/000012.
        manager
            .alter_table(
                Table::alter()
                    .table(ServiceRecords::Table)
                    .add_column(ColumnDef::new(ServiceRecords::BuildId).integer())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Parts::Table)
                    .add_column(ColumnDef::new(Parts::BuildId).integer())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Observations::Table)
                    .add_column(ColumnDef::new(Observations::BuildId).integer())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Observations::Table)
                    .drop_column(Observations::BuildId)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Parts::Table)
                    .drop_column(Parts::BuildId)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(ServiceRecords::Table)
                    .drop_column(ServiceRecords::BuildId)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(Builds::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Builds {
    Table,
    Id,
    VehicleId,
    Name,
    Description,
    Status,
    TargetDate,
    CompletedAt,
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
    BuildId,
}

#[derive(DeriveIden)]
enum Parts {
    Table,
    BuildId,
}

#[derive(DeriveIden)]
enum Observations {
    Table,
    BuildId,
}
