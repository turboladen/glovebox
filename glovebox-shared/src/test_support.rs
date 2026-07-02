use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;

use crate::migration::Migrator;

/// A fresh in-memory SQLite DB with all migrations applied. For service-layer unit tests.
///
/// `sqlite::memory:` databases are per-connection, so the pool is pinned to a single
/// connection: migrations and the test's queries share one physical connection, and the
/// migrated schema cannot vanish under a second connection. Pinning it explicitly (rather
/// than relying on SeaORM's implicit max-connections default) keeps each test's DB isolated
/// and non-flaky.
pub async fn test_db() -> DatabaseConnection {
    let mut opt = ConnectOptions::new("sqlite::memory:");
    opt.max_connections(1).min_connections(1);
    let db = Database::connect(opt)
        .await
        .expect("connect in-memory sqlite");
    Migrator::up(&db, None).await.expect("run migrations");
    db
}
