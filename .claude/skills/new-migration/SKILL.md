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

Check `src/migration/mod.rs` for the current highest number. The next migration is `N+1`.

File naming: `m20260301_{NNNNNN}_{descriptive_name}.rs`
- Use the same date prefix `20260301` as existing consolidated migrations
- Pad the sequence number to 6 digits (e.g., `000009`)

## Step 2: Create Migration File

### Template — New Table

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
                    // --- timestamps (always last before FKs) ---
                    .col(
                        ColumnDef::new({TableEnum}::CreatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .col(
                        ColumnDef::new({TableEnum}::UpdatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
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

        // Indexes for FK columns (always index FK columns)
        // manager
        //     .create_index(
        //         Index::create()
        //             .table({TableEnum}::Table)
        //             .name("idx_{table_name}_{fk_column}")
        //             .col({TableEnum}::FkColumn)
        //             .to_owned(),
        //     )
        //     .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table({TableEnum}::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum {TableEnum} {
    Table,
    Id,
    // ... all columns as PascalCase variants
    CreatedAt,
    UpdatedAt,
}

// Reference enums for FK targets (only need Table + Id)
// #[derive(DeriveIden)]
// enum Vehicles {
//     Table,
//     Id,
// }
```

### Template — Junction Table (Many-to-Many)

```rust
// No Id column, no timestamps
manager
    .create_table(
        Table::create()
            .table({JunctionEnum}::Table)
            .if_not_exists()
            .col(
                ColumnDef::new({JunctionEnum}::LeftId)
                    .integer()
                    .not_null(),
            )
            .col(
                ColumnDef::new({JunctionEnum}::RightId)
                    .integer()
                    .not_null(),
            )
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

### Template — ALTER TABLE (Add Column)

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

**IMPORTANT**: ALTER TABLE appends columns to the end. Update the entity struct to place new fields AFTER `created_at`/`updated_at`.

## Critical SQLite Rules

1. **DateTime defaults**: ALWAYS use `Expr::cust("(datetime('now'))")` — NEVER string literals like `"CURRENT_TIMESTAMP"` (SeaORM wraps strings in quotes)
2. **FK on-delete policy**:
   - Non-nullable parent ref (vehicle_id) → `ForeignKeyAction::Cascade`
   - Nullable ref (shop_id, created_by) → `ForeignKeyAction::SetNull`
   - Self-referential nullable (superseded_by) → `ForeignKeyAction::SetNull`
3. **Always index FK columns** with a named index: `idx_{table}_{column}`
4. **Column types**: Use `.text()` for strings (SQLite TEXT), `.integer()` for numbers
5. **CHECK constraints**: Use raw SQL via `manager.get_connection().execute_unprepared()` — SeaORM migration builder doesn't support CHECK natively
6. **String defaults**: `.default("value")` works for simple string defaults (SeaORM quotes correctly)
7. **Add `#[allow(clippy::too_many_lines)]`** on `impl MigrationTrait` if migration creates multiple tables
8. **Add `#[allow(clippy::unreadable_literal)]`** if migration name has long numeric sequences

## Step 3: Register Migration

Add to `src/migration/mod.rs`:
1. Add `mod m20260301_{NNNNNN}_{name};` declaration
2. Add `Box::new(m20260301_{NNNNNN}_{name}::Migration),` to the `migrations()` vec

## Step 4: Verify

Run `cargo build` to check the migration compiles. The migration will auto-run on next `cargo run` via `Migrator::up()`.
