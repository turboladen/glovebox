//! Adds `incidents.resolved_by_hand`, which says that the owner's word is what
//! resolved the incident, and marks each resolved incident that no service
//! resolves.
//!
//! A service resolves an incident through a stored `resolved` link or through
//! a completed job filed from it. A resolved incident with a resolving service
//! stays unmarked. A CHECK keeps the marker on resolved incidents only.

use sea_orm_migration::prelude::*;

use super::{
    m20261113_000001_job_checklist::add_column,
    schema::{Check, add_checks},
};

// The rule as this migration adds it. A rule changes only through a new
// migration, because `has_check` finds a constraint by its name and a fresh
// database must end up with what a migrated one has.
const CHECKS: &[Check] = &[Check {
    table: "incidents",
    name: "incidents_resolved_by_hand_is_resolved",
    holds: "resolved_by_hand = 0 OR resolved = 1",
}];

#[derive(DeriveIden)]
enum Incidents {
    Table,
    Id,
    Resolved,
    ResolvedByHand,
}

#[derive(DeriveIden)]
enum IncidentServiceLinks {
    Table,
    IncidentId,
    Role,
}

#[derive(DeriveIden)]
enum WorkItems {
    Table,
    IncidentId,
    CompletedServiceId,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column(
            manager,
            "incidents",
            "resolved_by_hand",
            "BOOLEAN NOT NULL DEFAULT 0",
        )
        .await?;

        let stored_resolver = Query::select()
            .expr(Expr::val(1))
            .from(IncidentServiceLinks::Table)
            .and_where(
                Expr::col((
                    IncidentServiceLinks::Table,
                    IncidentServiceLinks::IncidentId,
                ))
                .equals((Incidents::Table, Incidents::Id)),
            )
            .and_where(
                Expr::col((IncidentServiceLinks::Table, IncidentServiceLinks::Role)).eq("resolved"),
            )
            .to_owned();
        let completed_job = Query::select()
            .expr(Expr::val(1))
            .from(WorkItems::Table)
            .and_where(
                Expr::col((WorkItems::Table, WorkItems::IncidentId))
                    .equals((Incidents::Table, Incidents::Id)),
            )
            .and_where(Expr::col((WorkItems::Table, WorkItems::CompletedServiceId)).is_not_null())
            .to_owned();
        // A second run happens only when the first one failed before it was
        // recorded, and the database refuses to open for this build until
        // the migration is recorded, so the rows are the ones the first run saw.
        manager
            .exec_stmt(
                Query::update()
                    .table(Incidents::Table)
                    .value(Incidents::ResolvedByHand, 1)
                    .and_where(Expr::col(Incidents::Resolved).eq(1))
                    .and_where(Expr::not_exists(stored_resolver))
                    .and_where(Expr::not_exists(completed_job))
                    .to_owned(),
            )
            .await?;

        // One transaction pins one connection, which `PRAGMA writable_schema`
        // needs, holds the write lock from its start and leaves nothing half
        // done when a step fails.
        let txn = crate::services::begin_write(manager.get_connection()).await?;
        add_checks(
            &txn,
            "adding the resolved-by-hand check of incidents",
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

    // Seventeen migrations run before this one.
    const BEFORE: u32 = 17;

    const STAMP: &str = "'2026-11-20T00:00:00+00:00'";

    #[tokio::test]
    async fn only_a_resolved_incident_that_no_service_resolves_is_marked() {
        let db = migrated_to(BEFORE).await;
        // Incident 1 is resolved with no resolver. Incident 2 has a stored
        // `resolved` link, incident 3 a completed job, incident 4 is open and
        // incident 5 is resolved with only a `noticed` link and a planned job.
        db.execute_unprepared(&format!(
            "INSERT INTO vehicles (id, name, created_at, updated_at)
                 VALUES (1, 'Golf', {STAMP}, {STAMP});
             INSERT INTO service_records (id, vehicle_id, service_date, created_at, updated_at)
                 VALUES (1, 1, '2026-01-02', {STAMP}, {STAMP});
             INSERT INTO incidents (id, vehicle_id, category, title, occurred_at, resolved,
                     created_at, updated_at)
                 VALUES (1, 1, 'noise', 'Hand', '2026-01-01', 1, {STAMP}, {STAMP}),
                        (2, 1, 'noise', 'Linked', '2026-01-01', 1, {STAMP}, {STAMP}),
                        (3, 1, 'noise', 'Job', '2026-01-01', 1, {STAMP}, {STAMP}),
                        (4, 1, 'noise', 'Open', '2026-01-01', 0, {STAMP}, {STAMP}),
                        (5, 1, 'noise', 'Noticed', '2026-01-01', 1, {STAMP}, {STAMP});
             INSERT INTO incident_service_links (incident_id, service_record_id, role)
                 VALUES (2, 1, 'resolved'), (5, 1, 'noticed');
             INSERT INTO work_items (id, vehicle_id, title, status, completed_service_id,
                     incident_id, created_at, updated_at)
                 VALUES (1, 1, 'Fix', 'done', 1, 3, {STAMP}, {STAMP}),
                        (2, 1, 'Plan', 'planned', NULL, 5, {STAMP}, {STAMP});"
        ))
        .await
        .unwrap();
        let marked = || {
            text(
                &db,
                "SELECT group_concat(id, ' ') FROM (SELECT id FROM incidents WHERE \
                 resolved_by_hand = 1 ORDER BY id)",
            )
        };

        Migrator::up(&db, Some(1)).await.unwrap();
        assert_eq!(marked().await, "1 5");

        Migrator::down(&db, Some(1)).await.unwrap();
        Migrator::up(&db, Some(1)).await.unwrap();
        assert_eq!(marked().await, "1 5");

        let refused = db
            .execute_unprepared("UPDATE incidents SET resolved = 0 WHERE id = 1")
            .await;
        assert!(refused.is_err());
    }
}
