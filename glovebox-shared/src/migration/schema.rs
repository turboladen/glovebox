//! Column, foreign-key, index and full-text index builders, the soundness
//! checks and the stored-definition edit shared by the migrations.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

pub(super) fn c(name: &str) -> ColumnDef {
    ColumnDef::new(Alias::new(name))
}

pub(super) fn pk() -> ColumnDef {
    c("id")
        .integer()
        .not_null()
        .auto_increment()
        .primary_key()
        .to_owned()
}

pub(super) fn int(name: &str) -> ColumnDef {
    c(name).integer().to_owned()
}

pub(super) fn text(name: &str) -> ColumnDef {
    c(name).text().to_owned()
}

pub(super) fn int_not_null(name: &str) -> ColumnDef {
    c(name).integer().not_null().to_owned()
}

pub(super) fn text_not_null(name: &str) -> ColumnDef {
    c(name).text().not_null().to_owned()
}

pub(super) fn text_default(name: &str, default: &str) -> ColumnDef {
    c(name).text().not_null().default(default).to_owned()
}

/// A `TEXT NOT NULL` timestamp that defaults to the insert time.
///
/// `m20261114_000002_utc_timestamps` removes this default from every table, so
/// a table created after it declares its stamps with [`text_not_null`].
pub(super) fn stamp(name: &str) -> ColumnDef {
    c(name)
        .text()
        .not_null()
        .default(Expr::cust("datetime('now')"))
        .to_owned()
}

pub(super) fn fk(
    table: &str,
    column: &str,
    parent: &str,
    on_delete: ForeignKeyAction,
) -> ForeignKeyCreateStatement {
    ForeignKey::create()
        .from(Alias::new(table), Alias::new(column))
        .to(Alias::new(parent), Alias::new("id"))
        .on_delete(on_delete)
        .to_owned()
}

pub(super) fn index(name: &str, table: &str, cols: &[&str]) -> IndexCreateStatement {
    let mut idx = Index::create();
    idx.if_not_exists().name(name).table(Alias::new(table));
    for col in cols {
        idx.col(Alias::new(*col));
    }
    idx
}

/// A partial index over `cols`, restricted to rows where `predicate_col IS NOT NULL`.
pub(super) fn partial_index(
    name: &str,
    table: &str,
    cols: &[&str],
    predicate_col: &str,
    unique: bool,
) -> IndexCreateStatement {
    let mut idx = index(name, table, cols);
    if unique {
        idx.unique();
    }
    idx.and_where(Expr::col(Alias::new(predicate_col)).is_not_null())
        .to_owned()
}

/// Every row `PRAGMA integrity_check` and `PRAGMA foreign_key_check` report
/// over the whole database, each list sorted.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Soundness {
    pub integrity: Vec<String>,
    pub broken_links: Vec<String>,
}

// More rows than any database here could report, because the check stops at
// 100 by default and a cut-off list could hide a new fault.
const INTEGRITY_ROWS: u32 = 1_000_000;

/// Runs both checks over the whole database. The rows are sorted, because
/// SQLite lists them in an order that can change when the schema reloads.
pub(super) async fn soundness(db: &impl ConnectionTrait) -> Result<Soundness, DbErr> {
    let mut integrity = Vec::new();
    for row in rows(db, &format!("PRAGMA integrity_check({INTEGRITY_ROWS})")).await? {
        integrity.push(row.try_get_by_index::<String>(0)?);
    }
    let mut broken_links = Vec::new();
    for row in rows(db, "PRAGMA foreign_key_check").await? {
        let table = row.try_get_by_index::<String>(0)?;
        let rowid = row.try_get_by_index::<Option<i64>>(1)?;
        let parent = row.try_get_by_index::<String>(2)?;
        broken_links.push(format!("{table} row {rowid:?} names a missing {parent}"));
    }
    integrity.sort_unstable();
    broken_links.sort_unstable();
    Ok(Soundness {
        integrity,
        broken_links,
    })
}

async fn rows(db: &impl ConnectionTrait, sql: &str) -> Result<Vec<sea_orm::QueryResult>, DbErr> {
    db.query_all_raw(Statement::from_string(DatabaseBackend::Sqlite, sql))
        .await
}

