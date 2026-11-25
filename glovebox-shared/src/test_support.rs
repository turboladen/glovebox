use sea_orm::{
    ActiveModelTrait, ConnectionTrait, DatabaseConnection, EntityTrait, PaginatorTrait, Set,
    TransactionTrait,
};

use crate::entities::vehicle;

/// A fresh in-memory `SQLite` DB with all migrations applied, opened with the
/// same options as `db::connect` (WAL, foreign keys on). For service-layer
/// unit tests.
pub async fn test_db() -> DatabaseConnection {
    crate::db::connect_in_memory().await
}

/// Three pools on one new database file, for a test that races writes. Each
/// `connect` is its own one-connection pool, as the MCP server and the web
/// page are. The file lives as long as the returned directory.
#[cfg(test)]
pub async fn race_pools() -> (tempfile::TempDir, Vec<DatabaseConnection>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("glovebox.db");
    let path = path.to_str().unwrap();
    let mut pools = Vec::new();
    for _ in 0..3 {
        pools.push(crate::db::connect(path).await.unwrap());
    }
    (dir, pools)
}

/// Asserts that `items` is empty, printing them when it is not.
#[track_caller]
pub fn assert_empty<T: std::fmt::Debug>(items: &[T]) {
    assert!(items.is_empty(), "expected no items, got {items:?}");
}

/// Runs `op` twice on each of `pools` at once, as two page posts and two
/// tool calls on one file do, and returns every result. `op` gets the run's
/// index and its pool.
// A transaction that opens with `services::begin_write` holds the write lock
// before it reads, so no run of it fails on the lock. One that opens with a
// plain `BEGIN` fails a run whose snapshot another pool's commit made stale,
// which is what a race test built on this catches.
pub async fn race<T, F, Fut>(pools: &[DatabaseConnection], op: F) -> Vec<T>
where
    F: Fn(usize, DatabaseConnection) -> Fut,
    Fut: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let mut runs = tokio::task::JoinSet::new();
    for (i, db) in pools.iter().chain(pools.iter()).cloned().enumerate() {
        runs.spawn(op(i, db));
    }
    runs.join_all().await
}

/// The date `days` days before [`services::today`](crate::services::today).
/// A test that judges a reminder against the current date dates its fixtures
/// with this, so that it holds on whatever day it runs and agrees with the
/// code under test about what "today" is.
pub fn days_ago(days: i64) -> chrono::NaiveDate {
    crate::services::today() - chrono::Duration::days(days)
}

/// Reads a date literal such as `"2026-12-01"` or `"2026-12-01 08:30:00"` as
/// whatever type the place it is written wants: a calendar date, an event
/// time, or the text itself. Panics when the literal is not one.
pub fn date<T>(literal: &str) -> T
where
    T: std::str::FromStr,
    T::Err: std::fmt::Debug,
{
    literal.parse().unwrap()
}

