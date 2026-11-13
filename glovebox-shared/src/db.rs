//! The single place a `SQLite` connection is opened.

use std::path::{Path, PathBuf};

use sea_orm::{
    ConnectOptions, Database, DatabaseConnection,
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
        opts.journal_mode(SqliteJournalMode::Wal).foreign_keys(true)
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
        backup_before_migrating(&db, path).await?;
    }
    Migrator::up(&db, None).await?;
    Ok(db)
}

/// Backs up a database that already holds data: always when a migration is
/// about to change it, and otherwise once per calendar day.
///
/// # Errors
///
/// A failed backup stops the open only when a migration is pending, because
/// that is the case where the file is about to be changed in place.
async fn backup_before_migrating(db: &DatabaseConnection, path: &Path) -> DomainResult<()> {
    let now = chrono::Local::now();
    let pending = !Migrator::get_pending_migrations(db).await?.is_empty();
    if !pending && backup::has_backup_on(path, now) {
        return Ok(());
    }
    match backup::write(db, path, now).await {
        Ok(copy) => {
            tracing::info!(path = %copy.display(), "backed up the database");
            backup::prune(path);
            Ok(())
        }
        Err(e) if pending => Err(DomainError::Internal(format!(
            "refusing to migrate {} because it could not be backed up first: {e}",
            path.display()
        ))),
        Err(e) => {
            tracing::warn!(error = %e, "could not back up the database");
            Ok(())
        }
    }
}

/// Backs up the database at `db_path` now and returns the copy's path, or
/// `None` when the file is missing or empty.
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

    #[tokio::test]
    async fn a_new_database_is_not_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glovebox.db");
        connect(path.to_str().unwrap()).await.unwrap();
        assert!(backup::list(&path).is_empty());
    }

    #[tokio::test]
    async fn reopening_backs_up_once_per_day() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glovebox.db");
        let p = path.to_str().unwrap();
        drop(connect(p).await.unwrap());
        drop(connect(p).await.unwrap());
        assert_eq!(backup::list(&path).len(), 1);
        drop(connect(p).await.unwrap());
        assert_eq!(backup::list(&path).len(), 1);
    }

    #[tokio::test]
    async fn a_pending_migration_forces_a_backup_even_with_one_today() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glovebox.db");
        let p = path.to_str().unwrap();
        drop(connect(p).await.unwrap());
        drop(connect(p).await.unwrap());
        assert_eq!(backup::list(&path).len(), 1);
        // Forgetting the newest migration makes it pending again; it is
        // rerun-safe, so applying it a second time succeeds.
        let raw = Database::connect(connect_options(Some(path.clone())))
            .await
            .unwrap();
        raw.execute_unprepared(
            "DELETE FROM seaql_migrations WHERE version = (SELECT version FROM seaql_migrations \
             ORDER BY version DESC LIMIT 1)",
        )
        .await
        .unwrap();
        drop(raw);
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        drop(connect(p).await.unwrap());
        assert_eq!(backup::list(&path).len(), 2);
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
}
