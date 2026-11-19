//! Adds full-text indexes over parts and the items inside kits, so a search
//! finds a part by its name or number and a kit by one of its items' numbers.
//!
//! Each index is an external-content FTS5 table kept in sync by AFTER
//! INSERT/UPDATE/DELETE triggers, the same form the initial schema's indexes
//! take. The statements are written out here, not shared with that
//! migration, because a migration's text must not change once it has run.
//! `rebuild` fills each index from the rows already stored and is safe to run
//! again, so a crash in the middle of `up()` reruns cleanly.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// One external-content FTS5 index over `content`'s `cols`.
struct Index {
    content: &'static str,
    fts: &'static str,
    cols: &'static [&'static str],
}

const INDEXES: &[Index] = &[
    Index {
        content: "parts",
        fts: "fts_parts",
        cols: &[
            "name",
            "manufacturer",
            "part_number",
            "oe_part_number_replaced",
            "seller",
            "location",
            "notes",
            "order_ref",
        ],
    },
    Index {
        content: "part_contents",
        fts: "fts_part_contents",
        cols: &["name", "manufacturer", "part_number"],
    },
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for Index { content, fts, cols } in INDEXES {
            let list = cols.join(", ");
            let values = |prefix: &str| {
                cols.iter()
                    .map(|c| format!("{prefix}.{c}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let (new_vals, old_vals) = (values("new"), values("old"));
            db.execute_unprepared(&format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS {fts} USING fts5({list}, content='{content}', \
                 content_rowid='id')"
            ))
            .await?;
            db.execute_unprepared(&format!(
                "CREATE TRIGGER IF NOT EXISTS {content}_fts_ai AFTER INSERT ON {content} BEGIN \
                 INSERT INTO {fts}(rowid, {list}) VALUES (new.id, {new_vals}); END"
            ))
            .await?;
            // An external-content delete takes the old values with the special
            // 'delete' command.
            db.execute_unprepared(&format!(
                "CREATE TRIGGER IF NOT EXISTS {content}_fts_ad AFTER DELETE ON {content} BEGIN \
                 INSERT INTO {fts}({fts}, rowid, {list}) VALUES ('delete', old.id, {old_vals}); \
                 END"
            ))
            .await?;
            db.execute_unprepared(&format!(
                "CREATE TRIGGER IF NOT EXISTS {content}_fts_au AFTER UPDATE ON {content} BEGIN \
                 INSERT INTO {fts}({fts}, rowid, {list}) VALUES ('delete', old.id, {old_vals}); \
                 INSERT INTO {fts}(rowid, {list}) VALUES (new.id, {new_vals}); END"
            ))
            .await?;
            db.execute_unprepared(&format!("INSERT INTO {fts}({fts}) VALUES ('rebuild')"))
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for Index { content, fts, .. } in INDEXES {
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

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::{migration::Migrator, test_support::test_db};

    async fn count(db: &impl ConnectionTrait, sql: &str) -> i64 {
        db.query_one_raw(Statement::from_string(DatabaseBackend::Sqlite, sql))
            .await
            .unwrap()
            .unwrap()
            .try_get_by_index(0)
            .unwrap()
    }

    async fn exec(db: &impl ConnectionTrait, sql: &str) {
        db.execute_unprepared(sql).await.unwrap();
    }

    // Rows stored before the indexes exist are found once the migration runs,
    // and running it again leaves the indexes sound.
    #[tokio::test]
    async fn the_indexes_cover_rows_stored_before_them_and_a_rerun_keeps_them_sound() {
        let db = test_db().await;
        let migration = super::Migration;
        let manager = sea_orm_migration::SchemaManager::new(&db);
        sea_orm_migration::MigrationTrait::down(&migration, &manager)
            .await
            .unwrap();
        exec(
            &db,
            "INSERT INTO vehicles (id, name, created_at, updated_at) VALUES (1, 'Daily', \
             '2026-11-18T00:00:00+00:00', '2026-11-18T00:00:00+00:00')",
        )
        .await;
        exec(
            &db,
            "INSERT INTO parts (id, vehicle_id, name, part_number, status, supplied_by, \
             created_at, updated_at) VALUES (1, 1, 'Oil change kit', '06K115562LM', 'purchased', \
             'self', '2026-11-18T00:00:00+00:00', '2026-11-18T00:00:00+00:00')",
        )
        .await;
        exec(
            &db,
            "INSERT INTO part_contents (part_id, position, name, part_number, quantity, \
             created_at, updated_at) VALUES (1, 0, 'Oil filter', 'HU 7020 z', 1, \
             '2026-11-18T00:00:00+00:00', '2026-11-18T00:00:00+00:00')",
        )
        .await;

        for _ in 0..2 {
            sea_orm_migration::MigrationTrait::up(&migration, &manager)
                .await
                .unwrap();
        }

        let kit = "SELECT count(*) FROM fts_parts WHERE fts_parts MATCH '\"06K115562LM\"'";
        let item =
            "SELECT count(*) FROM fts_part_contents WHERE fts_part_contents MATCH '\"7020\"'";
        assert_eq!(count(&db, kit).await, 1);
        assert_eq!(count(&db, item).await, 1);
        for fts in ["fts_parts", "fts_part_contents"] {
            exec(
                &db,
                &format!("INSERT INTO {fts}({fts}) VALUES ('integrity-check')"),
            )
            .await;
        }

        // Deleting the vehicle cascades through the part to its items, and
        // the triggers take both out of their indexes.
        exec(&db, "DELETE FROM vehicles WHERE id = 1").await;
        assert_eq!(count(&db, kit).await, 0);
        assert_eq!(count(&db, item).await, 0);
        for fts in ["fts_parts", "fts_part_contents"] {
            exec(
                &db,
                &format!("INSERT INTO {fts}({fts}) VALUES ('integrity-check')"),
            )
            .await;
        }
        assert!(
            Migrator::get_pending_migrations(&db)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
