//! Moves every bookkeeping stamp to RFC 3339 in UTC and removes the column
//! defaults that wrote the other form.
//!
//! A stamp is read as a UTC instant and written as
//! `2026-11-14T12:00:00+00:00`. A row written by a `datetime('now')` default
//! would hold `2026-11-14 12:00:00`, which sorts before every RFC 3339 value,
//! so no stamp column keeps a default and code sets the stamps on every
//! insert. `incidents.occurred_at` carries the same default and loses it too;
//! the incident service always sets that column.
//!
//! The whole change runs in one transaction. Running it again finds no value
//! in the other form and no default to remove.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

use super::schema::{edit_definitions, scalar};

/// Every column that holds a bookkeeping stamp.
const STAMPS: &[(&str, &[&str])] = &[
    ("appointments", &["created_at", "updated_at"]),
    ("incident_followups", &["created_at"]),
    ("incidents", &["created_at", "updated_at"]),
    ("job_needs", &["created_at", "updated_at"]),
    ("maintenance_schedule_items", &["created_at", "updated_at"]),
    ("mileage_log", &["created_at"]),
    ("model_templates", &["created_at", "updated_at"]),
    ("need_options", &["created_at", "updated_at"]),
    ("parts", &["created_at", "updated_at"]),
    ("platforms", &["created_at", "updated_at"]),
    ("research_findings", &["created_at", "updated_at"]),
    ("research_reports", &["generated_at", "created_at"]),
    ("service_record_line_items", &["created_at", "updated_at"]),
    ("service_records", &["created_at", "updated_at"]),
    ("shops", &["created_at", "updated_at"]),
    ("todos", &["created_at", "updated_at"]),
    ("vehicle_attributes", &["created_at"]),
    ("vehicles", &["created_at", "updated_at", "archived_at"]),
    ("work_items", &["created_at", "updated_at"]),
];

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // One transaction pins one connection, which `PRAGMA writable_schema`
        // needs, and leaves nothing half done when a step fails.
        let txn = manager.get_connection().begin().await?;
        // A write first takes the write lock, waiting out another program's
        // write. A transaction that reads first and writes later fails at once
        // if anything was written in between.
        txn.execute_unprepared("UPDATE seaql_migrations SET version = version WHERE 0")
            .await?;

        let mut unreadable = Vec::new();
        for (table, columns) in STAMPS {
            for column in *columns {
                unreadable.extend(count_unreadable(&txn, table, column).await?);
            }
        }
        if !unreadable.is_empty() {
            return Err(DbErr::Migration(format!(
                "these columns hold a timestamp that is neither YYYY-MM-DD HH:MM:SS nor RFC 3339: \
                 {}. Nothing was changed. Correct the values, then start again.",
                unreadable.join(", ")
            )));
        }

        for (table, columns) in STAMPS {
            for column in *columns {
                txn.execute_unprepared(&format!(
                    "UPDATE {table} SET {column} = replace({column}, ' ', 'T') || '+00:00' WHERE \
                     {column} GLOB '{OLD_SHAPE}'"
                ))
                .await?;
            }
        }

        remove_defaults(&txn).await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Nothing is undone: the code that reads a stamp takes both forms.
        Ok(())
    }
}

/// The text shape of a stamp that `datetime('now')` wrote, as a `GLOB` and as
/// a chrono format.
const OLD_SHAPE: &str = "????-??-?? ??:??:??";
const OLD_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// Describes the values of `table.column` that are in neither stamp form, as
/// `Some` text such as `2 values in parts.updated_at`, or `None` when there
/// are none.
async fn count_unreadable(
    db: &impl ConnectionTrait,
    table: &str,
    column: &str,
) -> Result<Option<String>, DbErr> {
    // The column is cast because SQLite lets a TEXT column hold a number.
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!(
                "SELECT CAST({column} AS TEXT), count(*) FROM {table} WHERE {column} IS NOT NULL \
                 GROUP BY 1"
            ),
        ))
        .await?;
    let mut count = 0;
    for row in rows {
        let text = row.try_get_by_index::<String>(0)?;
        // The parser alone also takes `2026-1-05  00:00:00`, which the
        // rewrite's `GLOB` does not match, so the text must print back.
        let old = chrono::NaiveDateTime::parse_from_str(&text, OLD_FORMAT)
            .is_ok_and(|at| at.format(OLD_FORMAT).to_string() == text);
        if !old && chrono::DateTime::parse_from_rfc3339(&text).is_err() {
            count += row.try_get_by_index::<i64>(1)?;
        }
    }
    let noun = if count == 1 { "value" } else { "values" };
    Ok((count > 0).then(|| format!("{count} {noun} in {table}.{column}")))
}

