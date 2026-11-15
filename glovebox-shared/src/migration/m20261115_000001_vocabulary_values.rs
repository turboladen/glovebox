//! Makes sure every stored vocabulary value can be read as its enum.
//!
//! A column that takes its value from a fixed list is read as an enum, and a
//! value outside the list makes its whole row unreadable. This migration
//! refuses to finish while any such column holds one. It changes nothing, so
//! running it again is harmless.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

const SUPPLIERS: &[&str] = &["self", "shop"];
const PAYERS: &[&str] = &["self", "insurance", "third_party", "manufacturer"];

/// Each vocabulary column with the values it may hold. A null is always
/// allowed here; the column's own `NOT NULL` decides whether one can exist.
// The lists are the values allowed when this migration was written, and they
// stay as they are. A value an enum gains later is in no database that has
// yet to run this migration, and one it loses must still pass here so that
// the migration that rewrites those rows can run after this one.
const COLUMNS: &[(&str, &str, &[&str])] = &[
    (
        "incidents",
        "category",
        &[
            "general",
            "noise",
            "leak",
            "warning_light",
            "cosmetic",
            "performance",
            "obd_code",
            "damage",
            "accident",
            "note",
            "shop_observation",
        ],
    ),
    ("incident_service_links", "role", &["noticed", "resolved"]),
    ("service_records", "paid_by", PAYERS),
    ("service_records", "performed_by", &["shop", "self"]),
    ("service_record_line_items", "paid_by", PAYERS),
    (
        "service_record_line_items",
        "work_kind",
        &[
            "maintenance",
            "mod",
            "repair",
            "inspection",
            "recall",
            "fee",
        ],
    ),
    (
        "parts",
        "status",
        &["purchased", "installed", "replaced", "returned"],
    ),
    ("parts", "supplied_by", SUPPLIERS),
    ("work_items", "status", &["planned", "done", "dropped"]),
    ("job_needs", "supplied_by", SUPPLIERS),
    ("job_needs", "approach", &["replacement", "upgrade"]),
    (
        "need_options",
        "fitment",
        &["confirmed", "unconfirmed", "does_not_fit"],
    ),
];

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let mut unreadable = Vec::new();
        for (table, column, allowed) in COLUMNS {
            unreadable.extend(describe_unlisted(db, table, column, allowed).await?);
        }
        if unreadable.is_empty() {
            return Ok(());
        }
        Err(DbErr::Migration(format!(
            "these columns hold a value that is not in the column's list: {}. Correct the values, \
             then start again.",
            unreadable.join("; ")
        )))
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Nothing is undone, because nothing was changed.
        Ok(())
    }
}

