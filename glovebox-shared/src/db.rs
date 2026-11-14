//! The single place a `SQLite` connection is opened.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// The handle [`connect`] returns. Other crates hold it and pass it back to
/// the shared services; they never run a query of their own.
pub use sea_orm::DatabaseConnection;
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, Statement,
    sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode},
};
use sea_orm_migration::MigratorTrait;

use crate::{
    backup,
    error::{DomainError, DomainResult},
    migration::Migrator,
};

/// Builds the connection options shared by production and tests: WAL journal
/// mode and enforced foreign keys. `file` of `None` selects a private in-memory
/// database, which is pinned to one connection because each in-memory
/// connection is its own database.
pub(crate) fn connect_options(file: Option<PathBuf>) -> ConnectOptions {
    // The URL only seeds the options. Setting the filename directly avoids
    // percent-encoding arbitrary paths into a URL.
    let mut options = ConnectOptions::new("sqlite::memory:");
    let in_memory = file.is_none();
    options.map_sqlx_sqlite_opts(move |opts: SqliteConnectOptions| {
        let opts = match &file {
            Some(path) => opts
                .filename(path)
                .in_memory(false)
                .shared_cache(false)
                .create_if_missing(true),
            None => opts,
        };
        // Two processes (the MCP server and the web server) share one file, so a
        // writer waits for the lock instead of failing at once with "locked".
        opts.journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5))
    });
    if in_memory {
        options.max_connections(1).min_connections(1);
    }
    options.sqlx_logging(false);
    options
}

/// Opens (creating it and its parent directory if needed) the database at
/// `db_path` in WAL mode with foreign keys enforced, then applies all
/// migrations.
pub async fn connect(db_path: &str) -> DomainResult<DatabaseConnection> {
    let path = Path::new(db_path);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            DomainError::Internal(format!(
                "cannot create database directory {}: {e}",
                parent.display()
            ))
        })?;
    }
    let had_data = backup::has_data(path);
    let db = Database::connect(connect_options(Some(path.to_path_buf()))).await?;
    if had_data {
        refuse_unknown_migrations(&db).await?;
        backup_before_migrating(&db, path, chrono::Local::now()).await?;
    }
    Migrator::up(&db, None).await?;
    Ok(db)
}

/// Fails when the database records a migration this binary does not contain,
/// which means a newer build has already changed the file. Opening it with an
/// older binary could corrupt data, so the caller must rebuild first.
async fn refuse_unknown_migrations(db: &DatabaseConnection) -> DomainResult<()> {
    let known: Vec<String> = Migrator::migrations()
        .iter()
        .map(|m| m.name().to_owned())
        .collect();
    // The table is read directly because the migrator's own listing fails on
    // an unknown version with a message that does not say what to do.
    let table = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'seaql_migrations'",
        ))
        .await?;
    if table.is_none() {
        return Ok(());
    }
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT version FROM seaql_migrations",
        ))
        .await?;
    for row in rows {
        let version: String = row.try_get_by_index(0)?;
        if !known.contains(&version) {
            return Err(DomainError::Internal(format!(
                "the database has migration {version} that this binary does not know; the binary \
                 is older than the database, so rebuild it (`just install`) and restart"
            )));
        }
    }
    Ok(())
}

/// Backs up an existing database file: always when a migration is about to
/// change it, and otherwise once per calendar day when it holds a vehicle.
///
/// # Errors
///
/// A failed backup stops the open only when a migration is pending, because
/// that is the case where the file is about to be changed in place.
async fn backup_before_migrating(
    db: &DatabaseConnection,
    path: &Path,
    now: chrono::DateTime<chrono::Local>,
) -> DomainResult<()> {
    let pending = !Migrator::get_pending_migrations(db).await?.is_empty();
    if !pending {
        backup::daily(db, path, now).await;
        return Ok(());
    }
    match backup::write(db, path, now).await {
        Ok(copy) => {
            tracing::info!(path = %copy.display(), "backed up the database");
            backup::prune(path);
            Ok(())
        }
        Err(e) => Err(DomainError::Internal(format!(
            "refusing to migrate {} because it could not be backed up first: {e}",
            path.display()
        ))),
    }
}

/// Backs up the database at `db_path` now and returns the copy's path, or
/// `None` when the file is missing or empty. It never creates the file.
pub async fn backup_now(db_path: &str) -> DomainResult<Option<PathBuf>> {
    let path = Path::new(db_path);
    if !backup::has_data(path) {
        return Ok(None);
    }
    let db = Database::connect(connect_options(Some(path.to_path_buf()))).await?;
    let copy = backup::write(&db, path, chrono::Local::now()).await?;
    backup::prune(path);
    Ok(Some(copy))
}

