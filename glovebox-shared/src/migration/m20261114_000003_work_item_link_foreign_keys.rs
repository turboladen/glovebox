//! Gives `work_items.schedule_item_id`, `research_finding_id` and
//! `incident_id` an `ON DELETE SET NULL` foreign key and an index each.
//!
//! A job whose column names a row that does not exist has the column set to
//! NULL first and its `updated_at` stamped, and the count is logged as a
//! warning. The job itself is kept.
//!
//! The whole change runs in one transaction, and a failure leaves the
//! database as it was. Running it again finds the keys in place and edits
//! nothing.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

use super::schema::{scalar, soundness};

/// Each linking column of `work_items`, the table it points at and the name
/// of its index.
const LINKS: &[(&str, &str, &str)] = &[
    (
        "schedule_item_id",
        "maintenance_schedule_items",
        "idx_work_items_schedule_item",
    ),
    (
        "research_finding_id",
        "research_findings",
        "idx_work_items_research_finding",
    ),
    ("incident_id", "incidents", "idx_work_items_incident"),
];

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // One transaction pins one connection, which `PRAGMA writable_schema`
        // needs, and leaves nothing half done when a step fails.
        let txn = manager.get_connection().begin().await?;
        // A write first takes the write lock, waiting out another program's
        // write. A transaction that reads first and writes later fails at once
        // if anything was written in between.
        txn.execute_unprepared("UPDATE seaql_migrations SET version = version WHERE 0")
            .await?;

        // SQLite does not check the stored rows against a key added to the
        // table's definition, so the rows are made to satisfy it first.
        let cleared = clear_dangling(&txn).await?;
        add_foreign_keys(&txn).await?;
        for (column, _, index) in LINKS {
            txn.execute_unprepared(&format!(
                "CREATE INDEX IF NOT EXISTS {index} ON work_items ({column})"
            ))
            .await?;
        }
        txn.commit().await?;
        // A failure above rolls the clearing back, so it is reported only
        // once it is committed.
        for (column, cleared) in cleared {
            tracing::warn!(
                column,
                cleared,
                "cleared work_items ids that named a row that does not exist; the jobs are kept"
            );
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Nothing is undone: no code depends on the keys being absent.
        Ok(())
    }
}

/// Sets to NULL every linking column of `work_items` that names a row that
/// does not exist, stamps `updated_at` on each job it changes, and returns
/// each column that held one with the number of jobs cleared.
async fn clear_dangling(db: &impl ConnectionTrait) -> Result<Vec<(&'static str, u64)>, DbErr> {
    let mut cleared = Vec::new();
    for (column, parent, _) in LINKS {
        let rows = db
            .execute_unprepared(&format!(
                "UPDATE work_items SET {column} = NULL, updated_at = \
                 strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now') WHERE {column} IS NOT NULL AND \
                 {column} NOT IN (SELECT id FROM {parent})"
            ))
            .await?
            .rows_affected();
        if rows > 0 {
            cleared.push((*column, rows));
        }
    }
    Ok(cleared)
}

/// Adds the foreign key of every link that lacks one by editing the stored
/// definition of `work_items`. No row moves. It returns an error when a key
/// is still absent afterwards or the database's soundness checks changed.
async fn add_foreign_keys(txn: &impl ConnectionTrait) -> Result<(), DbErr> {
    // A foreign key lives only in the table's stored `CREATE TABLE` text, so
    // the edit leaves the table's triggers, indexes and search index alone.
    // `docs/adr/0003-foreign-keys-by-editing-the-table-definition.md` records
    // why the table is not rebuilt.
    let missing = links_without_key(txn).await?;
    if missing.is_empty() {
        return Ok(());
    }
    let version = scalar::<i64>(txn, "PRAGMA schema_version").await?;
    let before = soundness(txn).await?;

    txn.execute_unprepared("PRAGMA writable_schema = ON")
        .await?;
    let mut edited = Ok(());
    for (column, parent, _) in missing {
        // The comma keeps the edit to a column that carries no clause yet.
        let edit = txn
            .execute_unprepared(&format!(
                "UPDATE sqlite_schema SET sql = replace(sql, '\"{column}\" integer,', \
                 '\"{column}\" integer REFERENCES \"{parent}\" (\"id\") ON DELETE SET NULL,') \
                 WHERE type = 'table' AND name = 'work_items'"
            ))
            .await;
        if let Err(e) = edit {
            edited = Err(e);
            break;
        }
    }
    // The pragma belongs to the connection, which goes back to the pool, so it
    // is cleared whether or not the edit worked.
    txn.execute_unprepared("PRAGMA writable_schema = OFF")
        .await?;
    edited?;
    // Every connection reloads the schema when it sees a new version.
    txn.execute_unprepared(&format!("PRAGMA schema_version = {}", version + 1))
        .await?;

    // The edit is judged by what SQLite reads back, because a definition
    // spelled another way would leave the replacement with nothing to match.
    let absent: Vec<_> = links_without_key(txn)
        .await?
        .iter()
        .map(|(column, ..)| *column)
        .collect();
    // Both checks cover the whole database, so a fault that was already there
    // is not this edit's doing and does not stop it.
    let after = soundness(txn).await?;
    if !absent.is_empty() || after != before {
        return Err(DbErr::Migration(format!(
            "adding the foreign keys of work_items did not leave the database as expected (no key \
             was added for: [{}]; integrity check and broken foreign keys were {before:?} and are \
             {after:?}). Nothing was changed.",
            absent.join(", ")
        )));
    }
    Ok(())
}

