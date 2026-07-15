//! Import idempotency (glovebox-hwaf): two deterministic identity signals.
//!
//! Repeated MCP import attempts created duplicate service records and
//! re-ingested the same invoice. This adds the columns the service layer keys
//! on to make imports idempotent by IDENTITY, not resemblance:
//!
//! - `documents.content_sha256` — hex SHA-256 of the file bytes. The store
//!   path dedupes on `(vehicle_id, content_sha256)`; a retry loop cannot
//!   create N copies of one PDF. NULL for pre-existing rows (no backfill —
//!   the app is pre-production; lazy-hash on new stores is sufficient, and a
//!   NULL never matches a hash-equality filter).
//! - `service_records.invoice_ref` — the invoice number read off the scan.
//!   Unique per vehicle WHEN PRESENT (partial index; `SQLite` treats NULLs as
//!   distinct, so absent refs never collide). Re-recording the same ref
//!   returns the existing record.
//!
//! Both are plain nullable TEXT columns (`SQLite` ADD COLUMN cannot express
//! them any other way); the service layer owns dedup/short-circuit, the
//! partial unique indexes are DB backstops against a concurrent double-insert.
//!
//! Rerun-safety: `SQLite` migrations are NOT transactional. Each ADD COLUMN is
//! guarded by a `pragma_table_info` check (ADD COLUMN has no `IF NOT EXISTS`),
//! and the indexes use `IF NOT EXISTS`, so a crash-rerun is harmless.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let backend = manager.get_database_backend();

        let documents_hash_exists = db
            .query_one(sea_orm::Statement::from_string(
                backend,
                "SELECT 1 FROM pragma_table_info('documents') WHERE name = 'content_sha256'"
                    .to_owned(),
            ))
            .await?
            .is_some();
        if !documents_hash_exists {
            manager
                .alter_table(
                    Table::alter()
                        .table(Documents::Table)
                        .add_column(ColumnDef::new(Documents::ContentSha256).text())
                        .to_owned(),
                )
                .await?;
        }

        let service_ref_exists = db
            .query_one(sea_orm::Statement::from_string(
                backend,
                "SELECT 1 FROM pragma_table_info('service_records') WHERE name = 'invoice_ref'"
                    .to_owned(),
            ))
            .await?
            .is_some();
        if !service_ref_exists {
            manager
                .alter_table(
                    Table::alter()
                        .table(ServiceRecords::Table)
                        .add_column(ColumnDef::new(ServiceRecords::InvoiceRef).text())
                        .to_owned(),
                )
                .await?;
        }

        // Partial unique index: invoice_ref is unique per vehicle only WHEN
        // PRESENT. `SQLite` treats NULLs as distinct, so `WHERE invoice_ref IS
        // NOT NULL` leaves ref-less records unconstrained (they must coexist).
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_service_records_vehicle_invoice_ref ON \
             service_records (vehicle_id, invoice_ref) WHERE invoice_ref IS NOT NULL",
        )
        .await?;

        // Backstop for the content-hash dedup: guarantees no two identical
        // files for the SAME vehicle. Does NOT cover general docs
        // (vehicle_id NULL — SQLite NULLs distinct); those dedup only via the
        // service-level is_null() SELECT.
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_documents_vehicle_content_sha256 ON documents \
             (vehicle_id, content_sha256) WHERE content_sha256 IS NOT NULL",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP INDEX IF EXISTS ux_documents_vehicle_content_sha256")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS ux_service_records_vehicle_invoice_ref")
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(ServiceRecords::Table)
                    .drop_column(ServiceRecords::InvoiceRef)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Documents::Table)
                    .drop_column(Documents::ContentSha256)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Documents {
    Table,
    ContentSha256,
}

#[derive(DeriveIden)]
enum ServiceRecords {
    Table,
    InvoiceRef,
}
