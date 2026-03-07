use sea_orm_migration::prelude::*;

mod m20260301_000001_create_platforms;
mod m20260301_000002_create_model_templates;
mod m20260301_000003_create_vehicles;
mod m20260301_000004_create_vehicle_attributes;
mod m20260301_000005_create_mileage_log;
mod m20260301_000006_create_maintenance_schedule_items;
mod m20260301_000007_create_service_records;
mod m20260301_000008_create_service_schedule_links;
mod m20260301_000009_create_settings;
mod m20260301_000010_add_schedule_item_fields;
mod m20260301_000011_seed_vw_mqb_data;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260301_000001_create_platforms::Migration),
            Box::new(m20260301_000002_create_model_templates::Migration),
            Box::new(m20260301_000003_create_vehicles::Migration),
            Box::new(m20260301_000004_create_vehicle_attributes::Migration),
            Box::new(m20260301_000005_create_mileage_log::Migration),
            Box::new(m20260301_000006_create_maintenance_schedule_items::Migration),
            Box::new(m20260301_000007_create_service_records::Migration),
            Box::new(m20260301_000008_create_service_schedule_links::Migration),
            Box::new(m20260301_000009_create_settings::Migration),
            Box::new(m20260301_000010_add_schedule_item_fields::Migration),
            Box::new(m20260301_000011_seed_vw_mqb_data::Migration),
        ]
    }
}