/// Opens a fresh in-memory database with the production options and applies
/// all migrations.
#[cfg(any(test, feature = "test-support"))]
pub(crate) async fn connect_in_memory() -> DatabaseConnection {
    let db = Database::connect(connect_options(None))
        .await
        .expect("connect in-memory sqlite");
    Migrator::up(&db, None).await.expect("run migrations");
    db
}

#[cfg(test)]
mod tests {
    use sea_orm::{
        ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait, QueryFilter, Statement,
        TransactionTrait,
    };

    use super::*;
    use crate::{
        inputs::service_record::NewServiceRecord,
        services::{schedule, service_record},
        test_support::VehicleFixture,
    };

    async fn scalar(db: &DatabaseConnection, sql: &str) -> i32 {
        let row = db
            .query_one_raw(Statement::from_string(DatabaseBackend::Sqlite, sql))
            .await
            .unwrap()
            .unwrap();
        row.try_get_by_index(0).unwrap()
    }

    async fn table_names(db: &DatabaseConnection) -> Vec<String> {
        db.query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type = 'table'",
        ))
        .await
        .unwrap()
        .iter()
        .map(|r| r.try_get_by_index::<String>(0).unwrap())
        .collect()
    }

    #[tokio::test]
    async fn connect_creates_the_file_enables_foreign_keys_and_migrates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/glovebox.db");
        let db = connect(path.to_str().unwrap()).await.unwrap();
        let fk: i32 = scalar(&db, "PRAGMA foreign_keys").await;
        assert_eq!(fk, 1);
        let tables = table_names(&db).await;
        for t in [
            "vehicles",
            "service_records",
            "service_recall_links",
            "work_items",
        ] {
            assert!(tables.contains(&t.to_string()), "missing {t}");
        }
        for t in ["builds", "visits", "documents"] {
            assert!(!tables.contains(&t.to_string()), "{t} must not exist");
        }
    }

    fn noon(day: u32) -> chrono::DateTime<chrono::Local> {
        use chrono::TimeZone;
        chrono::Local
            .with_ymd_and_hms(2026, 11, day, 12, 0, 0)
            .single()
            .unwrap()
    }

    // A database file that holds one vehicle, with every connection closed.
    async fn populated(path: &std::path::Path) {
        let db = connect(path.to_str().unwrap()).await.unwrap();
        VehicleFixture::new().insert(&db).await;
        db.close().await.unwrap();
    }

    async fn raw(path: &std::path::Path) -> DatabaseConnection {
        Database::connect(connect_options(Some(path.to_path_buf())))
            .await
            .unwrap()
    }

    async fn forget_newest_migration(db: &DatabaseConnection) {
        db.execute_unprepared(
            "DELETE FROM seaql_migrations WHERE version = (SELECT version FROM seaql_migrations \
             ORDER BY version DESC LIMIT 1)",
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_new_database_is_not_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glovebox.db");
        connect(path.to_str().unwrap()).await.unwrap();
        connect(path.to_str().unwrap()).await.unwrap();
        // Reopening a database that holds no vehicle takes no backup either.
        assert!(backup::list(&path).is_empty());
    }

    #[tokio::test]
    async fn an_up_to_date_database_is_backed_up_once_per_day() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glovebox.db");
        populated(&path).await;
        let db = raw(&path).await;
        for copy in backup::list(&path) {
            std::fs::remove_file(copy).unwrap();
        }

        backup_before_migrating(&db, &path, noon(13)).await.unwrap();
        backup_before_migrating(&db, &path, noon(13)).await.unwrap();
        assert_eq!(backup::list(&path).len(), 1);
        backup_before_migrating(&db, &path, noon(14)).await.unwrap();
        assert_eq!(backup::list(&path).len(), 2);
    }

    #[tokio::test]
    async fn a_pending_migration_forces_a_backup_that_holds_the_old_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glovebox.db");
        populated(&path).await;
        let db = raw(&path).await;
        backup_before_migrating(&db, &path, noon(13)).await.unwrap();
        let before = backup::list(&path).len();
        let applied: i32 = scalar(&db, "SELECT COUNT(*) FROM seaql_migrations").await;
        forget_newest_migration(&db).await;

        // The day already has a backup, and the pending migration still
        // forces another.
        let later = noon(13) + chrono::Duration::seconds(5);
        backup_before_migrating(&db, &path, later).await.unwrap();

        let copies = backup::list(&path);
        assert_eq!(copies.len(), before + 1);
        let copy = raw(copies.last().unwrap()).await;
        let in_copy: i32 = scalar(&copy, "SELECT COUNT(*) FROM seaql_migrations").await;
        assert_eq!(in_copy, applied - 1);
    }

    #[tokio::test]
    async fn a_failed_backup_refuses_a_pending_migration_and_allows_a_plain_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glovebox.db");
        populated(&path).await;
        let db = raw(&path).await;
        // A file where the backups directory belongs makes every backup fail.
        let backups = backup::dir_for(&path);
        std::fs::remove_dir_all(&backups).ok();
        std::fs::write(&backups, b"not a directory").unwrap();

        backup_before_migrating(&db, &path, noon(13)).await.unwrap();

        forget_newest_migration(&db).await;
        let err = backup_before_migrating(&db, &path, noon(13))
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::Internal(m) if m.contains("refusing to migrate")));
    }

    #[tokio::test]
    async fn backup_now_copies_an_existing_database_and_skips_a_missing_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glovebox.db");
        let p = path.to_str().unwrap();
        assert_eq!(backup_now(p).await.unwrap(), None);
        assert!(!path.exists());
        drop(connect(p).await.unwrap());
        let copy = backup_now(p).await.unwrap().unwrap();
        assert!(copy.starts_with(dir.path().join("backups")));
    }

    #[tokio::test]
    async fn connect_uses_wal_journal_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glovebox.db");
        let db = connect(path.to_str().unwrap()).await.unwrap();
        let row = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                "PRAGMA journal_mode",
            ))
            .await
            .unwrap()
            .unwrap();
        let mode: String = row.try_get_by_index(0).unwrap();
        assert_eq!(mode, "wal");
    }

    #[tokio::test]
    async fn deleting_a_vehicle_cascades_to_its_service_records() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glovebox.db");
        let db = connect(path.to_str().unwrap()).await.unwrap();

        let v = VehicleFixture::new().name("Cascade").insert(&db).await;
        let svc = service_record::create(
            &db,
            v.id,
            NewServiceRecord {
                service_date: "2026-01-01".into(),
                mileage: None,
                description: Some("Oil change".into()),
                parts_cost_cents: None,
                parts_cost_currency: None,
                labor_cost_cents: None,
                labor_cost_currency: None,
                total_cost_cents: None,
                total_cost_currency: None,
                shop_name: None,
                shop_id: None,
                notes: None,
                paid_by: None,
                payer_note: None,
                schedule_item_ids: None,
                part_ids: None,
                line_items: None,
                invoice_ref: None,
                end_date: None,
                performed_by: None,
                work_item_ids: None,
                resolved_incident_ids: None,
                noticed_incident_ids: None,
                recall_finding_ids: None,
                parts: None,
                appointment_id: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            scalar(
                &db,
                &format!(
                    "SELECT COUNT(*) FROM service_records WHERE id = {}",
                    svc.record.id
                )
            )
            .await,
            1
        );

        crate::entities::vehicle::Entity::delete_by_id(v.id)
            .exec(&db)
            .await
            .unwrap();

        assert_eq!(
            scalar(
                &db,
                &format!(
                    "SELECT COUNT(*) FROM service_records WHERE vehicle_id = {}",
                    v.id
                )
            )
            .await,
            0
        );
    }

    #[tokio::test]
    async fn the_seed_gives_the_gti_template_a_spark_plug_item_at_60k_miles() {
        let db = crate::test_support::test_db().await;
        let gti_template = crate::entities::model_template::Entity::find()
            .filter(crate::entities::model_template::Column::Model.eq("Golf GTI"))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let v = VehicleFixture::new()
            .model_template_id(gti_template.id)
            .insert_id(&db)
            .await;

        let resolved = schedule::resolve(&db, v).await.unwrap();
        let plugs = resolved
            .iter()
            .find(|r| r.effective_item.name == "Spark Plugs")
            .expect("GTI schedule has Spark Plugs");
        assert_eq!(plugs.effective_item.interval_miles, Some(60000));
    }

    #[tokio::test]
    async fn foreign_keys_reject_a_service_record_for_a_missing_vehicle() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glovebox.db");
        let db = connect(path.to_str().unwrap()).await.unwrap();
        let err = db
            .execute_unprepared(
                "INSERT INTO service_records (vehicle_id, service_date) VALUES (999, '2026-01-01')",
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("FOREIGN KEY"), "{err}");
    }

    #[tokio::test]
    async fn migrations_rerun_without_duplicating_the_seed() {
        let db = crate::test_support::test_db().await;
        // Forgetting the applied versions makes `up` execute both migrations
        // again over the finished schema, as after a crash mid-migration.
        db.execute_unprepared("DELETE FROM seaql_migrations")
            .await
            .unwrap();
        Migrator::up(&db, None).await.unwrap();
        assert_eq!(scalar(&db, "SELECT COUNT(*) FROM platforms").await, 1);
        assert_eq!(scalar(&db, "SELECT COUNT(*) FROM model_templates").await, 2);
        assert_eq!(
            scalar(&db, "SELECT COUNT(*) FROM maintenance_schedule_items").await,
            18
        );
    }

    #[tokio::test]
    async fn two_connections_to_one_file_do_not_fail_with_locked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glovebox.db");
        let first = connect(path.to_str().unwrap()).await.unwrap();
        let second = raw(&path).await;
        // The write lock is held on one pooled connection until `commit`.
        let txn = TransactionTrait::begin(&first).await.unwrap();
        txn.execute_unprepared("INSERT INTO platforms (name) VALUES ('holder')")
            .await
            .unwrap();
        let waiting = tokio::spawn(async move {
            second
                .execute_unprepared("INSERT INTO platforms (name) VALUES ('busy-timeout-probe')")
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        txn.commit().await.unwrap();
        waiting
            .await
            .unwrap()
            .expect("the second write waits for the first");
    }

    #[tokio::test]
    async fn job_checklist_tables_exist_and_cascade() {
        let db = crate::test_support::test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        db.execute_unprepared(&format!(
            "INSERT INTO work_items (vehicle_id, title, status) VALUES ({v}, 'Brakes', 'planned')"
        ))
        .await
        .unwrap();
        let work_item = scalar(&db, "SELECT id FROM work_items").await;
        db.execute_unprepared(&format!(
            "INSERT INTO job_needs (work_item_id, name) VALUES ({work_item}, 'Pads')"
        ))
        .await
        .unwrap();
        let need = scalar(&db, "SELECT id FROM job_needs").await;
        db.execute_unprepared(&format!(
            "INSERT INTO need_options (need_id, description) VALUES ({need}, 'OEM pads')"
        ))
        .await
        .unwrap();
        assert_eq!(scalar(&db, "SELECT quantity FROM job_needs").await, 1);
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) FROM need_options WHERE fitment = 'unconfirmed'"
            )
            .await,
            1
        );

        db.execute_unprepared(&format!("DELETE FROM work_items WHERE id = {work_item}"))
            .await
            .unwrap();

        assert_eq!(scalar(&db, "SELECT COUNT(*) FROM job_needs").await, 0);
        assert_eq!(scalar(&db, "SELECT COUNT(*) FROM need_options").await, 0);
    }

    #[tokio::test]
    async fn the_job_checklist_columns_and_tables_exist() {
        let db = crate::test_support::test_db().await;
        let tables = table_names(&db).await;
        for t in ["appointments", "job_needs", "need_options", "todos"] {
            assert!(tables.contains(&t.to_string()), "missing {t}");
        }
        for (table, column) in [
            ("work_items", "appointment_id"),
            ("service_records", "appointment_id"),
            ("parts", "received_on"),
            ("parts", "returned_on"),
            ("parts", "return_reason"),
            ("parts", "order_ref"),
            ("parts", "returned_from_need_id"),
        ] {
            let n = scalar(
                &db,
                &format!(
                    "SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = '{column}'"
                ),
            )
            .await;
            assert_eq!(n, 1, "missing {table}.{column}");
        }
    }

    #[tokio::test]
    async fn migration_is_rerun_safe() {
        let db = crate::test_support::test_db().await;
        // Forgetting the newest version makes `up` run the job-checklist
        // migration again over the finished schema.
        forget_newest_migration(&db).await;
        Migrator::up(&db, None).await.unwrap();
        assert_eq!(
            scalar(&db, "SELECT COUNT(*) FROM seaql_migrations").await,
            3
        );
    }

    #[tokio::test]
    async fn connect_refuses_a_database_with_an_unknown_migration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glovebox.db");
        populated(&path).await;
        let db = raw(&path).await;
        db.execute_unprepared(
            "INSERT INTO seaql_migrations (version, applied_at) VALUES \
             ('m29990101_000001_future', 1)",
        )
        .await
        .unwrap();
        db.close().await.unwrap();
        let err = connect(path.to_str().unwrap()).await.unwrap_err();
        assert!(err.to_string().contains("rebuild"), "{err}");
        // The refusal happens before any backup is written.
        assert!(!dir.path().join("backups").exists());
    }
}
