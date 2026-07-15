use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;

use crate::migration::Migrator;

#[cfg(any(test, feature = "test-support"))]
use crate::entities::vehicle;
#[cfg(any(test, feature = "test-support"))]
use sea_orm::{ActiveModelTrait, ConnectionTrait, Set};

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

/// Test-only builder for a `vehicle` row. The single construction point for
/// vehicle fixtures across the service test modules — a future
/// ALTER TABLE-appended column touches only this constructor, not the ~25
/// former per-module `seed_vehicle` bodies.
///
/// Only fields a caller explicitly sets become `Set(Some(..))`; every other
/// field stays `NotSet` via `..Default::default()`, so an insert is
/// row-equivalent to the hand-written `vehicle::ActiveModel { .. }` literals
/// it replaces. `name` defaults to `"Car"`.
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct VehicleFixture {
    name: Option<String>,
    make: Option<String>,
    model_template_id: Option<i32>,
    purchase_date: Option<String>,
    purchase_mileage: Option<i32>,
    warranty_expires_on: Option<String>,
    warranty_expires_miles: Option<i32>,
}

#[cfg(any(test, feature = "test-support"))]
impl VehicleFixture {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn name(mut self, v: &str) -> Self {
        self.name = Some(v.into());
        self
    }

    #[must_use]
    pub fn make(mut self, v: &str) -> Self {
        self.make = Some(v.into());
        self
    }

    #[must_use]
    pub fn model_template_id(mut self, v: i32) -> Self {
        self.model_template_id = Some(v);
        self
    }

    #[must_use]
    pub fn purchase_date(mut self, v: &str) -> Self {
        self.purchase_date = Some(v.into());
        self
    }

    #[must_use]
    pub fn purchase_mileage(mut self, v: i32) -> Self {
        self.purchase_mileage = Some(v);
        self
    }

    #[must_use]
    pub fn warranty_expires_on(mut self, v: &str) -> Self {
        self.warranty_expires_on = Some(v.into());
        self
    }

    #[must_use]
    pub fn warranty_expires_miles(mut self, v: i32) -> Self {
        self.warranty_expires_miles = Some(v);
        self
    }

    /// Insert the vehicle and return the full row.
    pub async fn insert(self, db: &impl ConnectionTrait) -> vehicle::Model {
        let mut m = vehicle::ActiveModel {
            name: Set(self.name.unwrap_or_else(|| "Car".into())),
            ..Default::default()
        };
        if let Some(v) = self.make {
            m.make = Set(Some(v));
        }
        if let Some(v) = self.model_template_id {
            m.model_template_id = Set(Some(v));
        }
        if let Some(v) = self.purchase_date {
            m.purchase_date = Set(Some(v));
        }
        if let Some(v) = self.purchase_mileage {
            m.purchase_mileage = Set(Some(v));
        }
        if let Some(v) = self.warranty_expires_on {
            m.warranty_expires_on = Set(Some(v));
        }
        if let Some(v) = self.warranty_expires_miles {
            m.warranty_expires_miles = Set(Some(v));
        }
        m.insert(db).await.expect("insert test vehicle")
    }

    /// Insert the vehicle and return just its id (the common case).
    pub async fn insert_id(self, db: &impl ConnectionTrait) -> i32 {
        self.insert(db).await.id
    }
}