/// A foreign key of `work_items` as (column, parent table, delete action).
type Key = (String, String, String);

fn set_null_key(column: &str, parent: &str) -> Key {
    (column.into(), parent.into(), "SET NULL".into())
}

/// The entries of `LINKS` whose `ON DELETE SET NULL` key `work_items` does
/// not have.
async fn links_without_key(
    db: &impl ConnectionTrait,
) -> Result<Vec<&'static (&'static str, &'static str, &'static str)>, DbErr> {
    let present = foreign_keys(db).await?;
    Ok(LINKS
        .iter()
        .filter(|(column, parent, _)| !present.contains(&set_null_key(column, parent)))
        .collect())
}

async fn foreign_keys(db: &impl ConnectionTrait) -> Result<Vec<Key>, DbErr> {
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT \"from\", \"table\", on_delete FROM pragma_foreign_key_list('work_items')",
    ))
    .await?
    .iter()
    .map(|row| {
        Ok((
            row.try_get_by_index(0)?,
            row.try_get_by_index(1)?,
            row.try_get_by_index(2)?,
        ))
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection};
    use sea_orm_migration::MigratorTrait;

    use super::{clear_dangling, foreign_keys};
    use crate::migration::{
        Migrator,
        schema::testing::{count, migrated_to, text},
    };

    // The number of migrations that run before this one.
    const BEFORE: u32 = 6;

    // A database as the previous migration leaves it. Job 1 comes from
    // schedule item 9001 and has a need, job 2 from research finding 9002 and
    // job 3 from incident 9003.
    async fn db_before() -> DatabaseConnection {
        let db = migrated_to(BEFORE).await;
        let sql = "
            INSERT INTO vehicles (id, name, created_at, updated_at) VALUES (1, 'Golf', S, S);
            INSERT INTO maintenance_schedule_items (id, vehicle_id, name, created_at, updated_at)
                VALUES (9001, 1, 'Oil change', S, S);
            INSERT INTO research_reports (id, vehicle_id, generated_at, created_at)
                VALUES (1, 1, S, S);
            INSERT INTO research_findings (id, report_id, category, title, created_at, updated_at)
                VALUES (9002, 1, 'recall', 'Fuel pump', S, S);
            INSERT INTO incidents
                (id, vehicle_id, category, title, occurred_at, created_at, updated_at)
                VALUES (9003, 1, 'noise', 'Rattle', '2026-11-01', S, S);
            INSERT INTO work_items
                (id, vehicle_id, title, schedule_item_id, research_finding_id, incident_id,
                 created_at, updated_at)
                VALUES (1, 1, 'Change the oil', 9001, NULL, NULL, S, S),
                       (2, 1, 'Replace the fuel pump', NULL, 9002, NULL, S, S),
                       (3, 1, 'Chase the rattle', NULL, NULL, 9003, S, S);
            INSERT INTO job_needs (id, work_item_id, name, created_at, updated_at)
                VALUES (1, 1, 'Oil filter', S, S);";
        db.execute_unprepared(&sql.replace(" S", " '2026-11-14T12:00:00+00:00'"))
            .await
            .unwrap();
        db
    }

    // Adds job 4, whose three ids each name a row that does not exist.
    async fn add_dangling_job(db: &DatabaseConnection) {
        db.execute_unprepared(
            "INSERT INTO work_items
                (id, vehicle_id, title, schedule_item_id, research_finding_id, incident_id,
                 created_at, updated_at)
                VALUES (4, 1, 'Orphaned job', 777, 888, 999, '2026-11-14T12:00:00+00:00',
                        '2026-11-14T12:00:00+00:00')",
        )
        .await
        .unwrap();
    }

    // The three linking columns of every job, in id order.
    async fn links(db: &DatabaseConnection) -> String {
        text(
            db,
            "SELECT group_concat(id || ':' || coalesce(schedule_item_id, '-') || ',' || \
             coalesce(research_finding_id, '-') || ',' || coalesce(incident_id, '-'), ' ') FROM \
             (SELECT * FROM work_items ORDER BY id)",
        )
        .await
    }

    async fn definition(db: &DatabaseConnection) -> String {
        text(
            db,
            "SELECT sql FROM sqlite_schema WHERE name = 'work_items'",
        )
        .await
    }

    // Everything stored about the table except the definition this migration
    // edits and the indexes it adds: column order, triggers, earlier indexes.
    async fn surroundings(db: &DatabaseConnection) -> String {
        text(
            db,
            "SELECT (SELECT group_concat(name, ',') FROM pragma_table_info('work_items')) || \
             (SELECT group_concat(sql, ';') FROM (SELECT sql FROM sqlite_schema WHERE tbl_name = \
             'work_items' AND type IN ('trigger', 'index') AND name NOT IN \
             ('idx_work_items_schedule_item', 'idx_work_items_research_finding', \
             'idx_work_items_incident') ORDER BY name))",
        )
        .await
    }

    #[tokio::test]
    async fn deleting_a_schedule_item_clears_the_job_and_keeps_it() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();

        db.execute_unprepared("DELETE FROM maintenance_schedule_items WHERE id = 9001")
            .await
            .unwrap();

        assert_eq!(links(&db).await, "1:-,-,- 2:-,9002,- 3:-,-,9003");
    }

    #[tokio::test]
    async fn deleting_a_research_finding_clears_the_job_and_keeps_it() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();

        db.execute_unprepared("DELETE FROM research_findings WHERE id = 9002")
            .await
            .unwrap();

        assert_eq!(links(&db).await, "1:9001,-,- 2:-,-,- 3:-,-,9003");
    }

    #[tokio::test]
    async fn deleting_an_incident_clears_the_job_and_keeps_it() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();

        db.execute_unprepared("DELETE FROM incidents WHERE id = 9003")
            .await
            .unwrap();

        assert_eq!(links(&db).await, "1:9001,-,- 2:-,9002,- 3:-,-,-");
    }

    #[tokio::test]
    async fn a_job_cannot_name_a_row_that_does_not_exist() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();

        for column in ["schedule_item_id", "research_finding_id", "incident_id"] {
            let refused = db
                .execute_unprepared(&format!(
                    "UPDATE work_items SET {column} = 12345 WHERE id = 1"
                ))
                .await
                .unwrap_err()
                .to_string();
            assert!(refused.contains("FOREIGN KEY"), "{column}: {refused}");
        }
    }

    #[tokio::test]
    async fn a_dangling_id_is_cleared_and_the_job_is_kept() {
        let db = db_before().await;
        add_dangling_job(&db).await;

        Migrator::up(&db, Some(1)).await.unwrap();

        assert_eq!(links(&db).await, "1:9001,-,- 2:-,9002,- 3:-,-,9003 4:-,-,-");
        assert_eq!(
            text(&db, "SELECT title FROM work_items WHERE id = 4").await,
            "Orphaned job"
        );
        // Only the job that changed is stamped, in the form every stamp has.
        let seeded = "2026-11-14T12:00:00+00:00";
        assert_eq!(
            count(
                &db,
                &format!(
                    "SELECT count(*) FROM work_items WHERE id = 4 AND updated_at > '{seeded}' AND \
                     updated_at GLOB '????-??-??T??:??:??+00:00' AND created_at = '{seeded}'"
                )
            )
            .await,
            1
        );
        assert_eq!(
            count(
                &db,
                &format!("SELECT count(*) FROM work_items WHERE updated_at = '{seeded}'")
            )
            .await,
            3
        );
        assert_eq!(
            count(&db, "SELECT count(*) FROM pragma_foreign_key_check").await,
            0
        );
    }

    #[tokio::test]
    async fn the_cleared_ids_are_counted_per_column() {
        let db = db_before().await;
        assert_eq!(clear_dangling(&db).await.unwrap(), vec![]);
        add_dangling_job(&db).await;
        db.execute_unprepared("UPDATE work_items SET incident_id = 998 WHERE id = 1")
            .await
            .unwrap();

        let cleared = clear_dangling(&db).await.unwrap();

        assert_eq!(
            cleared,
            vec![
                ("schedule_item_id", 1),
                ("research_finding_id", 1),
                ("incident_id", 2)
            ]
        );
    }

    #[tokio::test]
    async fn the_table_keeps_its_rows_columns_triggers_indexes_and_inbound_key() {
        let db = db_before().await;
        let rows = text(
            &db,
            "SELECT group_concat(id || title || status || created_at, '|') FROM work_items",
        )
        .await;
        let around = surroundings(&db).await;
        assert!(around.contains("work_items_fts_au") && around.contains("appointment_id"));

        Migrator::up(&db, Some(1)).await.unwrap();

        assert_eq!(
            text(
                &db,
                "SELECT group_concat(id || title || status || created_at, '|') FROM work_items"
            )
            .await,
            rows
        );
        assert_eq!(surroundings(&db).await, around);
        assert_eq!(foreign_keys(&db).await.unwrap().len(), 6);
        assert_eq!(
            count(
                &db,
                "SELECT count(*) FROM sqlite_schema WHERE name IN \
                 ('idx_work_items_schedule_item', 'idx_work_items_research_finding', \
                 'idx_work_items_incident')"
            )
            .await,
            3
        );
        assert_eq!(text(&db, "PRAGMA integrity_check").await, "ok");
        // The search index finds a job by its title and follows a new title.
        let found = "SELECT count(*) FROM fts_work_items WHERE fts_work_items MATCH";
        assert_eq!(count(&db, &format!("{found} 'rattle'")).await, 1);
        db.execute_unprepared("UPDATE work_items SET title = 'Chase the squeak' WHERE id = 3")
            .await
            .unwrap();
        assert_eq!(count(&db, &format!("{found} 'squeak'")).await, 1);
        assert_eq!(count(&db, &format!("{found} 'rattle'")).await, 0);
        // A need still goes with its job.
        db.execute_unprepared("DELETE FROM work_items WHERE id = 1")
            .await
            .unwrap();
        assert_eq!(count(&db, "SELECT count(*) FROM job_needs").await, 0);
    }

    #[tokio::test]
    async fn running_it_again_changes_nothing() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();
        let once = definition(&db).await;
        assert_eq!(once.matches("ON DELETE SET NULL").count(), 5, "{once}");

        Migrator::down(&db, Some(1)).await.unwrap();
        Migrator::up(&db, Some(1)).await.unwrap();

        assert_eq!(definition(&db).await, once);
        assert_eq!(links(&db).await, "1:9001,-,- 2:-,9002,- 3:-,-,9003");
        assert_eq!(text(&db, "PRAGMA integrity_check").await, "ok");
    }

    // A database where one key is already present gets the other two.
    #[tokio::test]
    async fn a_table_that_already_has_one_key_gets_the_others() {
        let db = db_before().await;
        db.execute_unprepared(
            "PRAGMA writable_schema = ON;
             UPDATE sqlite_schema SET sql = replace(sql, '\"incident_id\" integer,', \
             '\"incident_id\" integer REFERENCES \"incidents\" (\"id\") ON DELETE SET NULL,') \
             WHERE name = 'work_items';
             PRAGMA writable_schema = OFF;",
        )
        .await
        .unwrap();

        Migrator::up(&db, Some(1)).await.unwrap();

        let stored = definition(&db).await;
        assert_eq!(stored.matches("ON DELETE SET NULL").count(), 5, "{stored}");
        assert_eq!(foreign_keys(&db).await.unwrap().len(), 6);
    }

    // A vehicle's jobs and the rows they name are all deleted with it, and
    // the search index is left holding nothing.
    #[tokio::test]
    async fn deleting_the_vehicle_removes_its_jobs_and_their_search_rows() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();

        db.execute_unprepared("DELETE FROM vehicles WHERE id = 1")
            .await
            .unwrap();

        assert_eq!(count(&db, "SELECT count(*) FROM work_items").await, 0);
        assert_eq!(
            count(
                &db,
                "SELECT count(*) FROM fts_work_items WHERE fts_work_items MATCH 'rattle'"
            )
            .await,
            0
        );
        db.execute_unprepared(
            "INSERT INTO fts_work_items(fts_work_items) VALUES ('integrity-check')",
        )
        .await
        .unwrap();
        assert_eq!(text(&db, "PRAGMA integrity_check").await, "ok");
    }

    // The replacement matches one spelling of a column. A definition spelled
    // another way must stop the migration, and the cleared ids must come back.
    #[tokio::test]
    async fn a_definition_spelled_another_way_stops_it_before_anything_changes() {
        let db = db_before().await;
        add_dangling_job(&db).await;
        db.execute_unprepared(
            "PRAGMA writable_schema = ON;
             UPDATE sqlite_schema SET sql = replace(sql, '\"incident_id\" integer,', \
             '\"incident_id\" INTEGER,') WHERE name = 'work_items';
             PRAGMA writable_schema = OFF;",
        )
        .await
        .unwrap();
        let before = definition(&db).await;
        assert!(before.contains("\"incident_id\" INTEGER,"));

        let err = Migrator::up(&db, Some(1)).await.unwrap_err().to_string();

        assert!(err.contains("no key was added for: [incident_id]"), "{err}");
        assert_eq!(definition(&db).await, before);
        assert_eq!(
            links(&db).await,
            "1:9001,-,- 2:-,9002,- 3:-,-,9003 4:777,888,999"
        );
        assert_eq!(foreign_keys(&db).await.unwrap().len(), 3);
        // The table still takes a write.
        db.execute_unprepared("UPDATE work_items SET title = 'Still here' WHERE id = 4")
            .await
            .unwrap();
    }
}
