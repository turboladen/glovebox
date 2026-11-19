//! Adds full-text indexes over parts and the items inside kits, so a search
//! finds a part by its name or number and a kit by one of its items' numbers.
//!
//! Each index is an external-content FTS5 table kept in sync by triggers,
//! built by the same `schema::create_fts` as the initial schema's indexes.
//! `rebuild` fills each index from the rows already stored and is safe to run
//! again, so a crash in the middle of `up()` reruns cleanly.

use sea_orm_migration::prelude::*;

use super::schema::{FtsSpec, create_fts, drop_fts};

#[derive(DeriveMigrationName)]
pub struct Migration;

const FTS_SPECS: &[FtsSpec] = &[
    FtsSpec {
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
    FtsSpec {
        content: "part_contents",
        fts: "fts_part_contents",
        cols: &["name", "manufacturer", "part_number"],
    },
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_fts(manager, FTS_SPECS).await?;
        let db = manager.get_connection();
        for FtsSpec { fts, .. } in FTS_SPECS {
            db.execute_unprepared(&format!("INSERT INTO {fts}({fts}) VALUES ('rebuild')"))
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_fts(manager, FTS_SPECS).await
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection};
    use sea_orm_migration::prelude::*;

    use super::super::{
        Migrator,
        schema::testing::{count, migrated_to},
    };

    async fn integrity(db: &DatabaseConnection) {
        for fts in ["fts_parts", "fts_part_contents"] {
            db.execute_unprepared(&format!(
                "INSERT INTO {fts}({fts}) VALUES ('integrity-check')"
            ))
            .await
            .unwrap();
        }
    }

    // Rows stored before the indexes exist are found once the migration runs,
    // running it again leaves the indexes sound, and the triggers follow a
    // cascading delete.
    #[tokio::test]
    async fn the_indexes_cover_rows_stored_before_them_and_a_rerun_keeps_them_sound() {
        let db = migrated_to(10).await;
        db.execute_unprepared(
            "INSERT INTO vehicles (id, name, created_at, updated_at) VALUES (1, 'Daily', \
             '2026-11-18T00:00:00+00:00', '2026-11-18T00:00:00+00:00'); INSERT INTO parts (id, \
             vehicle_id, name, part_number, status, supplied_by, created_at, updated_at) VALUES \
             (1, 1, 'Oil change kit', '06K115562LM', 'purchased', 'self', \
             '2026-11-18T00:00:00+00:00', '2026-11-18T00:00:00+00:00'); INSERT INTO part_contents \
             (part_id, position, name, part_number, quantity, created_at, updated_at) VALUES (1, \
             0, 'Oil filter', 'HU 7020 z', 1, '2026-11-18T00:00:00+00:00', \
             '2026-11-18T00:00:00+00:00')",
        )
        .await
        .unwrap();

        Migrator::up(&db, None).await.unwrap();
        super::Migration.up(&SchemaManager::new(&db)).await.unwrap();

        let kit = "SELECT count(*) FROM fts_parts WHERE fts_parts MATCH '\"06K115562LM\"'";
        let item =
            "SELECT count(*) FROM fts_part_contents WHERE fts_part_contents MATCH '\"7020\"'";
        assert_eq!(count(&db, kit).await, 1);
        assert_eq!(count(&db, item).await, 1);
        integrity(&db).await;

        db.execute_unprepared("DELETE FROM vehicles WHERE id = 1")
            .await
            .unwrap();
        assert_eq!(count(&db, kit).await, 0);
        assert_eq!(count(&db, item).await, 0);
        integrity(&db).await;
        assert_eq!(
            Migrator::get_pending_migrations(&db).await.unwrap().len(),
            0
        );
    }
}
