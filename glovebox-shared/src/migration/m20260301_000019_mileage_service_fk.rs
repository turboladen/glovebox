//! `mileage_log.service_record_id` FK (2hea unit E, folds in glovebox-w6ws).
//!
//! Service records that carry a mileage auto-create a `mileage_log` row; until
//! now the only linkage was the `source = "service"` label, which left two
//! holes in the activity feed's dedupe (documented in `services/activity.rs`):
//! logs orphaned by deleted services stayed hidden, and manual logs labeled
//! `"service"` were wrongly hidden. This adds the real FK the service layer
//! now maintains (create/update/delete) and the feed keys on.
//!
//! Plain nullable INT column — `SQLite` ALTER TABLE ADD COLUMN cannot add FK
//! constraints; the service layer enforces the linkage (000009/000012/000014
//! pattern).
//!
//! Rerun-safety: `SQLite` migrations are NOT transactional, and unlike the bare
//! single-ALTER migrations (000012/000014, where a crash-rerun's loud
//! "duplicate column" failure loses nothing), this `up()` has a backfill step
//! AFTER the ALTER. A crash between the two would wedge the rerun before the
//! backfill ever ran, so the ALTER is guarded by a `pragma_table_info` check
//! (ADD COLUMN has no IF NOT EXISTS) and the backfill is idempotent
//! (`service_record_id IS NULL`).

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        let column_exists = db
            .query_one(sea_orm::Statement::from_string(
                manager.get_database_backend(),
                "SELECT 1 FROM pragma_table_info('mileage_log') WHERE name = 'service_record_id'"
                    .to_owned(),
            ))
            .await?
            .is_some();

        if !column_exists {
            manager
                .alter_table(
                    Table::alter()
                        .table(MileageLog::Table)
                        .add_column(ColumnDef::new(MileageLog::ServiceRecordId).integer())
                        .to_owned(),
                )
                .await?;
        }

        // Best-effort backfill for pre-FK auto-logs: match on (vehicle,
        // mileage). Heuristic — if several services share a vehicle+mileage,
        // the correlated subquery picks one arbitrarily. Acceptable for
        // pre-ship data; the service layer sets the FK exactly from here on.
        db.execute_unprepared(
            "UPDATE mileage_log SET service_record_id = ( SELECT s.id FROM service_records s \
             WHERE s.vehicle_id = mileage_log.vehicle_id AND s.mileage = mileage_log.mileage \
             LIMIT 1 ) WHERE source = 'service' AND service_record_id IS NULL",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(MileageLog::Table)
                    .drop_column(MileageLog::ServiceRecordId)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum MileageLog {
    Table,
    ServiceRecordId,
}
