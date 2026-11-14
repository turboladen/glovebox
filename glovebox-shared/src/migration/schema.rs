//! Column, foreign-key and index builders shared by the schema migrations.

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
