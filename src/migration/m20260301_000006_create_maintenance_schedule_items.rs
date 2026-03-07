use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

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
                FOREIGN KEY (platform_id) REFERENCES platforms(id) ON DELETE SET NULL,
                FOREIGN KEY (model_template_id) REFERENCES model_templates(id) ON DELETE SET NULL,
                FOREIGN KEY (vehicle_id) REFERENCES vehicles(id) ON DELETE CASCADE,
                FOREIGN KEY (overrides_item_id) REFERENCES maintenance_schedule_items(id) ON DELETE SET NULL,
                CHECK (
                    (platform_id IS NOT NULL AND model_template_id IS NULL AND vehicle_id IS NULL) OR
                    (platform_id IS NULL AND model_template_id IS NOT NULL AND vehicle_id IS NULL) OR
                    (platform_id IS NULL AND model_template_id IS NULL AND vehicle_id IS NOT NULL)
                )
            )"
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_msi_platform
             ON maintenance_schedule_items(platform_id)
             WHERE platform_id IS NOT NULL"
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_msi_model_template
             ON maintenance_schedule_items(model_template_id)
             WHERE model_template_id IS NOT NULL"
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_msi_vehicle
             ON maintenance_schedule_items(vehicle_id)
             WHERE vehicle_id IS NOT NULL"
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS maintenance_schedule_items")
            .await?;
        Ok(())
    }
}