/// Removes ` DEFAULT (datetime('now'))` from every table definition.
///
/// A default lives only in the table's stored `CREATE TABLE` text, so `SQLite`
/// allows this change by editing that text (the second procedure under "Making
/// Other Kinds Of Table Schema Changes" in its `ALTER TABLE` documentation).
/// No row is touched and no table is rebuilt.
async fn remove_defaults<C: ConnectionTrait>(txn: &C) -> Result<(), DbErr> {
    const DEFAULT: &str = " DEFAULT (datetime(''now''))";
    // The edit touches every table, so the checks cover the whole database.
    edit_definitions(
        txn,
        "removing the timestamp defaults",
        &[],
        async |txn: &C| {
            let edited = txn
                .execute_unprepared(&format!(
                    "UPDATE sqlite_schema SET sql = replace(sql, '{DEFAULT}', '') WHERE type = \
                     'table' AND instr(sql, '{DEFAULT}') > 0"
                ))
                .await?;
            Ok(edited.rows_affected() > 0)
        },
    )
    .await?;

    let left = scalar::<i64>(
        txn,
        "SELECT count(*) FROM sqlite_schema WHERE instr(sql, 'datetime(') > 0",
    )
    .await?;
    if left > 0 {
        return Err(DbErr::Migration(format!(
            "removing the timestamp defaults left {left} definitions that still name datetime(). \
             Nothing was changed."
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection};
    use sea_orm_migration::MigratorTrait;

    use super::STAMPS;
    use crate::migration::{
        Migrator,
        schema::testing::{count, migrated_to, text},
    };

    // The number of migrations that run before this one.
    const BEFORE: u32 = 5;

    // A database as the previous migration leaves it, holding the seeded
    // rows and one vehicle with a service, each stamped by its default.
    async fn db_before() -> DatabaseConnection {
        let db = migrated_to(BEFORE).await;
        db.execute_unprepared(
            "INSERT INTO vehicles (id, name, created_at, updated_at, archived_at)
                 VALUES (1, 'Golf', '2024-01-02 03:04:05', '2025-06-07 08:09:10', NULL);
             INSERT INTO service_records (id, vehicle_id, service_date)
                 VALUES (1, 1, '2026-01-05');",
        )
        .await
        .unwrap();
        db
    }

    // How many stamps across the database are not whole-second RFC 3339 in
    // UTC, and how many stamps there are.
    async fn stamps(db: &DatabaseConnection) -> (i64, i64) {
        let (mut other, mut all) = (0, 0);
        for (table, columns) in STAMPS {
            for column in *columns {
                let present = format!("FROM {table} WHERE {column} IS NOT NULL");
                all += count(db, &format!("SELECT count(*) {present}")).await;
                other += count(
                    db,
                    &format!(
                        "SELECT count(*) {present} AND {column} NOT GLOB \
                         '????-??-??T??:??:??+00:00'"
                    ),
                )
                .await;
            }
        }
        (other, all)
    }

    async fn tables_with_a_default(db: &DatabaseConnection) -> i64 {
        count(
            db,
            "SELECT count(*) FROM sqlite_schema WHERE sql LIKE '%datetime(%'",
        )
        .await
    }

    #[tokio::test]
    async fn every_stamp_becomes_rfc_3339_and_keeps_its_instant() {
        let db = db_before().await;
        let (other, all) = stamps(&db).await;
        assert!(all > 20 && other == all, "{other} of {all}");

        Migrator::up(&db, Some(1)).await.unwrap();

        assert_eq!(stamps(&db).await, (0, all));
        assert_eq!(
            text(&db, "SELECT created_at || '|' || updated_at FROM vehicles").await,
            "2024-01-02T03:04:05+00:00|2025-06-07T08:09:10+00:00"
        );
        assert_eq!(
            count(
                &db,
                "SELECT count(*) FROM vehicles WHERE archived_at IS NULL"
            )
            .await,
            1
        );
    }

    #[tokio::test]
    async fn no_column_keeps_a_default_and_the_schema_is_sound() {
        let db = db_before().await;
        assert_eq!(tables_with_a_default(&db).await, 19);

        Migrator::up(&db, Some(1)).await.unwrap();

        assert_eq!(tables_with_a_default(&db).await, 0);
        assert_eq!(text(&db, "PRAGMA integrity_check").await, "ok");
        let refused = db
            .execute_unprepared("INSERT INTO platforms (name) VALUES ('No stamps')")
            .await
            .unwrap_err()
            .to_string();
        assert!(refused.contains("NOT NULL"), "{refused}");
        db.execute_unprepared(
            "INSERT INTO platforms (name, created_at, updated_at) VALUES ('Stamped', \
             '2026-11-14T12:00:00+00:00', '2026-11-14T12:00:00+00:00')",
        )
        .await
        .unwrap();
        // The search triggers still follow a write to the table.
        db.execute_unprepared("UPDATE vehicles SET name = 'Alltrack' WHERE id = 1")
            .await
            .unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT count(*) FROM fts_vehicles WHERE fts_vehicles MATCH 'alltrack'"
            )
            .await,
            1
        );
    }

    // The checks after the schema edit look at the whole database, so they
    // are judged against the same checks made before it.
    #[tokio::test]
    async fn a_broken_link_that_was_already_there_does_not_stop_it() {
        let db = db_before().await;
        db.execute_unprepared(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO service_records (id, vehicle_id, service_date) VALUES (9, 999, \
             '2026-01-05');
             PRAGMA foreign_keys = ON;",
        )
        .await
        .unwrap();

        Migrator::up(&db, Some(1)).await.unwrap();

        assert_eq!(tables_with_a_default(&db).await, 0);
        assert_eq!(stamps(&db).await.0, 0);
    }

    #[tokio::test]
    async fn running_it_again_changes_nothing() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();
        let once = text(&db, "SELECT created_at || updated_at FROM vehicles").await;

        Migrator::down(&db, Some(1)).await.unwrap();
        Migrator::up(&db, Some(1)).await.unwrap();

        assert_eq!(
            text(&db, "SELECT created_at || updated_at FROM vehicles").await,
            once
        );
        assert_eq!(stamps(&db).await.0, 0);
        assert_eq!(text(&db, "PRAGMA integrity_check").await, "ok");
    }

    #[tokio::test]
    async fn a_stamp_in_neither_form_stops_the_migration_before_anything_changes() {
        for bad in ["yesterday", "2026-13-45 99:99:99", "2026-01-05"] {
            let db = db_before().await;
            db.execute_unprepared(&format!("UPDATE service_records SET updated_at = '{bad}'"))
                .await
                .unwrap();
            let (other, _) = stamps(&db).await;

            let err = Migrator::up(&db, Some(1)).await.unwrap_err().to_string();

            assert!(
                err.contains("1 value in service_records.updated_at"),
                "{err}"
            );
            assert_eq!(stamps(&db).await.0, other, "{bad}: no stamp is rewritten");
            assert_eq!(tables_with_a_default(&db).await, 19, "{bad}");
        }
    }

    // No stamp column has a default, so every stamp here was written by the
    // code. The demo seed inserts and updates through the services.
    #[tokio::test]
    async fn every_stamp_the_services_write_is_whole_second_rfc_3339() {
        let db = crate::test_support::test_db().await;
        crate::seed::demo(&db).await.unwrap();

        let (other, all) = stamps(&db).await;

        assert!(all > 100, "{all}");
        assert_eq!(other, 0);
    }
}
