---
name: new-migration
description: |
  Generate a SeaORM migration for SQLite with correct patterns for this project.
  Handles table creation, ALTER TABLE, indexes, foreign keys, and DeriveIden enums.
  Trigger on: "new migration", "add migration", "add table", "alter table",
  "add column", "database change".
user_invocable: true
---

# SeaORM Migration Generator

When the user needs a new migration, determine the next sequence number and create the file.

## Step 1: Determine Migration Number

The schema is one squashed baseline, `m20261112_000001_initial_schema`, followed by the seed
migration `m20261112_000002_seed_vw_mqb`. New migrations go after those. Check
`glovebox-shared/src/migration/mod.rs` for the current highest number; the next migration is `N+1`.

File naming: `m{YYYYMMDD}_{NNNNNN}_{descriptive_name}.rs`
- Use the date the migration is written, and keep the sequence number increasing (`000003` is next
  after the seed). Migrations run in vector order, so the order of `migrations()` is what counts.
- Pad the sequence number to 6 digits.

Migrations live in `glovebox-shared/src/migration/`. Read `m20261112_000001_initial_schema.rs` first:
it shows the helpers (`pk()`, `text()`, `int()`, `stamp()`, `fk()`, `index()`) that build columns with
`Alias::new`, and new files can define the same small helpers or use `DeriveIden` enums.

## Step 2: Create Migration File

All APIs are `sea_orm_migration::prelude::*` from SeaORM 2.0. A migration is a unit struct with
`#[derive(DeriveMigrationName)]` and an `#[async_trait::async_trait]` `impl MigrationTrait`.

### Template: New Table

```rust
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table({TableEnum}::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new({TableEnum}::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // --- user columns ---
                    // .col(ColumnDef::new({TableEnum}::Name).text().not_null())
                    // .col(ColumnDef::new({TableEnum}::Description).text())  // nullable = no .not_null()
                    // .col(ColumnDef::new({TableEnum}::VehicleId).integer().not_null())
                    // --- timestamps ---
                    .col(
                        ColumnDef::new({TableEnum}::CreatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("datetime('now')")),
                    )
                    .col(
                        ColumnDef::new({TableEnum}::UpdatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("datetime('now')")),
                    )
                    // --- foreign keys ---
                    // .foreign_key(
                    //     ForeignKey::create()
                    //         .from({TableEnum}::Table, {TableEnum}::VehicleId)
                    //         .to(Vehicles::Table, Vehicles::Id)
                    //         .on_delete(ForeignKeyAction::Cascade),
                    // )
                    .to_owned(),
            )
            .await?;

        // Index every FK column.
        // manager
        //     .create_index(
        //         Index::create()
        //             .if_not_exists()
        //             .name("idx_{table_name}_{fk_column}")
        //             .table({TableEnum}::Table)
        //             .col({TableEnum}::FkColumn)
        //             .to_owned(),
        //     )
        //     .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table({TableEnum}::Table).if_exists().to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum {TableEnum} {
    Table,
    Id,
    // ... every column as a PascalCase variant
    CreatedAt,
    UpdatedAt,
}

// FK targets need only Table + Id.
// #[derive(DeriveIden)]
// enum Vehicles {
//     Table,
//     Id,
// }
```

### Template: Junction Table (Many-to-Many)

```rust
// No Id column, no timestamps
manager
    .create_table(
        Table::create()
            .table({JunctionEnum}::Table)
            .if_not_exists()
            .col(ColumnDef::new({JunctionEnum}::LeftId).integer().not_null())
            .col(ColumnDef::new({JunctionEnum}::RightId).integer().not_null())
            .primary_key(
                Index::create()
                    .col({JunctionEnum}::LeftId)
                    .col({JunctionEnum}::RightId),
            )
            .foreign_key(
                ForeignKey::create()
                    .from({JunctionEnum}::Table, {JunctionEnum}::LeftId)
                    .to(LeftTable::Table, LeftTable::Id)
                    .on_delete(ForeignKeyAction::Cascade),
            )
            .foreign_key(
                ForeignKey::create()
                    .from({JunctionEnum}::Table, {JunctionEnum}::RightId)
                    .to(RightTable::Table, RightTable::Id)
                    .on_delete(ForeignKeyAction::Cascade),
            )
            .to_owned(),
    )
    .await?;
```

### Template: ALTER TABLE (Add Column)

```rust
// SQLite ALTER TABLE only supports ADD COLUMN
manager
    .alter_table(
        Table::alter()
            .table({TableEnum}::Table)
            .add_column(ColumnDef::new({TableEnum}::NewCol).text())
            .to_owned(),
    )
    .await?;
```

**IMPORTANT**: ALTER TABLE appends columns to the end. Update the entity struct to place new fields
AFTER `created_at`/`updated_at`. A column named in an FK clause cannot be dropped; rebuild the table
instead.

## Critical SQLite Rules

1. **Rerun-safe**: SQLite migrations are not transactional, so a crash mid-`up()` reruns from the
   top. Use `if_not_exists()` on tables and indexes, `INSERT ... WHERE NOT EXISTS` for data, and
   pragma-guarded `ALTER`s.
2. **DateTime defaults**: use `Expr::cust("datetime('now')")`, never a string literal such as
   `"CURRENT_TIMESTAMP"` (SeaORM quotes strings).
3. **FK on-delete policy**:
   - Non-nullable parent ref (`vehicle_id`) uses `ForeignKeyAction::Cascade`.
   - Nullable ref (`shop_id`, `model_template_id`) uses `ForeignKeyAction::SetNull`.
   - Self-referential nullable refs use `ForeignKeyAction::SetNull`.
   Foreign keys are enforced: `db::connect` turns them on.
4. **Always index FK columns** with a named index: `idx_{table}_{column}`.
5. **Column types**: `.text()` for strings (SQLite TEXT), `.integer()` for numbers.
6. **No builder form**: CHECK constraints, partial-index predicates the builder cannot express, and
   FTS5 tables with their triggers go through `manager.get_connection().execute_unprepared()`.
   Copy the external-content trigger trio from the initial schema migration for FTS5.
7. **Seeding data** uses `execute_unprepared` with `NOT EXISTS`-gated inserts, as the seed migration
   does.
8. **Lints**: add `#![allow(clippy::too_many_lines)]` when a migration defines many tables.

## Step 3: Register Migration

In `glovebox-shared/src/migration/mod.rs`:
1. Add `mod m{YYYYMMDD}_{NNNNNN}_{name};`.
2. Add `Box::new(m{YYYYMMDD}_{NNNNNN}_{name}::Migration),` to the end of the `migrations()` vec.

## Step 4: Verify

Run `cargo test -p glovebox-shared`: `test_support::test_db()` applies every migration to an
in-memory database. For a migration that moves data, also start the binary against a copy of
`data/glovebox.db`.
