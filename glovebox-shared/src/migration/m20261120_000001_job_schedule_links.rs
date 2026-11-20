//! Removes each stored schedule link that names the schedule item of a job
//! the same service completes.
//!
//! A job's schedule item is read from the job, so such a row repeats it. The
//! service satisfies the same schedule items before and after. Running it
//! again finds no such row and removes nothing.

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum ServiceScheduleLinks {
    Table,
    ServiceRecordId,
    ScheduleItemId,
}

#[derive(DeriveIden)]
enum WorkItems {
    Table,
    CompletedServiceId,
    ScheduleItemId,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let link = |column| (ServiceScheduleLinks::Table, column);
        let repeating_job = Query::select()
            .expr(Expr::val(1))
            .from(WorkItems::Table)
            .and_where(
                Expr::col((WorkItems::Table, WorkItems::CompletedServiceId))
                    .equals(link(ServiceScheduleLinks::ServiceRecordId)),
            )
            .and_where(
                Expr::col((WorkItems::Table, WorkItems::ScheduleItemId))
                    .equals(link(ServiceScheduleLinks::ScheduleItemId)),
            )
            .to_owned();
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(ServiceScheduleLinks::Table)
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

    // The number of migrations that run before this one.
    const BEFORE: u32 = 15;

    const STAMP: &str = "'2026-11-20T00:00:00+00:00'";

    #[tokio::test]
    async fn only_a_link_that_repeats_a_completed_jobs_item_is_removed() {
        let db = migrated_to(BEFORE).await;
        // Service 1 completes a job for item 901 and also names item 902.
        // Service 2 names item 901 and completes no job, and a planned job
        // for item 902 completes nothing.
        db.execute_unprepared(&format!(
            "INSERT INTO vehicles (id, name, created_at, updated_at)
                 VALUES (1, 'Golf', {STAMP}, {STAMP});
             INSERT INTO service_records (id, vehicle_id, service_date, created_at, updated_at)
                 VALUES (1, 1, '2026-01-02', {STAMP}, {STAMP}),
                        (2, 1, '2026-02-02', {STAMP}, {STAMP});
             INSERT INTO maintenance_schedule_items (id, vehicle_id, name, created_at,
                     updated_at)
                 VALUES (901, 1, 'Brake fluid', {STAMP}, {STAMP}),
                        (902, 1, 'Cabin filter', {STAMP}, {STAMP});
             INSERT INTO work_items (id, vehicle_id, title, status, completed_service_id,
                     schedule_item_id, created_at, updated_at)
                 VALUES (1, 1, 'Flush', 'done', 1, 901, {STAMP}, {STAMP}),
                        (2, 1, 'Filter', 'planned', NULL, 902, {STAMP}, {STAMP});
             INSERT INTO service_schedule_links (service_record_id, schedule_item_id)
                 VALUES (1, 901), (1, 902), (2, 901);"
        ))
        .await
        .unwrap();
        let links = || {
            text(
                &db,
                "SELECT group_concat(service_record_id || ':' || schedule_item_id, ' ') FROM \
                 (SELECT * FROM service_schedule_links ORDER BY 1, 2)",
            )
        };

        Migrator::up(&db, Some(1)).await.unwrap();
        assert_eq!(links().await, "1:902 2:901");

        Migrator::down(&db, Some(1)).await.unwrap();
        Migrator::up(&db, Some(1)).await.unwrap();
        assert_eq!(links().await, "1:902 2:901");
    }
}
