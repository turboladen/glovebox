use sea_orm_migration::prelude::*;

mod m20260301_000001_create_core_tables;
mod m20260301_000002_create_maintenance_tables;
mod m20260301_000003_create_service_tables;
mod m20260301_000004_create_tracking_tables;
mod m20260301_000005_create_accident_tables;
mod m20260301_000006_create_parts_tables;
mod m20260301_000007_create_research_and_ai_tables;
mod m20260301_000008_seed_vw_mqb_data;
mod m20260301_000009_add_part_urls;
mod m20260301_000010_add_conversations;
mod m20260301_000011_create_service_record_line_items;
mod m20260301_000012_add_vehicle_archived_at;
mod m20260301_000013_add_fts5_search;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260301_000001_create_core_tables::Migration),
            Box::new(m20260301_000002_create_maintenance_tables::Migration),
            Box::new(m20260301_000003_create_service_tables::Migration),
            Box::new(m20260301_000004_create_tracking_tables::Migration),
            Box::new(m20260301_000005_create_accident_tables::Migration),
            Box::new(m20260301_000006_create_parts_tables::Migration),
            Box::new(m20260301_000007_create_research_and_ai_tables::Migration),
            Box::new(m20260301_000008_seed_vw_mqb_data::Migration),
            Box::new(m20260301_000009_add_part_urls::Migration),
            Box::new(m20260301_000010_add_conversations::Migration),
            Box::new(m20260301_000011_create_service_record_line_items::Migration),
            Box::new(m20260301_000012_add_vehicle_archived_at::Migration),
            Box::new(m20260301_000013_add_fts5_search::Migration),
        ]
    }
}