/// Creates a planned job titled `title` for `vehicle_id`, on
/// `appointment_id` when one is given, and returns its id. Panics if the
/// write fails.
pub async fn add_job(
    db: &DatabaseConnection,
    vehicle_id: i32,
    title: &str,
    appointment_id: Option<i32>,
) -> i32 {
    crate::services::work_item::create(
        db,
        vehicle_id,
        crate::inputs::work_item::NewWorkItem {
            title: title.into(),
            appointment_id,
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id
}

/// Sets a job's status through the work-item service. Panics if the write
/// fails.
pub async fn set_job_status(
    db: &DatabaseConnection,
    vehicle_id: i32,
    job_id: i32,
    status: crate::entities::work_item::JobStatus,
) {
    crate::services::work_item::update(
        db,
        vehicle_id,
        job_id,
        crate::inputs::work_item::UpdateWorkItem {
            status: Some(status),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

/// The number of rows in `E`'s table. Panics if the query fails.
pub async fn row_count<E>(db: &DatabaseConnection) -> u64
where
    E: EntityTrait,
    E::Model: Sync,
{
    E::find().count(db).await.unwrap()
}

/// Sets the stored decision date of a need that has a chosen option, so a
/// test can tell an untouched date from one stamped today. Panics if the
/// write fails, as it does for a need with no chosen option.
pub async fn set_decided_on(db: &DatabaseConnection, need_id: i32, date: &str) {
    use sea_orm::{ColumnTrait, QueryFilter, sea_query::Expr};

    use crate::entities::job_need;
    job_need::Entity::update_many()
        .col_expr(
            job_need::Column::DecidedOn,
            Expr::value(self::date::<chrono::NaiveDate>(date)),
        )
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
    purchase_date: Option<chrono::NaiveDate>,
    purchase_mileage: Option<i32>,
    warranty_expires_on: Option<chrono::NaiveDate>,
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
    pub fn purchase_date(mut self, v: &(impl ToString + ?Sized)) -> Self {
        self.purchase_date = Some(date(&v.to_string()));
        self
    }

    #[must_use]
    pub fn purchase_mileage(mut self, v: i32) -> Self {
        self.purchase_mileage = Some(v);
        self
    }

    #[must_use]
    pub fn warranty_expires_on(mut self, v: &(impl ToString + ?Sized)) -> Self {
        self.warranty_expires_on = Some(date(&v.to_string()));
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

/// Asserts that `record` serializes its shop as exactly the keys `shop_id`
/// and `name_key`: the id and name in `shop`, or JSON null in both when
/// `shop` is none. No other key of the record may contain `shop`.
///
/// # Panics
/// When `record` does not serialize to an object, or a key or value differs.
pub fn assert_shop_keys<T: serde::Serialize>(
    record: &T,
    name_key: &str,
    shop: Option<(i32, &str)>,
) {
    let json = serde_json::to_value(record).expect("serialize the record");
    let object = json.as_object().expect("the record is a JSON object");
    let mut keys: Vec<&str> = object
        .keys()
        .map(String::as_str)
        .filter(|k| k.contains("shop"))
        .collect();
    keys.sort_unstable();
    let mut expected = ["shop_id", name_key];
    expected.sort_unstable();
    assert_eq!(keys, expected, "got: {json}");
    assert_eq!(object["shop_id"], serde_json::json!(shop.map(|s| s.0)));
    assert_eq!(object[name_key], serde_json::json!(shop.map(|s| s.1)));
}

/// Adds a shop with no address to the directory and returns its id.
pub async fn add_shop<C: ConnectionTrait + TransactionTrait>(db: &C, name: &str) -> i32 {
    crate::services::shop::create(
        db,
        crate::inputs::shop::NewShop {
            name: name.into(),
            ..Default::default()
        },
    )
    .await
    .expect("insert test shop")
    .id
}

/// Inserts a vehicle with only a name and returns its id.
#[cfg(any(test, feature = "test-support"))]
pub async fn seed_vehicle(db: &impl ConnectionTrait) -> i32 {
    VehicleFixture::new().insert_id(db).await
}

/// Inserts a service dated 2024-01-01 with nothing else set and returns its
/// id.
#[cfg(any(test, feature = "test-support"))]
pub async fn seed_service(db: &impl ConnectionTrait, vehicle_id: i32) -> i32 {
    crate::entities::service_record::ActiveModel {
        vehicle_id: Set(vehicle_id),
        service_date: Set(date("2024-01-01")),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert test service")
    .id
}

/// Stores a recall the way a recall check does and returns its finding id.
/// The recall's title is `title`, and NHTSA sent nothing else about it.
#[cfg(any(test, feature = "test-support"))]
pub async fn seed_recall<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    campaign: &str,
    title: &str,
) -> i32 {
    let found = crate::services::nhtsa::RecallInfo {
        campaign_number: campaign.into(),
        manufacturer: None,
        title: title.into(),
        summary: None,
        consequence: None,
        remedy: None,
        report_date: None,
        component: None,
        action_number: None,
    };
    crate::services::research::record_recalls(db, vehicle_id, &[found])
        .await
        .expect("store test recall")
        .recalls
        .into_iter()
        .find(|r| r.campaign_number.as_deref() == Some(campaign))
        .expect("the stored recall is listed")
        .finding_id
}