/// Describes the values of `table.column` that are not in `allowed`, as
/// `Some` text such as `2 values in parts.status ('lost') (allowed:
/// purchased, installed)`, or `None` when there are none.
async fn describe_unlisted(
    db: &SchemaManagerConnection<'_>,
    table: &str,
    column: &str,
    allowed: &[&str],
) -> Result<Option<String>, DbErr> {
    // The column is cast because SQLite lets a TEXT column hold a number.
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!(
                "SELECT CAST({column} AS TEXT), count(*) FROM {table} WHERE {column} IS NOT NULL \
                 GROUP BY 1 ORDER BY 1"
            ),
        ))
        .await?;
    let mut count = 0;
    let mut unlisted = Vec::new();
    for row in rows {
        let text = row.try_get_by_index::<String>(0)?;
        if !allowed.contains(&text.as_str()) {
            count += row.try_get_by_index::<i64>(1)?;
            unlisted.push(format!("'{text}'"));
        }
    }
    let noun = if count == 1 { "value" } else { "values" };
    Ok((count > 0).then(|| {
        format!(
            "{count} {noun} in {table}.{column} ({}) (allowed: {})",
            unlisted.join(", "),
            allowed.join(", ")
        )
    }))
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection};
    use sea_orm_migration::MigratorTrait;

    use super::COLUMNS;
    use crate::{
        entities::{
            incident::IncidentCategory,
            incident_service_link::LinkRole,
            service_record::{Payer, Performer},
            service_record_line_item::WorkKind,
        },
        migration::{Migrator, schema::testing::migrated_to},
        vocab::{self, Vocabulary},
    };

    // The number of migrations that run before this one.
    const BEFORE: u32 = 7;

    // One row in every table with a vocabulary column, each holding a listed
    // value or, where the column allows it, a null.
    async fn db_before() -> DatabaseConnection {
        let db = migrated_to(BEFORE).await;
        db.execute_unprepared(
            "INSERT INTO vehicles (id, name, created_at, updated_at)
                 VALUES (1, 'Golf', '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00');
             INSERT INTO service_records
                     (id, vehicle_id, service_date, paid_by, performed_by, created_at, updated_at)
                 VALUES (1, 1, '2026-01-05', 'manufacturer', 'self',
                         '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00');
             INSERT INTO service_record_line_items
                     (id, service_record_id, description, work_kind, paid_by, created_at,
                      updated_at)
                 VALUES (1, 1, 'Oil', 'maintenance', NULL,
                         '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00'),
                        (2, 1, 'Tax', 'fee', 'self',
                         '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00');
             INSERT INTO incidents
                     (id, vehicle_id, category, title, occurred_at, created_at, updated_at)
                 VALUES (1, 1, 'shop_observation', 'Seep', '2026-01-05',
                         '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00');
             INSERT INTO incident_service_links (incident_id, service_record_id, role)
                 VALUES (1, 1, 'noticed');
             INSERT INTO parts (id, vehicle_id, name, status, supplied_by, created_at, updated_at)
                 VALUES (1, 1, 'Filter', 'returned', 'shop',
                         '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00');
             INSERT INTO work_items (id, vehicle_id, title, status, created_at, updated_at)
                 VALUES (1, 1, 'Brakes', 'dropped',
                         '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00');
             INSERT INTO job_needs
                     (id, work_item_id, name, supplied_by, approach, created_at, updated_at)
                 VALUES (1, 1, 'Pads', NULL, NULL,
                         '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00'),
                        (2, 1, 'Rotors', 'self', 'upgrade',
                         '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00');
             INSERT INTO need_options (id, need_id, description, fitment, created_at, updated_at)
                 VALUES (1, 2, 'Big rotors', 'does_not_fit',
                         '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00');",
        )
        .await
        .unwrap();
        db
    }

    #[tokio::test]
    async fn listed_values_and_nulls_pass_and_a_second_run_passes_too() {
        let db = db_before().await;

        Migrator::up(&db, Some(1)).await.unwrap();
        Migrator::down(&db, Some(1)).await.unwrap();
        Migrator::up(&db, Some(1)).await.unwrap();
    }

    #[tokio::test]
    async fn a_value_outside_the_list_stops_the_migration_and_is_named() {
        for (table, column, _) in COLUMNS {
            let db = db_before().await;
            db.execute_unprepared(&format!("UPDATE {table} SET {column} = 'Bogus'"))
                .await
                .unwrap();

            let err = Migrator::up(&db, Some(1)).await.unwrap_err().to_string();

            assert!(
                err.contains(&format!("{table}.{column} ('Bogus')")),
                "{err}"
            );
        }
    }

    #[tokio::test]
    async fn the_count_covers_every_row_and_the_allowed_values_are_shown() {
        let db = db_before().await;
        db.execute_unprepared("UPDATE service_record_line_items SET work_kind = 'labour'")
            .await
            .unwrap();

        let err = Migrator::up(&db, Some(1)).await.unwrap_err().to_string();

        assert!(
            err.contains("2 values in service_record_line_items.work_kind"),
            "{err}"
        );
        assert!(err.contains("maintenance, mod, repair"), "{err}");
    }

    // Each text the guard lets through must be one the column's enum reads. A
    // misspelled text in an enum passes its own round-trip tests and fails
    // only on rows that already hold the listed spelling.
    #[test]
    fn the_enum_of_a_column_reads_every_value_the_guard_allows() {
        fn check<V: Vocabulary>(table: &str, column: &str) {
            let (_, _, allowed) = COLUMNS
                .iter()
                .find(|(t, c, _)| (*t, *c) == (table, column))
                .unwrap_or_else(|| panic!("{table}.{column} must be guarded"));
            for text in *allowed {
                assert!(
                    vocab::parse::<V>(column, text).is_ok(),
                    "{table}.{column} allows '{text}', which its enum does not read"
                );
            }
        }

        check::<IncidentCategory>("incidents", "category");
        check::<LinkRole>("incident_service_links", "role");
        check::<Payer>("service_records", "paid_by");
        check::<Payer>("service_record_line_items", "paid_by");
        check::<Performer>("service_records", "performed_by");
        check::<WorkKind>("service_record_line_items", "work_kind");
    }
}
