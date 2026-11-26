//! Adds `research_findings.searched_model`, the NHTSA model line a recall was
//! found under when it is not the vehicle's own model ("Golf" for a Golf
//! GTI). Stored recalls keep NULL, which reads as the vehicle's own model,
//! until the next check finds them again. A CHECK keeps the column on recalls
//! only.

use sea_orm_migration::prelude::*;

use super::{
    m20261113_000001_job_checklist::add_column,
    schema::{Check, add_checks},
};

// The rule as this migration adds it. A rule changes only through a new
// migration, because `has_check` finds a constraint by its name and a fresh
// database must end up with what a migrated one has.
const CHECKS: &[Check] = &[Check {
    table: "research_findings",
    name: "research_findings_searched_model_is_recall",
    holds: "searched_model IS NULL OR category = 'recall'",
}];

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column(manager, "research_findings", "searched_model", "TEXT").await?;
        // One transaction pins one connection, which `PRAGMA writable_schema`
        // needs, and holds the write lock from its start.
        let txn = crate::services::begin_write(manager.get_connection()).await?;
        add_checks(
            &txn,
            "adding the model-line check of research findings",
            CHECKS,
        )
        .await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The column and the constraint are inert once the code that reads
        // them is gone, so `down` leaves them.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::ConnectionTrait;
    use sea_orm_migration::MigratorTrait;

    use crate::migration::{
        Migrator,
        schema::testing::{migrated_to, text},
    };

    // Twenty-two migrations run before this one.
    const BEFORE: u32 = 22;

    const STAMP: &str = "'2026-11-25T00:00:00+00:00'";

    #[tokio::test]
    async fn a_stored_recall_reads_no_model_line_and_only_a_recall_takes_one() {
        let db = migrated_to(BEFORE).await;
        db.execute_unprepared(&format!(
            "INSERT INTO vehicles (id, name, created_at, updated_at)
                 VALUES (1, 'Golf', {STAMP}, {STAMP});
             INSERT INTO research_reports (id, vehicle_id, report_type, generated_at, created_at)
                 VALUES (1, 1, 'recalls_only', {STAMP}, {STAMP});
             INSERT INTO research_findings (id, report_id, category, title, status, created_at,
                     updated_at)
                 VALUES (1, 1, 'recall', 'Airbag', 'new', {STAMP}, {STAMP}),
                        (2, 1, 'known_issue', 'Water pump', 'new', {STAMP}, {STAMP});"
        ))
        .await
        .unwrap();
        let lines = || {
            text(
                &db,
                "SELECT group_concat(coalesce(searched_model, '-'), ' ') FROM (SELECT \
                 searched_model FROM research_findings ORDER BY id)",
            )
        };

        Migrator::up(&db, Some(1)).await.unwrap();
        assert_eq!(lines().await, "- -");

        Migrator::down(&db, Some(1)).await.unwrap();
        Migrator::up(&db, Some(1)).await.unwrap();
        assert_eq!(lines().await, "- -");

        db.execute_unprepared("UPDATE research_findings SET searched_model = 'Golf' WHERE id = 1")
            .await
            .unwrap();
        let refused = db
            .execute_unprepared("UPDATE research_findings SET searched_model = 'Golf' WHERE id = 2")
            .await;
        assert!(refused.is_err());
    }
}
