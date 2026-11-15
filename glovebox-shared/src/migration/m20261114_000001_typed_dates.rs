//! Makes sure every stored date can be read as a typed date.
//!
//! A date column is read as a calendar date and an event-time column as a day
//! or a date and time, and a value in any other form makes its whole row
//! unreadable. This migration writes a space for the `T` in an event time and
//! then refuses to finish while any value is in another form. It changes no
//! other value, so running it again is harmless.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

use crate::dates::{self, DayOrTime};

/// Columns that hold a calendar date, `YYYY-MM-DD`.
const DATES: &[(&str, &str)] = &[
    ("appointments", "planned_date"),
    ("appointments", "cancelled_on"),
    ("appointments", "closed_on"),
    ("job_needs", "decided_on"),
    ("need_options", "rejected_on"),
    ("need_options", "chosen_on"),
    ("parts", "purchase_date"),
    ("parts", "installed_date"),
    ("parts", "replaced_date"),
    ("parts", "warranty_expires_on"),
    ("parts", "received_on"),
    ("parts", "returned_on"),
    ("service_records", "service_date"),
    ("service_records", "end_date"),
    ("todos", "done_on"),
    ("vehicles", "purchase_date"),
    ("vehicles", "sold_date"),
    ("vehicles", "warranty_expires_on"),
    ("work_items", "revisit_by_date"),
];

/// Columns that hold a day or a date and time, `YYYY-MM-DD[ HH:MM:SS]`.
const EVENT_TIMES: &[(&str, &str)] = &[
    ("incidents", "occurred_at"),
    ("incident_followups", "occurred_at"),
    ("mileage_log", "recorded_at"),
];

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for (table, column) in EVENT_TIMES {
            db.execute_unprepared(&format!(
                "UPDATE {table} SET {column} = replace({column}, 'T', ' ') WHERE {column} GLOB \
                 '????-??-??T??:??:??'"
            ))
            .await?;
        }

        // Each value is judged by the parser that reads it, so the guard and
        // the reader cannot disagree. SQLite's `date()` and `datetime()` do:
        // they take hour 24 and refuse a leap second.
        let mut unreadable = Vec::new();
        for (table, column) in DATES {
            let readable = |text: &str| dates::parse_date(column, text).is_ok();
            unreadable.extend(count_unreadable(db, table, column, readable).await?);
        }
        for (table, column) in EVENT_TIMES {
            let readable = |text: &str| text.parse::<DayOrTime>().is_ok();
            unreadable.extend(count_unreadable(db, table, column, readable).await?);
        }
        if unreadable.is_empty() {
            return Ok(());
        }
        Err(DbErr::Migration(format!(
            "these columns hold a date that is not written YYYY-MM-DD (or, for an event time, \
             YYYY-MM-DD HH:MM:SS): {}. Correct the values, then start again.",
            unreadable.join(", ")
        )))
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Nothing is undone: every value this migration leaves is one the
        // string-typed code also reads.
        Ok(())
    }
}

