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
mod m20261118_000002_parts_search;
mod m20261119_000001_part_life_cycle_checks;
mod m20261119_000002_need_decision_checks;
mod m20261119_000003_appointment_ending_check;
mod m20261119_000004_single_row_checks;
mod m20261120_000001_job_schedule_links;
mod m20261120_000002_job_incident_links;
mod m20261120_000003_incident_resolved_by_hand;
mod m20261121_000001_shops_search;
mod m20261121_000002_shops_from_names;
mod m20261124_000001_drop_unused_columns;
mod m20261124_000002_finding_vocabularies;
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
            Box::new(m20261118_000002_parts_search::Migration),
            Box::new(m20261119_000001_part_life_cycle_checks::Migration),
            Box::new(m20261119_000002_need_decision_checks::Migration),
            Box::new(m20261119_000003_appointment_ending_check::Migration),
            Box::new(m20261119_000004_single_row_checks::Migration),
            Box::new(m20261120_000001_job_schedule_links::Migration),
            Box::new(m20261120_000002_job_incident_links::Migration),
            Box::new(m20261120_000003_incident_resolved_by_hand::Migration),
            Box::new(m20261121_000001_shops_search::Migration),
            Box::new(m20261121_000002_shops_from_names::Migration),
            Box::new(m20261124_000001_drop_unused_columns::Migration),
            Box::new(m20261124_000002_finding_vocabularies::Migration),
        ]
    }
}
