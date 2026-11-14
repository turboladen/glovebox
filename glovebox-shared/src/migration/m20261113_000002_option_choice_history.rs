//! Columns on `need_options` that keep the reason and date a need chose the
//! option, so a choice that is later rejected keeps the owner's reasoning.
//!
//! Each column is skipped when `pragma_table_info` already lists it, because
//! `SQLite` migrations are not transactional.

use sea_orm_migration::prelude::*;

use super::m20261113_000001_job_checklist::add_column;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column(manager, "need_options", "chosen_reason", "TEXT").await?;
        add_column(manager, "need_options", "chosen_on", "TEXT").await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The columns are nullable and inert, so `down` leaves them.
        Ok(())
    }
}
