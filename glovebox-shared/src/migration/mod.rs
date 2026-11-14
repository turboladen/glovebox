use sea_orm_migration::prelude::*;

mod m20261112_000001_initial_schema;
mod m20261112_000002_seed_vw_mqb;
mod m20261113_000001_job_checklist;
mod m20261113_000002_option_choice_history;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20261112_000001_initial_schema::Migration),
            Box::new(m20261112_000002_seed_vw_mqb::Migration),
            Box::new(m20261113_000001_job_checklist::Migration),
            Box::new(m20261113_000002_option_choice_history::Migration),
        ]
    }
}
