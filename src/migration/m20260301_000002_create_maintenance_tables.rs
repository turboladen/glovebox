use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // maintenance_schedule_items (includes fields from former migration 10)
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS maintenance_schedule_items (
                id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
                platform_id INTEGER,
                model_template_id INTEGER,
                vehicle_id INTEGER,
                overrides_item_id INTEGER,
                name TEXT NOT NULL,
                description TEXT,
                interval_miles INTEGER,
                interval_months INTEGER,
                labor_categories TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                warning_miles INTEGER DEFAULT 1000,
                warning_days INTEGER DEFAULT 30,
                enabled BOOLEAN NOT NULL DEFAULT TRUE,
                source TEXT,
                notes TEXT,
                is_factory_recommended BOOLEAN DEFAULT FALSE,
                FOREIGN KEY (platform_id) REFERENCES platforms(id) ON DELETE SET NULL,
                FOREIGN KEY (model_template_id) REFERENCES model_templates(id) ON DELETE SET NULL,
                FOREIGN KEY (vehicle_id) REFERENCES vehicles(id) ON DELETE CASCADE,
                FOREIGN KEY (overrides_item_id) REFERENCES maintenance_schedule_items(id) ON \
             DELETE SET NULL,
                CHECK (
                    (platform_id IS NOT NULL AND model_template_id IS NULL AND vehicle_id IS NULL) \
             OR
                    (platform_id IS NULL AND model_template_id IS NOT NULL AND vehicle_id IS NULL) \
             OR
                    (platform_id IS NULL AND model_template_id IS NULL AND vehicle_id IS NOT NULL)
                )
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_msi_platform
             ON maintenance_schedule_items(platform_id)
             WHERE platform_id IS NOT NULL",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_msi_model_template
             ON maintenance_schedule_items(model_template_id)
             WHERE model_template_id IS NOT NULL",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_msi_vehicle
             ON maintenance_schedule_items(vehicle_id)
             WHERE vehicle_id IS NOT NULL",
        )
        .await?;

        // shops
        manager
            .create_table(
                Table::create()
                    .table(Shops::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Shops::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Shops::Name).text().not_null())
                    .col(ColumnDef::new(Shops::Address).text())
                    .col(ColumnDef::new(Shops::Phone).text())
                    .col(ColumnDef::new(Shops::Website).text())
                    .col(ColumnDef::new(Shops::Specialty).text())
                    .col(ColumnDef::new(Shops::Notes).text())
                    .col(
                        ColumnDef::new(Shops::CreatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .col(
                        ColumnDef::new(Shops::UpdatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Shops::Table).to_owned())
            .await?;
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS maintenance_schedule_items")
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Shops {
    Table,
    Id,
    Name,
    Address,
    Phone,
    Website,
    Specialty,
    Notes,
    CreatedAt,
    UpdatedAt,
}