/// Edits stored table definitions in `sqlite_schema` on `txn`, which must be
/// a transaction holding the write lock. `edit` runs with
/// `PRAGMA writable_schema` on and returns whether it changed a definition;
/// when it did, `schema_version` is raised so every connection reloads the
/// schema. `what` names the change in the error.
///
/// # Errors
/// The error `edit` returns, or `DbErr::Migration` when the integrity or
/// foreign-key checks of the whole database differ after the edit. The
/// caller drops its transaction on either error, which rolls the edit back.
pub(super) async fn edit_definitions<C, F>(txn: &C, what: &str, edit: F) -> Result<(), DbErr>
where
    C: ConnectionTrait,
    F: AsyncFnOnce(&C) -> Result<bool, DbErr>,
{
    // The checks cover the whole database, because foreign keys that point
    // into an edited table and `sqlite_schema` itself can break as well.
    let version = scalar::<i64>(txn, "PRAGMA schema_version").await?;
    let before = soundness(txn).await?;
    txn.execute_unprepared("PRAGMA writable_schema = ON")
        .await?;
    let edited = edit(txn).await;
    // The pragma belongs to the connection, which goes back to the pool, so it
    // is cleared whether or not the edit worked.
    txn.execute_unprepared("PRAGMA writable_schema = OFF")
        .await?;
    if edited? {
        txn.execute_unprepared(&format!("PRAGMA schema_version = {}", version + 1))
            .await?;
    }
    // A fault that was there before the edit is not the edit's doing, so only
    // a change stops it.
    let after = soundness(txn).await?;
    if after != before {
        return Err(DbErr::Migration(format!(
            "{what} changed the database's checks: integrity_check reported {} rows and reports \
             {}, and foreign_key_check reported {} broken links and reports {}. Nothing was \
             changed.",
            before.integrity.len(),
            after.integrity.len(),
            before.broken_links.len(),
            after.broken_links.len()
        )));
    }
    Ok(())
}

pub(super) async fn scalar<T: sea_orm::TryGetable>(
    db: &impl ConnectionTrait,
    sql: &str,
) -> Result<T, DbErr> {
    let row = db
        .query_one_raw(Statement::from_string(DatabaseBackend::Sqlite, sql))
        .await?
        .ok_or_else(|| DbErr::Migration(format!("`{sql}` returned no row")))?;
    row.try_get_by_index::<T>(0)
}

/// A rule one row of `table` must keep, added as the table constraint
/// `CONSTRAINT "name" CHECK (holds)`.
pub(super) struct Check {
    pub table: &'static str,
    pub name: &'static str,
    pub holds: &'static str,
}

// The ids a refusal names for each broken rule, enough to find the rows.
const IDS_SHOWN: usize = 10;

