//! FTS5 full-text search index (mq9r).
//!
//! One external-content FTS5 virtual table per content table, kept in sync by
//! AFTER INSERT/UPDATE/DELETE triggers, so the index stays consistent through
//! normal `SeaORM` writes with zero service-code changes. External-content tables
//! store only the index (no duplicated text); reads resolve against the content
//! table, and the shared `services::search` joins back to the content tables to
//! derive `vehicle_id` and titles at query time.
//!
//! Virtual-table and trigger DDL has no query-builder representation, so this
//! migration uses `execute_unprepared` raw SQL (the `Expr::cust()` convention
//! applies to expressions inside builder queries, not standalone DDL).

use sea_orm_migration::prelude::*;

/// One FTS5 external-content index over `content` table's `cols`.
///
/// Constraint of external-content FTS5: every declared column must exist on the
/// content table (FTS5 reads values back by rowid), so derived attributes like a
/// line item's `vehicle_id` cannot live here — `services::search` joins the
/// content tables instead.
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
        content: "vehicles",
        fts: "fts_vehicles",
        cols: &[
            "name",
            "make",
            "model",
            "trim_level",
            "vin",
            "license_plate",
            "notes",
        ],
    },
    FtsSpec {
        content: "service_records",
        fts: "fts_service_records",
        cols: &["description", "shop_name", "notes"],
    },
    FtsSpec {
        content: "service_record_line_items",
        fts: "fts_service_record_line_items",
        cols: &["description"],
    },
    FtsSpec {
        content: "observations",
        fts: "fts_observations",
        cols: &["title", "description", "obd_codes", "notes"],
    },
    FtsSpec {
        content: "accidents",
        fts: "fts_accidents",
        cols: &["description", "notes"],
    },
    FtsSpec {
        content: "accident_correspondence",
        fts: "fts_accident_correspondence",
        cols: &["summary", "notes"],
    },
    FtsSpec {
        content: "documents",
        fts: "fts_documents",
        cols: &["title", "file_name", "notes", "extracted_text"],
    },
    FtsSpec {
        content: "research_findings",
        fts: "fts_research_findings",
        cols: &["title", "description"],
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
            // transaction, so a crash mid-migration must not wedge the re-run
            // (matches the .if_not_exists() convention of the earlier migrations).
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

            // Index pre-existing rows: 'rebuild' repopulates from the content table.
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
