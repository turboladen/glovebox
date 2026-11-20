//! Adds a full-text index over shops, so a search for a shop's name or
//! address finds the services it did. Rows stored before the migration are
//! indexed by it.

use sea_orm_migration::prelude::*;

use super::schema::{FtsSpec, create_fts, drop_fts};

#[derive(DeriveMigrationName)]
pub struct Migration;

const FTS_SPECS: &[FtsSpec] = &[FtsSpec {
    content: "shops",
    fts: "fts_shops",
    cols: &["name", "address", "notes"],
}];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_fts(manager, FTS_SPECS).await?;
        // `rebuild` fills the index from the rows already stored, and running
        // it again is harmless, so a crash in the middle of `up()` reruns
        // cleanly.
        manager
            .get_connection()
            .execute_unprepared("INSERT INTO fts_shops(fts_shops) VALUES ('rebuild')")
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_fts(manager, FTS_SPECS).await
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::ConnectionTrait;
    use sea_orm_migration::MigratorTrait;

    use crate::migration::{
        Migrator,
        schema::testing::{count, fts_integrity, migrated_to},
    };

    // Eighteen migrations run before this one.
    const BEFORE: u32 = 18;

    #[tokio::test]
    async fn the_index_covers_shops_stored_before_it() {
        let db = migrated_to(BEFORE).await;
        db.execute_unprepared(
            "INSERT INTO shops (id, name, address, created_at, updated_at) VALUES (1, 'Tint \
             Pros', '6129 N Blackstone Ave', '2026-11-21T00:00:00+00:00', \
             '2026-11-21T00:00:00+00:00')",
        )
        .await
        .unwrap();

        Migrator::up(&db, Some(1)).await.unwrap();
        let found = "SELECT count(*) FROM fts_shops WHERE fts_shops MATCH '\"Blackstone\"'";
        assert_eq!(count(&db, found).await, 1);
        fts_integrity(&db, &["fts_shops"]).await;

        Migrator::down(&db, Some(1)).await.unwrap();
        Migrator::up(&db, Some(1)).await.unwrap();
        assert_eq!(count(&db, found).await, 1);
        db.execute_unprepared("DELETE FROM shops WHERE id = 1")
            .await
            .unwrap();
        assert_eq!(count(&db, found).await, 0);
        fts_integrity(&db, &["fts_shops"]).await;
    }
}