/// Adds each check its table lacks by editing the table's stored definition
/// on `txn`, a transaction holding the write lock. Triggers and indexes are
/// rows of their own in `sqlite_schema`, so they are left as they are.
///
/// # Errors
/// `DbErr::Migration`, with nothing changed, while a stored row breaks a
/// check (the message names the table, the check, the count and the first
/// ids), when a definition does not end in `)`, or when the soundness checks
/// of [`edit_definitions`] differ afterwards.
pub(super) async fn add_checks<C: ConnectionTrait>(
    txn: &C,
    what: &str,
    checks: &[Check],
) -> Result<(), DbErr> {
    // SQLite does not test stored rows against a constraint added to the
    // definition, so they are tested here. A CHECK fails only when its
    // expression is false, which is exactly when `NOT (holds)` is true.
    let mut broken = Vec::new();
    for check in checks {
        let ids: Vec<i64> = rows(
            txn,
            &format!(
                "SELECT id FROM {} WHERE NOT ({}) ORDER BY id",
                check.table, check.holds
            ),
        )
        .await?
        .iter()
        .map(|row| row.try_get_by_index::<i64>(0))
        .collect::<Result<_, _>>()?;
        if !ids.is_empty() {
            let shown: Vec<String> = ids.iter().take(IDS_SHOWN).map(i64::to_string).collect();
            let more = if ids.len() > IDS_SHOWN { ", ..." } else { "" };
            let (noun, verb) = if ids.len() == 1 {
                ("row", "breaks")
            } else {
                ("rows", "break")
            };
            broken.push(format!(
                "{} {noun} of {} {verb} {} (id {}{more})",
                ids.len(),
                check.table,
                check.name,
                shown.join(", ")
            ));
        }
    }
    if !broken.is_empty() {
        return Err(DbErr::Migration(format!(
            "{what} stopped, because these rows break a rule: {}. Correct them, then start again. \
             Nothing was changed.",
            broken.join("; ")
        )));
    }

    let mut missing = Vec::new();
    for check in checks {
        if !has_check(txn, check).await? {
            missing.push(check);
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    edit_definitions(txn, what, async |txn: &C| {
        for check in &missing {
            // The closing parenthesis of the definition is replaced by the
            // constraint and a new one. A definition spelled another way
            // matches nothing and is reported below.
            txn.execute_unprepared(&format!(
                "UPDATE sqlite_schema SET sql = rtrim(substr(rtrim(sql), 1, length(rtrim(sql)) - \
                 1)) || ', CONSTRAINT \"{name}\" CHECK ({holds}) )' WHERE type = 'table' AND name \
                 = '{table}' AND rtrim(sql) LIKE '%)'",
                name = check.name,
                holds = check.holds.replace('\'', "''"),
                table = check.table,
            ))
            .await?;
        }
        Ok(true)
    })
    .await?;

    let mut absent = Vec::new();
    for check in missing {
        if !has_check(txn, check).await? {
            absent.push(format!("{}.{}", check.table, check.name));
        }
    }
    if !absent.is_empty() {
        return Err(DbErr::Migration(format!(
            "{what} did not finish: no constraint was added for: [{}]. Nothing was changed.",
            absent.join(", ")
        )));
    }
    Ok(())
}

async fn has_check(db: &impl ConnectionTrait, check: &Check) -> Result<bool, DbErr> {
    scalar::<bool>(
        db,
        &format!(
            "SELECT count(*) > 0 FROM sqlite_schema WHERE type = 'table' AND name = '{}' AND \
             instr(sql, 'CONSTRAINT \"{}\" ') > 0",
            check.table, check.name
        ),
    )
    .await
}

/// One external-content FTS5 index over `content`'s `cols`. Every column
/// must exist on the content table, because FTS5 reads values back by rowid.
pub(super) struct FtsSpec {
    pub content: &'static str,
    pub fts: &'static str,
    pub cols: &'static [&'static str],
}

/// Creates each index with the AFTER INSERT/DELETE/UPDATE triggers that keep
/// it in step with its content table. Every statement is `IF NOT EXISTS`.
pub(super) async fn create_fts(
    manager: &SchemaManager<'_>,
    specs: &[FtsSpec],
) -> Result<(), DbErr> {
    let db = manager.get_connection();
    for spec in specs {
        let FtsSpec { content, fts, .. } = spec;
        let cols = spec.cols.join(", ");
        let new_vals = spec
            .cols
            .iter()
            .map(|c| format!("new.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let old_vals = spec
            .cols
            .iter()
            .map(|c| format!("old.{c}"))
            .collect::<Vec<_>>()
            .join(", ");

        db.execute_unprepared(&format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS {fts} USING fts5({cols}, content='{content}', \
             content_rowid='id')"
        ))
        .await?;
        db.execute_unprepared(&format!(
            "CREATE TRIGGER IF NOT EXISTS {content}_fts_ai AFTER INSERT ON {content} BEGIN INSERT \
             INTO {fts}(rowid, {cols}) VALUES (new.id, {new_vals}); END"
        ))
        .await?;
        // External-content FTS5 deletes take the OLD column values alongside the
        // special 'delete' command.
        db.execute_unprepared(&format!(
            "CREATE TRIGGER IF NOT EXISTS {content}_fts_ad AFTER DELETE ON {content} BEGIN INSERT \
             INTO {fts}({fts}, rowid, {cols}) VALUES ('delete', old.id, {old_vals}); END"
        ))
        .await?;
        db.execute_unprepared(&format!(
            "CREATE TRIGGER IF NOT EXISTS {content}_fts_au AFTER UPDATE ON {content} BEGIN INSERT \
             INTO {fts}({fts}, rowid, {cols}) VALUES ('delete', old.id, {old_vals}); INSERT INTO \
             {fts}(rowid, {cols}) VALUES (new.id, {new_vals}); END"
        ))
        .await?;
    }
    Ok(())
}

/// Drops each index's triggers, then the index.
pub(super) async fn drop_fts(manager: &SchemaManager<'_>, specs: &[FtsSpec]) -> Result<(), DbErr> {
    let db = manager.get_connection();
    for FtsSpec { content, fts, .. } in specs {
        for suffix in ["ai", "ad", "au"] {
            db.execute_unprepared(&format!("DROP TRIGGER IF EXISTS {content}_fts_{suffix}"))
                .await?;
        }
        db.execute_unprepared(&format!("DROP TABLE IF EXISTS {fts}"))
            .await?;
    }
    Ok(())
}

/// What the migration test modules share.
#[cfg(test)]
pub(super) mod testing {
    use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::{db::connect_options, migration::Migrator};

    /// An in-memory database with the first `count` migrations applied.
    /// Panics if one fails.
    pub async fn migrated_to(count: u32) -> DatabaseConnection {
        let db = Database::connect(connect_options(None)).await.unwrap();
        Migrator::up(&db, Some(count)).await.unwrap();
        db
    }

    /// The first column of the first row of `sql`, as text. Panics if the
    /// query fails or returns no row.
    pub async fn text(db: &DatabaseConnection, sql: &str) -> String {
        first_row(db, sql).await.try_get_by_index(0).unwrap()
    }

    /// The first column of the first row of `sql`, as an integer. Panics if
    /// the query fails or returns no row.
    pub async fn count(db: &DatabaseConnection, sql: &str) -> i64 {
        first_row(db, sql).await.try_get_by_index(0).unwrap()
    }

    /// The stored definitions of the named tables, ordered by table name and
    /// joined with `;`. Panics if none exists.
    pub async fn table_sql(db: &DatabaseConnection, tables: &[&str]) -> String {
        let names = quoted(tables);
        text(
            db,
            &format!(
                "SELECT group_concat(sql, ';') FROM (SELECT sql FROM sqlite_schema WHERE type = \
                 'table' AND name IN ({names}) ORDER BY name)"
            ),
        )
        .await
    }

    /// The stored indexes and triggers of the named tables, ordered by name
    /// and joined with `;`. Panics if there are none.
    pub async fn surrounding_sql(db: &DatabaseConnection, tables: &[&str]) -> String {
        let names = quoted(tables);
        text(
            db,
            &format!(
                "SELECT group_concat(sql, ';') FROM (SELECT sql FROM sqlite_schema WHERE tbl_name \
                 IN ({names}) AND type <> 'table' ORDER BY name)"
            ),
        )
        .await
    }

    fn quoted(names: &[&str]) -> String {
        names
            .iter()
            .map(|name| format!("'{name}'"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Runs FTS5's `integrity-check` on each named index. Panics if one
    /// fails.
    pub async fn fts_integrity(db: &DatabaseConnection, indexes: &[&str]) {
        for fts in indexes {
            db.execute_unprepared(&format!(
                "INSERT INTO {fts}({fts}) VALUES ('integrity-check')"
            ))
            .await
            .unwrap();
        }
    }

    async fn first_row(db: &DatabaseConnection, sql: &str) -> sea_orm::QueryResult {
        db.query_one_raw(Statement::from_string(DatabaseBackend::Sqlite, sql))
            .await
            .unwrap()
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::ConnectionTrait;

    use super::{edit_definitions, scalar, soundness, testing::table_sql};

    // Rows that already break a key are reported in a stable order, and an
    // edit that breaks another key is refused with writable_schema off and,
    // once the transaction drops, the definition as it was.
    #[tokio::test]
    async fn an_edit_that_breaks_a_key_is_refused() {
        use sea_orm::TransactionTrait;

        let db = crate::test_support::test_db().await;
        db.execute_unprepared(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO work_items (id, vehicle_id, title, status, est_cost_cents, created_at,
                 updated_at)
                 VALUES (1, 98, 'A', 'planned', 12345, 'x', 'x'),
                        (2, 99, 'B', 'planned', NULL, 'x', 'x');
             PRAGMA foreign_keys = ON;",
        )
        .await
        .unwrap();
        let checked = soundness(&db).await.unwrap();
        assert_eq!(checked.integrity, ["ok"]);
        assert_eq!(checked.broken_links.len(), 2, "{:?}", checked.broken_links);
        let definition = || table_sql(&db, &["work_items"]);
        let before = definition().await;

        let txn = db.begin().await.unwrap();
        let err = edit_definitions(&txn, "the test edit", async |txn| {
            txn.execute_unprepared(
                "UPDATE sqlite_schema SET sql = replace(sql, '\"est_cost_cents\" integer,', \
                 '\"est_cost_cents\" integer REFERENCES \"vehicles\" (\"id\"),') WHERE name = \
                 'work_items'",
            )
            .await?;
            Ok(true)
        })
        .await
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("reported 2 broken links and reports 3"),
            "{err}"
        );
        assert_eq!(
            scalar::<i64>(&txn, "PRAGMA writable_schema").await.unwrap(),
            0
        );
        drop(txn);
        assert_eq!(definition().await, before);
    }
}
