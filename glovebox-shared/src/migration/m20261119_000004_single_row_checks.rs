//! Adds CHECK constraints that each read one row: a job names its completing
//! service only while done, a service ends no earlier than it starts and a
//! self-performed one names no shop, a reading linked to a service says so in
//! its source, only a vehicle's item overrides another, and a research
//! finding's status is one of its vocabulary.
//!
//! It refuses to finish, and changes nothing, while a stored row breaks one
//! of them. Running it again finds the constraints in place and edits nothing.

use sea_orm_migration::prelude::*;

use super::schema::{Check, add_checks};

// The rules as this migration adds them. A rule changes only through a new
// migration, because `has_check` finds a constraint by its name and a fresh
// database must end up with what a migrated one has.
const CHECKS: &[Check] = &[
    Check {
        table: "work_items",
        name: "work_items_completed_while_done",
        holds: "completed_service_id IS NULL OR status = 'done'",
    },
    Check {
        table: "service_records",
        name: "service_records_end_after_start",
        holds: "end_date IS NULL OR end_date >= service_date",
    },
    Check {
        table: "service_records",
        name: "service_records_self_performed_has_no_shop",
        holds: "performed_by <> 'self' OR (shop_name IS NULL AND shop_id IS NULL)",
    },
    // Deleting a service deletes its reading, but the key's `ON DELETE SET
    // NULL` may still leave a `service` reading with no service, so the rule
    // runs one way. `IS` treats a null source as not `service`.
    Check {
        table: "mileage_log",
        name: "mileage_log_service_reading_source",
        holds: "service_record_id IS NULL OR source IS 'service'",
    },
    Check {
        table: "maintenance_schedule_items",
        name: "maintenance_schedule_items_overrides_on_vehicle_item",
        holds: "overrides_item_id IS NULL OR vehicle_id IS NOT NULL",
    },
    Check {
        table: "research_findings",
        name: "research_findings_status_listed",
        holds: "status IN ('new', 'completed', 'dismissed')",
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
        add_checks(&txn, "adding the single-row checks", CHECKS).await?;
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
    const BEFORE: u32 = 14;

    const STAMP: &str = "'2026-11-19T00:00:00+00:00'";

    const TABLES: &str = "('work_items', 'service_records', 'mileage_log', \
                          'maintenance_schedule_items', 'research_findings')";

    // A database as the previous migration leaves it, with one row in each
    // table that every rule allows.
    async fn db_before() -> DatabaseConnection {
        let db = migrated_to(BEFORE).await;
        db.execute_unprepared(&format!(
            "INSERT INTO vehicles (id, name, created_at, updated_at)
                 VALUES (1, 'Golf', {STAMP}, {STAMP});
             INSERT INTO service_records (id, vehicle_id, service_date, end_date, shop_name,
                     created_at, updated_at)
                 VALUES (1, 1, '2026-01-02', '2026-01-03', 'Example VW', {STAMP}, {STAMP});
             INSERT INTO work_items (id, vehicle_id, title, status, completed_service_id,
                     created_at, updated_at)
                 VALUES (1, 1, 'Brakes', 'done', 1, {STAMP}, {STAMP});
             INSERT INTO mileage_log (id, vehicle_id, mileage, recorded_at, source,
                     service_record_id, created_at)
                 VALUES (1, 1, 1000, '2026-01-02', 'service', 1, {STAMP});
             INSERT INTO maintenance_schedule_items (id, platform_id, name, created_at,
                     updated_at)
                 VALUES (900, (SELECT min(id) FROM platforms), 'Oil', {STAMP}, {STAMP});
             INSERT INTO maintenance_schedule_items (id, vehicle_id, overrides_item_id, name,
                     created_at, updated_at)
                 VALUES (901, 1, 900, 'Oil', {STAMP}, {STAMP});
             INSERT INTO research_reports (id, vehicle_id, generated_at, created_at)
                 VALUES (1, 1, {STAMP}, {STAMP});
             INSERT INTO research_findings (id, report_id, category, title, status, created_at,
                     updated_at)
                 VALUES (1, 1, 'recall', 'Pump', 'completed', {STAMP}, {STAMP});"
        ))
        .await
        .unwrap();
        db
    }

    async fn definitions(db: &DatabaseConnection) -> String {
        text(
            db,
            &format!(
                "SELECT group_concat(sql, ';') FROM (SELECT sql FROM sqlite_schema WHERE type = \
                 'table' AND name IN {TABLES} ORDER BY name)"
            ),
        )
        .await
    }

    // The five tables' indexes and FTS triggers: their entries in
    // `sqlite_schema` other than the tables' own.
    async fn surroundings(db: &DatabaseConnection) -> String {
        text(
            db,
            &format!(
                "SELECT group_concat(sql, ';') FROM (SELECT sql FROM sqlite_schema WHERE tbl_name \
                 IN {TABLES} AND type <> 'table' ORDER BY name)"
            ),
        )
        .await
    }

    #[tokio::test]
    async fn the_tables_keep_their_rows_triggers_and_indexes_and_gain_the_checks() {
        let db = db_before().await;
        let around = surroundings(&db).await;

        Migrator::up(&db, Some(1)).await.unwrap();

        let defs = definitions(&db).await;
        for check in super::CHECKS {
            assert!(
                defs.contains(&format!("CONSTRAINT \"{}\" CHECK", check.name)),
                "{defs}"
            );
        }
        assert_eq!(surroundings(&db).await, around);
        for (table, rows) in [
            ("work_items", 1),
            ("service_records", 1),
            ("mileage_log", 1),
            ("research_findings", 1),
            ("maintenance_schedule_items WHERE id >= 900", 2),
        ] {
            assert_eq!(
                count(&db, &format!("SELECT count(*) FROM {table}")).await,
                rows,
                "{table}"
            );
        }
        fts_integrity(
            &db,
            &[
                "fts_work_items",
                "fts_service_records",
                "fts_research_findings",
                "fts_maintenance_schedule_items",
            ],
        )
        .await;
    }

    #[tokio::test]
    async fn a_write_that_breaks_a_rule_is_refused_afterwards() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();
        for (rule, sql) in [
            (
                "work_items_completed_while_done",
                "UPDATE work_items SET status = 'planned' WHERE id = 1",
            ),
            (
                "service_records_end_after_start",
                "UPDATE service_records SET end_date = '2026-01-01' WHERE id = 1",
            ),
            (
                "service_records_self_performed_has_no_shop",
                "UPDATE service_records SET performed_by = 'self' WHERE id = 1",
            ),
            (
                "mileage_log_service_reading_source",
                "UPDATE mileage_log SET source = NULL WHERE id = 1",
            ),
            (
                "maintenance_schedule_items_overrides_on_vehicle_item",
                "UPDATE maintenance_schedule_items SET vehicle_id = NULL, platform_id = (SELECT \
                 min(id) FROM platforms) WHERE id = 901",
            ),
            (
                "research_findings_status_listed",
                "UPDATE research_findings SET status = 'open' WHERE id = 1",
            ),
        ] {
            let err = db.execute_unprepared(sql).await.expect_err(sql).to_string();
            assert!(err.contains(rule), "{sql}: {err}");
        }
    }

    // A value added to `FindingStatus` needs a migration that rewrites this
    // rule, so the two lists cannot part.
    #[test]
    fn the_status_rule_lists_every_finding_status() {
        use crate::{entities::research_finding::FindingStatus, vocab::Vocabulary};
        let listed: Vec<String> = FindingStatus::ALL
            .iter()
            .map(|s| format!("'{}'", s.as_str()))
            .collect();
        let rule = super::CHECKS
            .iter()
            .find(|c| c.name == "research_findings_status_listed")
            .unwrap();
        assert_eq!(rule.holds, format!("status IN ({})", listed.join(", ")));
    }

    #[tokio::test]
    async fn running_it_again_changes_nothing() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();
        let after = definitions(&db).await;
        Migrator::down(&db, Some(1)).await.unwrap();
        Migrator::up(&db, Some(1)).await.unwrap();
        assert_eq!(definitions(&db).await, after);
    }

    #[tokio::test]
    async fn a_row_that_breaks_a_rule_stops_it_before_anything_changes() {
        let db = db_before().await;
        db.execute_unprepared(
            "UPDATE work_items SET status = 'planned' WHERE id = 1;
             UPDATE research_findings SET status = 'acknowledged' WHERE id = 1;",
        )
        .await
        .unwrap();
        let before = definitions(&db).await;

        let err = Migrator::up(&db, Some(1)).await.unwrap_err().to_string();

        assert!(
            err.contains(
                "1 row of work_items breaks work_items_completed_while_done (id 1); 1 row of \
                 research_findings breaks research_findings_status_listed (id 1). Correct them, \
                 then start again. Nothing was changed."
            ),
            "{err}"
        );
        assert_eq!(definitions(&db).await, before);
    }
}
