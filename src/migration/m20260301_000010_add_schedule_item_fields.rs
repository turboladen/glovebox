use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "ALTER TABLE maintenance_schedule_items ADD COLUMN warning_miles INTEGER DEFAULT 1000"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE maintenance_schedule_items ADD COLUMN warning_days INTEGER DEFAULT 30"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE maintenance_schedule_items ADD COLUMN enabled BOOLEAN NOT NULL DEFAULT TRUE"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE maintenance_schedule_items ADD COLUMN source TEXT"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE maintenance_schedule_items ADD COLUMN notes TEXT"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE maintenance_schedule_items ADD COLUMN is_factory_recommended BOOLEAN DEFAULT FALSE"
        ).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite doesn't support DROP COLUMN before 3.35.0; recreate table if needed
        let _ = manager.get_connection();
        Ok(())
    }
}
