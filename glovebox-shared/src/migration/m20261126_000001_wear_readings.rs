//! Adds `wear_readings`, one measured thickness or tread depth of a brake
//! pad, a brake rotor or the tires, stored as the number and unit the shop
//! wrote, and the tag `parts.wears_component` and `parts.wears_axle` that
//! marks a part as a new one of those, so its install starts a new series.

use sea_orm_migration::prelude::*;

use super::{
    m20261113_000001_job_checklist::add_column,
    schema::{Check, add_checks, index},
};

// The table's rules as this migration writes them. A rule changes only
// through a new migration.
const CREATE_WEAR_READINGS: &str = "CREATE TABLE IF NOT EXISTS wear_readings (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    vehicle_id INTEGER NOT NULL,
    service_id INTEGER,
    component TEXT NOT NULL,
    axle TEXT NOT NULL,
    value REAL NOT NULL,
    unit TEXT NOT NULL,
    limit_value REAL,
    limit_unit TEXT,
    measured_on TEXT NOT NULL,
    odometer INTEGER,
    notes TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (vehicle_id) REFERENCES vehicles (id) ON DELETE CASCADE,
    FOREIGN KEY (service_id) REFERENCES service_records (id) ON DELETE SET NULL,
    CONSTRAINT wear_readings_component_listed
        CHECK (component IN ('brake_pads', 'brake_rotors', 'tire_tread')),
    CONSTRAINT wear_readings_axle_listed CHECK (axle IN ('front', 'rear')),
    CONSTRAINT wear_readings_unit_listed CHECK (unit IN ('mm', 'in_32', 'in')),
    CONSTRAINT wear_readings_value_not_negative CHECK (value >= 0),
    CONSTRAINT wear_readings_limit_has_unit
        CHECK ((limit_value IS NULL) = (limit_unit IS NULL)),
    CONSTRAINT wear_readings_limit_not_negative CHECK (limit_value IS NULL OR limit_value >= 0),
    CONSTRAINT wear_readings_limit_unit_listed
        CHECK (limit_unit IS NULL OR limit_unit IN ('mm', 'in_32', 'in')),
    CONSTRAINT wear_readings_odometer_not_negative CHECK (odometer IS NULL OR odometer >= 0)
)";

// A brake part names its axle; the tires are one set, so a tire part names
// none. A CHECK passes on NULL, so each arm tests NULL with `IS`.
const PART_CHECKS: &[Check] = &[Check {
    table: "parts",
    name: "parts_wears_as",
    holds: "(wears_component IS NULL AND wears_axle IS NULL) OR (wears_component IS 'tire_tread' \
            AND wears_axle IS NULL) OR (wears_component IS NOT NULL AND wears_component IN \
            ('brake_pads', 'brake_rotors') AND wears_axle IS NOT NULL AND wears_axle IN ('front', \
            'rear'))",
}];

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(CREATE_WEAR_READINGS).await?;
        manager
            .create_index(index(
                "idx_wear_readings_vehicle_component",
                "wear_readings",
                &["vehicle_id", "component"],
            ))
            .await?;
        manager
            .create_index(index(
                "idx_wear_readings_service",
                "wear_readings",
                &["service_id"],
            ))
            .await?;
        add_column(manager, "parts", "wears_component", "TEXT").await?;
        add_column(manager, "parts", "wears_axle", "TEXT").await?;
        // One transaction pins one connection, which `PRAGMA writable_schema`
        // needs, and holds the write lock from its start.
        let txn = crate::services::begin_write(db).await?;
        add_checks(&txn, "adding the wear tag check of parts", PART_CHECKS).await?;
        txn.commit().await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // The part columns and their check are inert once the code that reads
        // them is gone, so `down` drops only the table.
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("wear_readings"))
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::ConnectionTrait;
    use sea_orm_migration::MigratorTrait;

    use crate::migration::{
        Migrator,
        schema::testing::{count, migrated_to},
    };

    // Twenty-three migrations run before this one.
    const BEFORE: u32 = 23;

    const STAMP: &str = "'2026-11-26T00:00:00+00:00'";

    async fn reading(db: &sea_orm::DatabaseConnection, values: &str) -> Result<(), sea_orm::DbErr> {
        db.execute_unprepared(&format!(
            "INSERT INTO wear_readings (vehicle_id, component, axle, value, unit, limit_value,
                 limit_unit, measured_on, created_at, updated_at)
             VALUES (1, {values}, '2026-11-26', {STAMP}, {STAMP})"
        ))
        .await
        .map(|_| ())
    }

    #[tokio::test]
    async fn the_table_and_the_part_tag_keep_their_rules_and_rerun() {
        let db = migrated_to(BEFORE).await;
        db.execute_unprepared(&format!(
            "INSERT INTO vehicles (id, name, created_at, updated_at)
                 VALUES (1, 'Golf', {STAMP}, {STAMP});
             INSERT INTO parts (id, vehicle_id, name, status, supplied_by, created_at, updated_at)
                 VALUES (1, 1, 'Pads', 'purchased', 'self', {STAMP}, {STAMP});"
        ))
        .await
        .unwrap();

        Migrator::up(&db, Some(1)).await.unwrap();
        Migrator::down(&db, Some(1)).await.unwrap();
        Migrator::up(&db, Some(1)).await.unwrap();

        reading(&db, "'brake_pads', 'front', 4, 'mm', 3, 'mm'")
            .await
            .unwrap();
        reading(&db, "'tire_tread', 'rear', 7, 'in_32', NULL, NULL")
            .await
            .unwrap();
        for bad in [
            "'wipers', 'front', 4, 'mm', NULL, NULL",
            "'brake_pads', 'left', 4, 'mm', NULL, NULL",
            "'brake_pads', 'front', 4, 'cm', NULL, NULL",
            "'brake_pads', 'front', -1, 'mm', NULL, NULL",
            "'brake_pads', 'front', 4, 'mm', 3, NULL",
            "'brake_pads', 'front', 4, 'mm', NULL, 'mm'",
        ] {
            assert!(reading(&db, bad).await.is_err(), "{bad}");
        }
        assert_eq!(count(&db, "SELECT count(*) FROM wear_readings").await, 2);

        let tag = |component: &'static str, axle: &'static str| {
            let db = &db;
            async move {
                db.execute_unprepared(&format!(
                    "UPDATE parts SET wears_component = {component}, wears_axle = {axle} WHERE id \
                     = 1"
                ))
                .await
            }
        };
        assert!(tag("'brake_pads'", "'front'").await.is_ok());
        assert!(tag("'tire_tread'", "NULL").await.is_ok());
        assert!(tag("NULL", "NULL").await.is_ok());
        assert!(tag("'brake_pads'", "NULL").await.is_err());
        assert!(tag("'tire_tread'", "'front'").await.is_err());
        assert!(tag("NULL", "'rear'").await.is_err());
    }
}
