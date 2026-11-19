//! Adds CHECK constraints to `parts` that tie its install, replace and return
//! columns to its status, and keep `received_on` from falling before
//! `purchase_date`.
//!
//! It refuses to finish, and changes nothing, while a stored part breaks one
//! of them. Running it again finds the constraints in place and edits nothing.

use sea_orm_migration::prelude::*;

use super::schema::{Check, add_checks};

// The rules as this migration adds them. A rule changes only through a new
// migration, because `has_check` finds a constraint by its name and a fresh
// database must end up with what a migrated one has.
const CHECKS: &[Check] = &[
    Check {
        table: "parts",
        name: "parts_install_while_fitted",
        holds: "status IN ('installed', 'replaced') OR (installed_service_id IS NULL AND \
                installed_date IS NULL AND installed_odometer IS NULL)",
    },
    Check {
        table: "parts",
        name: "parts_replace_while_replaced",
        holds: "status = 'replaced' OR (replaced_date IS NULL AND replaced_odometer IS NULL)",
    },
    Check {
        table: "parts",
        name: "parts_return_while_returned",
        holds: "(status = 'returned') = (returned_on IS NOT NULL) AND (status = 'returned') = \
                (return_reason IS NOT NULL)",
    },
    // The need's `ON DELETE SET NULL` key clears the id, so a returned part
    // may name no need.
    Check {
        table: "parts",
        name: "parts_return_need_while_returned",
        holds: "status = 'returned' OR returned_from_need_id IS NULL",
    },
    Check {
        table: "parts",
        name: "parts_received_after_purchase",
        holds: "received_on IS NULL OR purchase_date IS NULL OR received_on >= purchase_date",
    },
];

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // One transaction pins one connection, which `PRAGMA writable_schema`
        // needs, holds the write lock from its start and leaves nothing half
        // done when a step fails.
        let txn = crate::services::begin_write(manager.get_connection()).await?;
        add_checks(&txn, "adding the life-cycle checks of parts", CHECKS).await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Nothing is undone: no code depends on the constraints being absent.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection};
    use sea_orm_migration::MigratorTrait;

    use crate::migration::{
        Migrator,
        schema::testing::{count, fts_integrity, migrated_to, text},
    };

    // The number of migrations that run before this one.
    const BEFORE: u32 = 11;

    const STAMP: &str = "'2026-11-19T00:00:00+00:00'";

    // A database as the previous migration leaves it, holding a vehicle, a
    // service and a need, and one part in each status.
    async fn db_before() -> DatabaseConnection {
        let db = migrated_to(BEFORE).await;
        db.execute_unprepared(&format!(
            "INSERT INTO vehicles (id, name, created_at, updated_at)
                 VALUES (1, 'Golf', {STAMP}, {STAMP});
             INSERT INTO service_records (id, vehicle_id, service_date, created_at, updated_at)
                 VALUES (1, 1, '2026-01-02', {STAMP}, {STAMP});
             INSERT INTO work_items (id, vehicle_id, title, created_at, updated_at)
                 VALUES (1, 1, 'Brakes', {STAMP}, {STAMP});
             INSERT INTO job_needs (id, work_item_id, name, created_at, updated_at)
                 VALUES (1, 1, 'Pads', {STAMP}, {STAMP});
             INSERT INTO parts (id, vehicle_id, name, status, purchase_date, received_on,
                     created_at, updated_at)
                 VALUES (1, 1, 'Bought', 'purchased', '2026-01-01', '2026-01-01', {STAMP}, \
             {STAMP});
             INSERT INTO parts (id, vehicle_id, name, status, installed_service_id,
                     installed_date, installed_odometer, created_at, updated_at)
                 VALUES (2, 1, 'On', 'installed', 1, '2026-01-02', 1000, {STAMP}, {STAMP});
             INSERT INTO parts (id, vehicle_id, name, status, installed_date, replaced_date,
                     replaced_odometer, created_at, updated_at)
                 VALUES (3, 1, 'Off', 'replaced', '2026-01-02', '2026-06-01', 9000, {STAMP}, \
             {STAMP});
             INSERT INTO parts (id, vehicle_id, name, status, returned_on, return_reason,
                     returned_from_need_id, created_at, updated_at)
                 VALUES (4, 1, 'Back', 'returned', '2026-01-03', 'wrong size', 1, {STAMP}, \
             {STAMP});"
        ))
        .await
        .unwrap();
        db
    }

    async fn definition(db: &DatabaseConnection) -> String {
        text(db, "SELECT sql FROM sqlite_schema WHERE name = 'parts'").await
    }

    // Everything in `sqlite_schema` that belongs to `parts` but its own row.
    async fn surroundings(db: &DatabaseConnection) -> String {
        text(
            db,
            "SELECT group_concat(sql, ';') FROM (SELECT sql FROM sqlite_schema WHERE tbl_name = \
             'parts' AND type <> 'table' ORDER BY name)",
        )
        .await
    }

    #[tokio::test]
    async fn the_table_keeps_its_rows_triggers_and_indexes_and_gains_the_checks() {
        let db = db_before().await;
        let rows = text(&db, "SELECT group_concat(id || status, ',') FROM parts").await;
        let around = surroundings(&db).await;
        let columns = text(
            &db,
            "SELECT group_concat(name, ',') FROM pragma_table_info('parts')",
        )
        .await;

        Migrator::up(&db, Some(1)).await.unwrap();

        let def = definition(&db).await;
        for name in [
            "parts_install_while_fitted",
            "parts_replace_while_replaced",
            "parts_return_while_returned",
            "parts_return_need_while_returned",
            "parts_received_after_purchase",
        ] {
            assert!(
                def.contains(&format!("CONSTRAINT \"{name}\" CHECK")),
                "{def}"
            );
        }
        assert_eq!(
            text(&db, "SELECT group_concat(id || status, ',') FROM parts").await,
            rows
        );
        assert_eq!(surroundings(&db).await, around);
        assert_eq!(
            text(
                &db,
                "SELECT group_concat(name, ',') FROM pragma_table_info('parts')"
            )
            .await,
            columns
        );
        db.execute_unprepared("UPDATE parts SET name = 'Pads' WHERE id = 1")
            .await
            .unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT count(*) FROM fts_parts WHERE fts_parts MATCH 'Pads'"
            )
            .await,
            1
        );
        fts_integrity(&db, &["fts_parts", "fts_part_contents"]).await;
    }

    #[tokio::test]
    async fn a_write_that_breaks_a_rule_is_refused_afterwards() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();
        for (rule, sql) in [
            (
                "parts_install_while_fitted",
                "UPDATE parts SET installed_date = '2026-01-05' WHERE id = 1",
            ),
            (
                "parts_replace_while_replaced",
                "UPDATE parts SET replaced_odometer = 5 WHERE id = 2",
            ),
            (
                "parts_return_while_returned",
                "UPDATE parts SET return_reason = NULL WHERE id = 4",
            ),
            (
                "parts_return_while_returned",
                "UPDATE parts SET returned_on = '2026-01-03' WHERE id = 1",
            ),
            (
                "parts_return_need_while_returned",
                "UPDATE parts SET status = 'purchased', returned_on = NULL, return_reason = NULL \
                 WHERE id = 4",
            ),
            (
                "parts_received_after_purchase",
                "UPDATE parts SET received_on = '2025-12-31' WHERE id = 1",
            ),
        ] {
            let err = db.execute_unprepared(sql).await.expect_err(sql).to_string();
            assert!(err.contains(rule), "{sql}: {err}");
        }
        // A deleted need clears the id its returned part names.
        db.execute_unprepared("DELETE FROM job_needs WHERE id = 1")
            .await
            .unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT count(*) FROM parts WHERE id = 4 AND returned_from_need_id IS NULL"
            )
            .await,
            1
        );
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
    async fn a_part_that_breaks_a_rule_stops_it_before_anything_changes() {
        let db = db_before().await;
        db.execute_unprepared(
            "UPDATE parts SET installed_date = '2026-01-05' WHERE id = 1;
             UPDATE parts SET replaced_date = '2026-01-05' WHERE id IN (1, 2);",
        )
        .await
        .unwrap();
        let before = definition(&db).await;

        let err = Migrator::up(&db, Some(1)).await.unwrap_err().to_string();

        assert!(
            err.contains(
                "1 row of parts breaks parts_install_while_fitted (id 1); 2 rows of parts break \
                 parts_replace_while_replaced (id 1, 2). Correct them, then start again. Nothing \
                 was changed."
            ),
            "{err}"
        );
        assert_eq!(definition(&db).await, before);
    }
}
