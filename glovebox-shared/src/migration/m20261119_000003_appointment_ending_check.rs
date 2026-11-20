//! Adds a CHECK constraint to `appointments` that keeps an appointment from
//! being both closed and cancelled.
//!
//! It refuses to finish, and changes nothing, while a stored appointment
//! holds both dates. Running it again finds the constraint in place and edits
//! nothing.

use sea_orm_migration::prelude::*;

use super::schema::{Check, add_checks};

// The rule as this migration adds it. A rule changes only through a new
// migration, because `has_check` finds a constraint by its name and a fresh
// database must end up with what a migrated one has.
const CHECKS: &[Check] = &[Check {
    table: "appointments",
    name: "appointments_one_ending",
    holds: "closed_on IS NULL OR cancelled_on IS NULL",
}];

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // One transaction pins one connection, which `PRAGMA writable_schema`
        // needs, holds the write lock from its start and leaves nothing half
        // done when a step fails.
        let txn = crate::services::begin_write(manager.get_connection()).await?;
        add_checks(&txn, "adding the ending check of appointments", CHECKS).await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Nothing is undone: no code depends on the constraint being absent.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection};
    use sea_orm_migration::MigratorTrait;

    use crate::migration::{
        Migrator,
        schema::testing::{migrated_to, text},
    };

    // The number of migrations that run before this one.
    const BEFORE: u32 = 13;

    const STAMP: &str = "'2026-11-19T00:00:00+00:00'";

    // A database as the previous migration leaves it, with an open, a closed
    // and a cancelled appointment.
    async fn db_before() -> DatabaseConnection {
        let db = migrated_to(BEFORE).await;
        db.execute_unprepared(&format!(
            "INSERT INTO vehicles (id, name, created_at, updated_at)
                 VALUES (1, 'Golf', {STAMP}, {STAMP});
             INSERT INTO appointments (id, vehicle_id, purpose, created_at, updated_at)
                 VALUES (1, 1, 'Open', {STAMP}, {STAMP});
             INSERT INTO appointments (id, vehicle_id, purpose, closed_on, created_at, updated_at)
                 VALUES (2, 1, 'Closed', '2026-11-01', {STAMP}, {STAMP});
             INSERT INTO appointments (id, vehicle_id, purpose, cancelled_on, created_at,
                     updated_at)
                 VALUES (3, 1, 'Cancelled', '2026-11-02', {STAMP}, {STAMP});"
        ))
        .await
        .unwrap();
        db
    }

    async fn definition(db: &DatabaseConnection) -> String {
        text(
            db,
            "SELECT sql FROM sqlite_schema WHERE name = 'appointments'",
        )
        .await
    }

    async fn everything_else(db: &DatabaseConnection) -> String {
        text(
            db,
            "SELECT group_concat(sql, ';') FROM (SELECT sql FROM sqlite_schema WHERE tbl_name = \
             'appointments' AND type <> 'table' ORDER BY name)",
        )
        .await
    }

    async fn rows(db: &DatabaseConnection) -> String {
        text(
            db,
            "SELECT group_concat(id || ifnull(closed_on, '-') || ifnull(cancelled_on, '-'), ',') \
             FROM appointments",
        )
        .await
    }

    #[tokio::test]
    async fn the_table_keeps_its_rows_and_indexes_and_gains_the_check() {
        let db = db_before().await;
        let before = rows(&db).await;
        let around = everything_else(&db).await;

        Migrator::up(&db, Some(1)).await.unwrap();

        let def = definition(&db).await;
        assert!(
            def.contains("CONSTRAINT \"appointments_one_ending\" CHECK"),
            "{def}"
        );
        assert_eq!(rows(&db).await, before);
        assert_eq!(everything_else(&db).await, around);
        let err = db
            .execute_unprepared("UPDATE appointments SET cancelled_on = '2026-11-03' WHERE id = 2")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("appointments_one_ending"), "{err}");
    }

    #[tokio::test]
    async fn running_it_again_changes_nothing() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();
        let after = definition(&db).await;
        Migrator::down(&db, Some(1)).await.unwrap();
        Migrator::up(&db, Some(1)).await.unwrap();
        assert_eq!(definition(&db).await, after);
    }

    #[tokio::test]
    async fn an_appointment_with_both_dates_stops_it_before_anything_changes() {
        let db = db_before().await;
        db.execute_unprepared("UPDATE appointments SET cancelled_on = '2026-11-03' WHERE id = 2")
            .await
            .unwrap();
        let before = definition(&db).await;

        let err = Migrator::up(&db, Some(1)).await.unwrap_err().to_string();

        assert!(
            err.contains(
                "1 row of appointments breaks appointments_one_ending (id 2). Correct them, then \
                 start again. Nothing was changed."
            ),
            "{err}"
        );
        assert_eq!(definition(&db).await, before);
    }
}