/// Describes the values of `table.column` that `readable` turns down, as
/// `Some` text such as `2 values in parts.purchase_date`, or `None` when
/// there are none.
async fn count_unreadable(
    db: &SchemaManagerConnection<'_>,
    table: &str,
    column: &str,
    readable: impl Fn(&str) -> bool,
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
        if !readable(&row.try_get_by_index::<String>(0)?) {
            count += row.try_get_by_index::<i64>(1)?;
        }
    }
    let noun = if count == 1 { "value" } else { "values" };
    Ok((count > 0).then(|| format!("{count} {noun} in {table}.{column}")))
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::{db::connect_options, migration::Migrator};

    // The number of migrations that run before this one.
    const BEFORE: u32 = 4;

    async fn db_before() -> DatabaseConnection {
        let db = Database::connect(connect_options(None)).await.unwrap();
        Migrator::up(&db, Some(BEFORE)).await.unwrap();
        db.execute_unprepared(
            "INSERT INTO vehicles (id, name, purchase_date) VALUES (1, 'Golf', '2020-03-04');
             INSERT INTO service_records (id, vehicle_id, service_date, end_date)
                 VALUES (1, 1, '2026-01-05', NULL);
             INSERT INTO incidents (id, vehicle_id, category, title, occurred_at)
                 VALUES (1, 1, 'note', 'Day only', '2026-01-05'),
                        (2, 1, 'note', 'Timed', '2026-01-05 08:30:00');
             INSERT INTO mileage_log (id, vehicle_id, mileage, recorded_at)
                 VALUES (1, 1, 100, '2026-01-06T09:15:00');",
        )
        .await
        .unwrap();
        db
    }

    async fn text(db: &DatabaseConnection, sql: &str) -> String {
        db.query_one_raw(Statement::from_string(DatabaseBackend::Sqlite, sql))
            .await
            .unwrap()
            .unwrap()
            .try_get_by_index::<String>(0)
            .unwrap()
    }

    // Every date and event-time value in the fixture, as one string.
    async fn snapshot(db: &DatabaseConnection) -> String {
        text(
            db,
            "SELECT (SELECT purchase_date FROM vehicles) || '|' ||
                    (SELECT service_date FROM service_records) || '|' ||
                    (SELECT group_concat(occurred_at, ',') FROM incidents) || '|' ||
                    (SELECT recorded_at FROM mileage_log)",
        )
        .await
    }

    #[tokio::test]
    async fn a_t_in_an_event_time_becomes_a_space_and_nothing_else_changes() {
        let db = db_before().await;

        Migrator::up(&db, Some(1)).await.unwrap();

        assert_eq!(
            snapshot(&db).await,
            "2020-03-04|2026-01-05|2026-01-05,2026-01-05 08:30:00|2026-01-06 09:15:00"
        );
    }

    #[tokio::test]
    async fn a_value_the_reader_takes_passes_even_where_sqlite_would_not() {
        let db = db_before().await;
        db.execute_unprepared(
            "UPDATE incidents SET occurred_at = '2026-12-31 23:59:60' WHERE id = 2",
        )
        .await
        .unwrap();

        Migrator::up(&db, Some(1)).await.unwrap();
    }

    #[tokio::test]
    async fn running_it_again_changes_nothing() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();
        let once = snapshot(&db).await;

        Migrator::down(&db, Some(1)).await.unwrap();
        Migrator::up(&db, Some(1)).await.unwrap();

        assert_eq!(snapshot(&db).await, once);
    }

    #[tokio::test]
    async fn a_value_that_is_not_a_real_date_stops_the_migration_and_is_named() {
        for (sql, named) in [
            (
                "UPDATE service_records SET service_date = '2026-02-30'",
                "service_records.service_date",
            ),
            (
                "UPDATE vehicles SET purchase_date = '3/4/2020'",
                "vehicles.purchase_date",
            ),
            (
                "UPDATE service_records SET end_date = '2026-01-05 10:00:00'",
                "service_records.end_date",
            ),
            (
                "UPDATE incidents SET occurred_at = '2026-01-05 25:00:00' WHERE id = 2",
                "incidents.occurred_at",
            ),
            (
                "UPDATE mileage_log SET recorded_at = 'last week'",
                "mileage_log.recorded_at",
            ),
            // SQLite's own date functions take hour 24, and the reader does not.
            (
                "UPDATE incidents SET occurred_at = '2026-01-05 24:00:00' WHERE id = 2",
                "incidents.occurred_at",
            ),
        ] {
            let db = db_before().await;
            db.execute_unprepared(sql).await.unwrap();

            let err = Migrator::up(&db, Some(1)).await.unwrap_err().to_string();

            assert!(err.contains(named), "{sql}: {err}");
            assert!(err.contains("1 value"), "{sql}: {err}");
        }
    }
}
