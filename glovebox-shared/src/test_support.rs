use sea_orm::{
    ActiveModelTrait, ConnectionTrait, DatabaseConnection, EntityTrait, PaginatorTrait, Set,
};

use crate::entities::vehicle;

/// A fresh in-memory `SQLite` DB with all migrations applied, opened with the
/// same options as `db::connect` (WAL, foreign keys on). For service-layer
/// unit tests.
pub async fn test_db() -> DatabaseConnection {
    crate::db::connect_in_memory().await
}

/// The date `days` days before [`services::today`](crate::services::today),
/// written `YYYY-MM-DD`. A test that judges a reminder against the current
/// date dates its fixtures with this, so that it holds on whatever day it
/// runs and agrees with the code under test about what "today" is.
pub fn days_ago(days: i64) -> String {
    (crate::services::today() - chrono::Duration::days(days))
        .format("%Y-%m-%d")
        .to_string()
}

/// The number of rows in `E`'s table. Panics if the query fails.
pub async fn row_count<E>(db: &DatabaseConnection) -> u64
where
    E: EntityTrait,
    E::Model: Sync,
{
    E::find().count(db).await.unwrap()
}

/// Sets the stored decision date of a need, so a test can tell an untouched
/// date from one stamped today. Panics if the write fails.
pub async fn set_decided_on(db: &DatabaseConnection, need_id: i32, date: &str) {
    use sea_orm::{ColumnTrait, QueryFilter, sea_query::Expr};

    use crate::entities::job_need;
    job_need::Entity::update_many()
        .col_expr(job_need::Column::DecidedOn, Expr::value(date))
        .filter(job_need::Column::Id.eq(need_id))
        .exec(db)
        .await
        .unwrap();
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
