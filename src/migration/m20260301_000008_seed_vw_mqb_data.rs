use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[allow(clippy::too_many_lines, clippy::unreadable_literal)]
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Platform: VW MQB (EA888)
        db.execute_unprepared(
            "INSERT INTO platforms (name, notes) VALUES ('VW MQB (EA888)', 'Modular Transverse \
             Matrix platform, 2.0L EA888 turbo inline-4, 2015-2021')",
        )
        .await?;

        // Model Template: 2017 Golf GTI Mk7 (6MT, FWD)
        db.execute_unprepared(
            "INSERT INTO model_templates (platform_id, year, make, model, trim_level, engine, \
             transmission, drivetrain) VALUES (1, 2017, 'Volkswagen', 'Golf GTI', 'SE', '2.0L TSI \
             (EA888 Gen3)', '6MT', 'FWD')",
        )
        .await?;

        // Model Template: 2017 Golf Alltrack Mk7 (DSG, 4MOTION)
        db.execute_unprepared(
            "INSERT INTO model_templates (platform_id, year, make, model, trim_level, engine, \
             transmission, drivetrain) VALUES (1, 2017, 'Volkswagen', 'Golf Alltrack', 'SEL', \
             '1.8L TSI (EA888 Gen3)', 'DSG (DQ250)', '4MOTION')",
        )
        .await?;

        // Platform-level schedule items (shared by both cars)
        let platform_items = [
            (
                "Oil & Filter Change",
                10000,
                12,
                "[\"undercar\"]",
                "Factory interval",
            ),
            ("Spark Plugs", 60000, 0, "[\"engine_bay\"]", ""),
            ("Air Filter", 20000, 24, "[\"engine_bay\"]", ""),
            (
                "Cabin Air Filter",
                20000,
                24,
                "[\"interior\"]",
                "Behind glove box",
            ),
            (
                "Brake Fluid Flush",
                0,
                24,
                "[\"brakes\"]",
                "VW recommends every 2 years",
            ),
            ("Coolant", 100000, 0, "[\"engine_bay\"]", ""),
            (
                "Serpentine Belt",
                80000,
                0,
                "[\"engine_bay\"]",
                "Inspect annually after 60k",
            ),
            ("Tire Rotation", 5000, 6, "[\"wheels_off\"]", ""),
            ("Wiper Blades", 0, 12, "[\"exterior\"]", ""),
        ];

        for (name, miles, months, categories, notes) in &platform_items {
            let miles_clause = if *miles > 0 {
                format!("{miles}")
            } else {
                "NULL".to_string()
            };
            let months_clause = if *months > 0 {
                format!("{months}")
            } else {
                "NULL".to_string()
            };
            let notes_clause = if notes.is_empty() {
                "NULL".to_string()
            } else {
                format!("'{notes}'")
            };

            db.execute_unprepared(&format!(
                "INSERT INTO maintenance_schedule_items (platform_id, name, interval_miles, \
                 interval_months, labor_categories, notes, source, is_factory_recommended) VALUES \
                 ((SELECT id FROM platforms WHERE name = 'VW MQB (EA888)'), '{name}', \
                 {miles_clause}, {months_clause}, '{categories}', {notes_clause}, 'factory', TRUE)"
            ))
            .await?;
        }

        // GTI model template items
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

        for (name, miles, categories, notes) in &gti_items {
            db.execute_unprepared(&format!(
                "INSERT INTO maintenance_schedule_items (model_template_id, name, interval_miles, \
                 labor_categories, notes, source) VALUES ((SELECT id FROM model_templates WHERE \
                 model = 'Golf GTI'), '{name}', {miles}, '{categories}', '{notes}', 'community')"
            ))
            .await?;
        }

        // Alltrack model template items
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

        for (name, miles, categories, notes) in &alltrack_items {
            db.execute_unprepared(&format!(
                "INSERT INTO maintenance_schedule_items (model_template_id, name, interval_miles, \
                 labor_categories, notes, source) VALUES ((SELECT id FROM model_templates WHERE \
                 model = 'Golf Alltrack'), '{name}', {miles}, '{categories}', '{notes}', \
                 'community')"
            ))
            .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DELETE FROM maintenance_schedule_items")
            .await?;
        db.execute_unprepared("DELETE FROM model_templates").await?;
        db.execute_unprepared("DELETE FROM platforms").await?;
        Ok(())
    }
}
