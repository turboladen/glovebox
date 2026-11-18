//! Gives a planned job the quote it came from: who quoted it (`quoted_by`),
//! on what day (`quoted_on`), and the service whose visit produced the quote
//! (`quote_service_id`).

use sea_orm_migration::prelude::*;

use super::{m20261113_000001_job_checklist::add_column, schema::index};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column(manager, "work_items", "quoted_by", "TEXT").await?;
        add_column(manager, "work_items", "quoted_on", "TEXT").await?;
        // The key comes with the new column, which starts out null. A key on a
        // column that already holds data is added by editing the table
        // definition instead (ADR 0003).
        add_column(
            manager,
            "work_items",
            "quote_service_id",
            "INTEGER REFERENCES service_records(id) ON DELETE SET NULL",
        )
        .await?;
        manager
            .create_index(index(
                "idx_work_items_quote_service",
                "work_items",
                &["quote_service_id"],
            ))
            .await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The columns are nullable and inert, so `down` leaves them.
        Ok(())
    }
}
