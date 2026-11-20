//! Removes each stored `resolved` link that names the incident of a job the
//! same service completes.
//!
//! A job's incident is read from the job, and a stored link means the invoice
//! named the incident. A row written for a completed job cannot say which it
//! was, so it is taken as the job's. Each incident is resolved by the same
//! services before and after. Running it again finds no such row and removes
//! nothing.

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum IncidentServiceLinks {
    Table,
    IncidentId,
    ServiceRecordId,
    Role,
}

#[derive(DeriveIden)]
enum WorkItems {
    Table,
    CompletedServiceId,
    IncidentId,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let link = |column| (IncidentServiceLinks::Table, column);
        let repeating_job = Query::select()
            .expr(Expr::val(1))
            .from(WorkItems::Table)
            .and_where(
                Expr::col((WorkItems::Table, WorkItems::CompletedServiceId))
                    .equals(link(IncidentServiceLinks::ServiceRecordId)),
            )
            .and_where(
                Expr::col((WorkItems::Table, WorkItems::IncidentId))
                    .equals(link(IncidentServiceLinks::IncidentId)),
            )
            .to_owned();
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(IncidentServiceLinks::Table)
                    .and_where(Expr::col(IncidentServiceLinks::Role).eq("resolved"))
                    .cond_where(Expr::exists(repeating_job))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Nothing is undone: the removed rows said nothing the jobs do not.
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

    // Sixteen migrations run before this one.
    const BEFORE: u32 = 16;

    const STAMP: &str = "'2026-11-20T00:00:00+00:00'";

    #[tokio::test]
    async fn only_a_resolved_link_that_repeats_a_completed_jobs_incident_is_removed() {
        let db = migrated_to(BEFORE).await;
        // Service 1 completes a job filed from incident 1, noticed that
        // incident, and resolved incident 2 with no job. Service 2 resolved
        // incident 1 with no job, and a planned job from incident 2 completes
        // nothing.
        db.execute_unprepared(&format!(
            "INSERT INTO vehicles (id, name, created_at, updated_at)
                 VALUES (1, 'Golf', {STAMP}, {STAMP});
             INSERT INTO service_records (id, vehicle_id, service_date, created_at, updated_at)
                 VALUES (1, 1, '2026-01-02', {STAMP}, {STAMP}),
                        (2, 1, '2026-02-02', {STAMP}, {STAMP});
             INSERT INTO incidents (id, vehicle_id, category, title, occurred_at, created_at,
                     updated_at)
                 VALUES (1, 1, 'noise', 'Rattle', '2026-01-01', {STAMP}, {STAMP}),
                        (2, 1, 'noise', 'Squeak', '2026-01-01', {STAMP}, {STAMP});
             INSERT INTO work_items (id, vehicle_id, title, status, completed_service_id,
                     incident_id, created_at, updated_at)
                 VALUES (1, 1, 'Fix rattle', 'done', 1, 1, {STAMP}, {STAMP}),
                        (2, 1, 'Fix squeak', 'planned', NULL, 2, {STAMP}, {STAMP});
             INSERT INTO incident_service_links (incident_id, service_record_id, role)
                 VALUES (1, 1, 'resolved'), (1, 1, 'noticed'), (2, 1, 'resolved'),
                        (1, 2, 'resolved');"
        ))
        .await
        .unwrap();
        let links = || {
            text(
                &db,
                "SELECT group_concat(incident_id || ':' || service_record_id || ':' || role, ' ') \
                 FROM (SELECT * FROM incident_service_links ORDER BY 1, 2, 3)",
            )
        };
        let kept = "1:1:noticed 1:2:resolved 2:1:resolved";

        Migrator::up(&db, Some(1)).await.unwrap();
        assert_eq!(links().await, kept);

        Migrator::down(&db, Some(1)).await.unwrap();
        Migrator::up(&db, Some(1)).await.unwrap();
        assert_eq!(links().await, kept);
    }
}
