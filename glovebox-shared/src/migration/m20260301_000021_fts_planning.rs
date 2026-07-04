//! FTS5 arms for the planning tables (design pass round 4).
//!
//! Migration 000013 indexed the historical record (services, incidents,
//! documents, …) but not the forward-looking half: searching "air filter"
//! found the service record that replaced one, never the maintenance
//! schedule item that says when to do it again, nor the work item planning
//! it. This migration extends the same external-content FTS5 + trigger-trio
//! pattern to `maintenance_schedule_items` and `work_items`.
//!
//! Virtual-table and trigger DDL has no query-builder representation, so this
//! migration uses `execute_unprepared` raw SQL (the `Expr::cust()` convention
//! applies to expressions inside builder queries, not standalone DDL).

use sea_orm_migration::prelude::*;

/// One FTS5 external-content index over `content` table's `cols`
/// (same shape as migration 000013's `FtsSpec`).
struct FtsSpec {
    /// Content (source) table name; its `id` column is the FTS rowid.
    content: &'static str,
    /// FTS5 virtual table name.
    fts: &'static str,
    /// Indexed text columns (must be columns of `content`).
    cols: &'static [&'static str],
}

const SPECS: &[FtsSpec] = &[
    FtsSpec {
        content: "maintenance_schedule_items",
        fts: "fts_maintenance_schedule_items",
        cols: &["name", "description", "notes"],
    },
    FtsSpec {
        content: "work_items",
        fts: "fts_work_items",
        cols: &["title", "notes"],
    },
];

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        for spec in SPECS {
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

            // IF NOT EXISTS throughout: sea-orm-migration does not wrap up() in a
            // transaction on SQLite, so a crash mid-migration must not wedge the
            // re-run (matches 000013).
            db.execute_unprepared(&format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS {fts} USING fts5({cols}, content='{content}', \
                 content_rowid='id')"
            ))
            .await?;

            db.execute_unprepared(&format!(
                "CREATE TRIGGER IF NOT EXISTS {content}_fts_ai AFTER INSERT ON {content} BEGIN \
                 INSERT INTO {fts}(rowid, {cols}) VALUES (new.id, {new_vals}); END"
            ))
            .await?;

            // External-content FTS5 deletes take the OLD column values alongside
            // the special 'delete' command.
            db.execute_unprepared(&format!(
                "CREATE TRIGGER IF NOT EXISTS {content}_fts_ad AFTER DELETE ON {content} BEGIN \
                 INSERT INTO {fts}({fts}, rowid, {cols}) VALUES ('delete', old.id, {old_vals}); \
                 END"
            ))
            .await?;

            db.execute_unprepared(&format!(
                "CREATE TRIGGER IF NOT EXISTS {content}_fts_au AFTER UPDATE ON {content} BEGIN \
                 INSERT INTO {fts}({fts}, rowid, {cols}) VALUES ('delete', old.id, {old_vals}); \
                 INSERT INTO {fts}(rowid, {cols}) VALUES (new.id, {new_vals}); END"
            ))
            .await?;

            // Backfill pre-existing rows: 'rebuild' repopulates from the content
            // table. Rerun-safe by construction (it always rebuilds from content).
            db.execute_unprepared(&format!("INSERT INTO {fts}({fts}) VALUES ('rebuild')"))
                .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        for spec in SPECS {
            let FtsSpec { content, fts, .. } = spec;
            for suffix in ["ai", "ad", "au"] {
                db.execute_unprepared(&format!("DROP TRIGGER IF EXISTS {content}_fts_{suffix}"))
                    .await?;
            }
            db.execute_unprepared(&format!("DROP TABLE IF EXISTS {fts}"))
                .await?;
        }

        Ok(())
    }
}
