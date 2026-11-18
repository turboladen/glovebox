use sea_orm_migration::prelude::*;

mod m20261112_000001_initial_schema;
mod m20261112_000002_seed_vw_mqb;
mod m20261113_000001_job_checklist;
mod m20261113_000002_option_choice_history;
mod m20261114_000001_typed_dates;
mod m20261114_000002_utc_timestamps;
mod m20261114_000003_work_item_link_foreign_keys;
mod m20261115_000001_vocabulary_values;
mod m20261117_000001_work_item_quotes;
mod m20261118_000001_part_contents;
mod schema;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20261112_000001_initial_schema::Migration),
            Box::new(m20261112_000002_seed_vw_mqb::Migration),
            Box::new(m20261113_000001_job_checklist::Migration),
            Box::new(m20261113_000002_option_choice_history::Migration),
            Box::new(m20261114_000001_typed_dates::Migration),
            Box::new(m20261114_000002_utc_timestamps::Migration),
            Box::new(m20261114_000003_work_item_link_foreign_keys::Migration),
            Box::new(m20261115_000001_vocabulary_values::Migration),
            Box::new(m20261117_000001_work_item_quotes::Migration),
            Box::new(m20261118_000001_part_contents::Migration),
        ]
    }
}
