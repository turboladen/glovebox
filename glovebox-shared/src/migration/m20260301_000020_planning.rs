//! Planning primitives (2hea unit G): `work_items` + `visits`, plus
//! `est_cost_cents` on schedule items (decision ⑧) and warranty expiry
//! fields on vehicles and parts (decision ⑩).
//!
//! `work_items` are "the list of things I'm actually gonna do" — sourced
//! from a schedule item, a research finding (recall), an incident, a build,
//! or ad-hoc. `visits` group items into a shop trip (or DIY session);
//! completing a visit produces the linked service record. Source FKs and
//! `visit_id` are plain nullable INTs — the service layer enforces
//! vehicle ownership (000014 pattern); only `vehicle_id` carries a real
//! CASCADE FK. No FTS arms: planning rows are short-lived operational
//! data (deliberate non-goal; searchable later if wanted).
//!
//! Rerun-safety: `SQLite` migrations are NOT transactional, so a crash
//! mid-`up()` reruns from the top. `create_table`/`create_index` use
//! `if_not_exists`; every ALTER is guarded by a `pragma_table_info` check
//! (ADD COLUMN has no IF NOT EXISTS) — the 000019 pattern.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// True when `table` already has `column` (pragma-guard for ALTERs).
async fn column_exists(
    manager: &SchemaManager<'_>,
    table: &str,
    column: &str,
) -> Result<bool, DbErr> {
    let db = manager.get_connection();
    Ok(db
        .query_one(sea_orm::Statement::from_string(
            manager.get_database_backend(),
            format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = '{column}'"),
        ))
        .await?
        .is_some())
}

async fn add_column_if_missing(
    manager: &SchemaManager<'_>,
    table: &str,
    column: &str,
    def: ColumnDef,
) -> Result<(), DbErr> {
    if column_exists(manager, table, column).await? {
        return Ok(());
    }
    manager
        .alter_table(
            Table::alter()
                .table(Alias::new(table))
                .add_column(def)
                .to_owned(),
        )
        .await
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    #[allow(clippy::too_many_lines)] // two full table definitions + five guarded ALTERs
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(WorkItems::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(WorkItems::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(WorkItems::VehicleId).integer().not_null())
                    .col(ColumnDef::new(WorkItems::Title).text().not_null())
                    .col(ColumnDef::new(WorkItems::Notes).text())
                    .col(ColumnDef::new(WorkItems::ScheduleItemId).integer())
                    .col(ColumnDef::new(WorkItems::ResearchFindingId).integer())
                    .col(ColumnDef::new(WorkItems::IncidentId).integer())
                    .col(ColumnDef::new(WorkItems::BuildId).integer())
                    .col(ColumnDef::new(WorkItems::EstCostCents).integer())
                    .col(
                        ColumnDef::new(WorkItems::Status)
                            .text()
                            .not_null()
                            .default("planned"),
                    )
                    .col(ColumnDef::new(WorkItems::VisitId).integer())
                    .col(
                        ColumnDef::new(WorkItems::CreatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .col(
                        ColumnDef::new(WorkItems::UpdatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(WorkItems::Table, WorkItems::VehicleId)
                            .to(Vehicles::Table, Vehicles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_work_items_vehicle")
                    .table(WorkItems::Table)
                    .col(WorkItems::VehicleId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Visits::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Visits::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Visits::VehicleId).integer().not_null())
                    .col(ColumnDef::new(Visits::PlannedDate).text())
                    .col(ColumnDef::new(Visits::ShopName).text())
                    .col(ColumnDef::new(Visits::ShopId).integer())
                    .col(ColumnDef::new(Visits::Notes).text())
                    .col(
                        ColumnDef::new(Visits::Status)
                            .text()
                            .not_null()
                            .default("planned"),
                    )
                    .col(ColumnDef::new(Visits::ServiceRecordId).integer())
                    .col(
                        ColumnDef::new(Visits::CreatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .col(
                        ColumnDef::new(Visits::UpdatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Visits::Table, Visits::VehicleId)
                            .to(Vehicles::Table, Vehicles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_visits_vehicle")
                    .table(Visits::Table)
                    .col(Visits::VehicleId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // ⑧ estimated cost per schedule item (budget forecast input).
        add_column_if_missing(
            manager,
            "maintenance_schedule_items",
            "est_cost_cents",
            ColumnDef::new(Alias::new("est_cost_cents"))
                .integer()
                .to_owned(),
        )
        .await?;

        // ⑩ warranty expiry (date and/or mileage) on vehicles and parts.
        for table in ["vehicles", "parts"] {
            add_column_if_missing(
                manager,
                table,
                "warranty_expires_on",
                ColumnDef::new(Alias::new("warranty_expires_on"))
                    .text()
                    .to_owned(),
            )
            .await?;
            add_column_if_missing(
                manager,
                table,
                "warranty_expires_miles",
                ColumnDef::new(Alias::new("warranty_expires_miles"))
                    .integer()
                    .to_owned(),
            )
            .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in ["parts", "vehicles"] {
            for column in ["warranty_expires_miles", "warranty_expires_on"] {
                manager
                    .alter_table(
                        Table::alter()
                            .table(Alias::new(table))
                            .drop_column(Alias::new(column))
                            .to_owned(),
                    )
                    .await?;
            }
        }
        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("maintenance_schedule_items"))
                    .drop_column(Alias::new("est_cost_cents"))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(Visits::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(WorkItems::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum WorkItems {
    Table,
    Id,
    VehicleId,
    Title,
    Notes,
    ScheduleItemId,
    ResearchFindingId,
    IncidentId,
    BuildId,
    EstCostCents,
    Status,
    VisitId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Visits {
    Table,
    Id,
    VehicleId,
    PlannedDate,
    ShopName,
    ShopId,
    Notes,
    Status,
    ServiceRecordId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Vehicles {
    Table,
    Id,
}
