//! Column, foreign-key and index builders, the soundness checks and the
//! stored-definition edit shared by the migrations.

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

/// Every row `PRAGMA integrity_check` and `PRAGMA foreign_key_check` report,
/// for `tables` or, when it is empty, the whole database.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Soundness {
    pub integrity: Vec<String>,
    pub broken_links: Vec<String>,
}

pub(super) async fn soundness(
    db: &impl ConnectionTrait,
    tables: &[&str],
) -> Result<Soundness, DbErr> {
    let scopes: Vec<String> = if tables.is_empty() {
        vec![String::new()]
    } else {
        tables.iter().map(|t| format!("({t})")).collect()
    };
    let mut out = Soundness {
        integrity: Vec::new(),
        broken_links: Vec::new(),
    };
    for scope in scopes {
        for row in rows(db, &format!("PRAGMA integrity_check{scope}")).await? {
            out.integrity.push(row.try_get_by_index::<String>(0)?);
        }
        for row in rows(db, &format!("PRAGMA foreign_key_check{scope}")).await? {
            let table = row.try_get_by_index::<String>(0)?;
            let rowid = row.try_get_by_index::<Option<i64>>(1)?;
            let parent = row.try_get_by_index::<String>(2)?;
            out.broken_links
                .push(format!("{table} row {rowid:?} names a missing {parent}"));
        }
    }
    Ok(out)
}

async fn rows(db: &impl ConnectionTrait, sql: &str) -> Result<Vec<sea_orm::QueryResult>, DbErr> {
    db.query_all_raw(Statement::from_string(DatabaseBackend::Sqlite, sql))
        .await
}

/// Edits stored table definitions in `sqlite_schema` on `txn`, which must be
/// a transaction holding the write lock. `edit` runs with
/// `PRAGMA writable_schema` on and returns whether it changed a definition;
/// when it did, `schema_version` is raised so every connection reloads the
/// schema. Returns the soundness of `tables` (the whole database when empty)
/// before and after, for the caller to compare.
pub(super) async fn edit_definitions<C, F>(
    txn: &C,
    tables: &[&str],
    edit: F,
) -> Result<(Soundness, Soundness), DbErr>
where
    C: ConnectionTrait,
    F: AsyncFnOnce(&C) -> Result<bool, DbErr>,
{
    let version = scalar::<i64>(txn, "PRAGMA schema_version").await?;
    let before = soundness(txn, tables).await?;
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
    let after = soundness(txn, tables).await?;
    Ok((before, after))
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

    use super::{soundness, testing::migrated_to};
    use crate::migration::Migrator;

    // Two jobs name a vehicle that does not exist. The check of `work_items`
    // reports both, and the check of `todos` reports neither.
    #[tokio::test]
    async fn soundness_reports_every_broken_link_of_the_tables_named() {
        let db = migrated_to(
            u32::try_from(<Migrator as sea_orm_migration::MigratorTrait>::migrations().len())
                .unwrap(),
        )
        .await;
        db.execute_unprepared(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO work_items (id, vehicle_id, title, status, created_at, updated_at)
                 VALUES (1, 98, 'A', 'planned', 'x', 'x'), (2, 99, 'B', 'planned', 'x', 'x');
             PRAGMA foreign_keys = ON;",
        )
        .await
        .unwrap();

        let jobs = soundness(&db, &["work_items"]).await.unwrap();
        assert_eq!(jobs.integrity, ["ok"]);
        assert_eq!(jobs.broken_links.len(), 2, "{:?}", jobs.broken_links);
        let todos = soundness(&db, &["todos"]).await.unwrap();
        crate::test_support::assert_empty(&todos.broken_links);
        assert_eq!(soundness(&db, &[]).await.unwrap().broken_links.len(), 2);
    }
}
