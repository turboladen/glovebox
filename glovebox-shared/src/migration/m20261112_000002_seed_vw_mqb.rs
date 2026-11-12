//! Seed data: the VW MQB (EA888) platform, its two model templates and their
//! maintenance schedule items.
//!
//! Each insert is gated on the row not already existing, so a rerun after a
//! partial `up()` completes the seed without duplicating anything.

// Each function is one linear list of table or seed definitions.
#![allow(clippy::too_many_lines)]

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const PLATFORM: &str = "VW MQB (EA888)";

/// Inserts one platform-level schedule item unless the platform already has an item of that name.
fn platform_item_sql(
    name: &str,
    miles: &str,
    months: &str,
    categories: &str,
    notes: &str,
) -> String {
    format!(
        "INSERT INTO maintenance_schedule_items (platform_id, name, interval_miles, \
         interval_months, labor_categories, notes, source, is_factory_recommended) SELECT (SELECT \
         id FROM platforms WHERE name = '{PLATFORM}'), '{name}', {miles}, {months}, \
         '{categories}', {notes}, 'factory', TRUE WHERE NOT EXISTS (SELECT 1 FROM \
         maintenance_schedule_items WHERE name = '{name}' AND platform_id = (SELECT id FROM \
         platforms WHERE name = '{PLATFORM}'))"
    )
}

/// Inserts one model-template schedule item unless that template already has an item of that name.
fn template_item_sql(model: &str, name: &str, miles: i32, categories: &str, notes: &str) -> String {
    format!(
        "INSERT INTO maintenance_schedule_items (model_template_id, name, interval_miles, \
         labor_categories, notes, source) SELECT (SELECT id FROM model_templates WHERE model = \
         '{model}'), '{name}', {miles}, '{categories}', '{notes}', 'community' WHERE NOT EXISTS \
         (SELECT 1 FROM maintenance_schedule_items WHERE name = '{name}' AND model_template_id = \
         (SELECT id FROM model_templates WHERE model = '{model}'))"
    )
}

fn template_sql(
    year: i32,
    model: &str,
    trim: &str,
    engine: &str,
    transmission: &str,
    drivetrain: &str,
) -> String {
    format!(
        "INSERT INTO model_templates (platform_id, year, make, model, trim_level, engine, \
         transmission, drivetrain) SELECT (SELECT id FROM platforms WHERE name = '{PLATFORM}'), \
         {year}, 'Volkswagen', '{model}', '{trim}', '{engine}', '{transmission}', '{drivetrain}' \
         WHERE NOT EXISTS (SELECT 1 FROM model_templates WHERE model = '{model}')"
    )
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(&format!(
            "INSERT INTO platforms (name, notes) SELECT '{PLATFORM}', 'Modular Transverse Matrix \
             platform, 2.0L EA888 turbo inline-4, 2015-2021' WHERE NOT EXISTS (SELECT 1 FROM \
             platforms WHERE name = '{PLATFORM}')"
        ))
        .await?;

        db.execute_unprepared(&template_sql(
            2017,
            "Golf GTI",
            "SE",
            "2.0L TSI (EA888 Gen3)",
            "6MT",
            "FWD",
        ))
        .await?;
        db.execute_unprepared(&template_sql(
            2017,
            "Golf Alltrack",
            "SEL",
            "1.8L TSI (EA888 Gen3)",
            "DSG (DQ250)",
            "4MOTION",
        ))
        .await?;

        // (name, interval miles, interval months, labor categories, notes);
        // "NULL" marks a missing interval or note.
        let platform_items = [
            (
                "Oil & Filter Change",
                "10000",
                "12",
                "[\"undercar\"]",
                "'Factory interval'",
            ),
            ("Spark Plugs", "60000", "NULL", "[\"engine_bay\"]", "NULL"),
            ("Air Filter", "20000", "24", "[\"engine_bay\"]", "NULL"),
            (
                "Cabin Air Filter",
                "20000",
                "24",
                "[\"interior\"]",
                "'Behind glove box'",
            ),
            (
                "Brake Fluid Flush",
                "NULL",
                "24",
                "[\"brakes\"]",
                "'VW recommends every 2 years'",
            ),
            ("Coolant", "100000", "NULL", "[\"engine_bay\"]", "NULL"),
            (
                "Serpentine Belt",
                "80000",
                "NULL",
                "[\"engine_bay\"]",
                "'Inspect annually after 60k'",
            ),
            ("Tire Rotation", "5000", "6", "[\"wheels_off\"]", "NULL"),
            ("Wiper Blades", "NULL", "12", "[\"exterior\"]", "NULL"),
        ];
        for (name, miles, months, categories, notes) in platform_items {
            db.execute_unprepared(&platform_item_sql(name, miles, months, categories, notes))
                .await?;
        }

        let gti_items = [
            (
                "Manual Transmission Fluid",
                60000,
                "[\"undercar\"]",
                "Not in factory schedule but widely recommended",
            ),
            (
                "Carbon Cleaning (walnut blast)",
                80000,
                "[\"engine_bay\"]",
                "Direct injection buildup",
            ),
            (
                "Timing Chain Tensioner Inspection",
                80000,
                "[\"engine_bay\"]",
                "Known EA888 weak point",
            ),
            (
                "Water Pump Inspection",
                80000,
                "[\"engine_bay\"]",
                "Known failure around 80-100k",
            ),
        ];
        for (name, miles, categories, notes) in gti_items {
            db.execute_unprepared(&template_item_sql(
                "Golf GTI", name, miles, categories, notes,
            ))
            .await?;
        }

        let alltrack_items = [
            (
                "DSG Fluid & Filter",
                40000,
                "[\"undercar\"]",
                "Critical for DSG longevity",
            ),
            (
                "Haldex Fluid & Filter",
                40000,
                "[\"undercar\"]",
                "Critical for AWD system",
            ),
            (
                "Carbon Cleaning (walnut blast)",
                80000,
                "[\"engine_bay\"]",
                "Direct injection buildup",
            ),
            (
                "Timing Chain Tensioner Inspection",
                80000,
                "[\"engine_bay\"]",
                "Known EA888 weak point, especially at high mileage",
            ),
            (
                "Water Pump Inspection",
                80000,
                "[\"engine_bay\"]",
                "Known failure around 80-100k",
            ),
        ];
        for (name, miles, categories, notes) in alltrack_items {
            db.execute_unprepared(&template_item_sql(
                "Golf Alltrack",
                name,
                miles,
                categories,
                notes,
            ))
            .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "DELETE FROM maintenance_schedule_items WHERE source IN ('factory', 'community') AND \
             vehicle_id IS NULL",
        )
        .await?;
        db.execute_unprepared(
            "DELETE FROM model_templates WHERE make = 'Volkswagen' AND model IN ('Golf GTI', \
             'Golf Alltrack')",
        )
        .await?;
        db.execute_unprepared(&format!("DELETE FROM platforms WHERE name = '{PLATFORM}'"))
            .await?;
        Ok(())
    }
}
