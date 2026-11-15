//! Column, foreign-key and index builders and the whole-database checks
//! shared by the migrations.

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

/// The result of `PRAGMA integrity_check` and the number of rows
/// `PRAGMA foreign_key_check` reports.
pub(super) async fn soundness(db: &impl ConnectionTrait) -> Result<(String, usize), DbErr> {
    let integrity = scalar::<String>(db, "PRAGMA integrity_check").await?;
    let broken_links = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA foreign_key_check",
        ))
        .await?
        .len();
    Ok((integrity, broken_links))
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
