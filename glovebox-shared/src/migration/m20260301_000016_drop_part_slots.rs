//! Drops the `part_slots` subsystem (2hea unit C): slot names survive as a
//! plain `location` text column on parts.
//!
//! `SQLite` refuses `ALTER TABLE ... DROP COLUMN` on a column named in the
//! table's foreign key definitions ("error in table parts after drop column:
//! unknown column `slot_id` in foreign key definition"), so this is the
//! standard table rebuild: create the new shape, copy rows (backfilling
//! `location` from the joined slot name), drop the old table, rename.
//! Nothing FK-references `parts` and it has no FTS triggers (absent from
//! 000013's SPECS), so the rebuild has no knock-on schema to restore beyond
//! `idx_parts_vehicle`.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Column order = physical order after 000006 + 000009 + 000014, minus
        // slot_id, with `location` appended last (entity field-order rule).
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS parts_new (id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT, \
             vehicle_id INTEGER NOT NULL, name TEXT NOT NULL, manufacturer TEXT, part_number \
             TEXT, oe_part_number_replaced TEXT, seller TEXT, purchase_date TEXT, cost_cents \
             INTEGER, cost_currency TEXT DEFAULT 'USD', invoice_url TEXT, status TEXT NOT NULL \
             DEFAULT 'purchased', installed_date TEXT, installed_odometer INTEGER, \
             installed_service_id INTEGER, replaced_date TEXT, replaced_odometer INTEGER, notes \
             TEXT, created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL \
             DEFAULT (datetime('now')), manufacturer_url TEXT, retailer_url TEXT, build_id \
             INTEGER, location TEXT, FOREIGN KEY (vehicle_id) REFERENCES vehicles (id) ON DELETE \
             CASCADE, FOREIGN KEY (installed_service_id) REFERENCES service_records (id) ON \
             DELETE SET NULL)",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO parts_new (id, vehicle_id, name, manufacturer, part_number, \
             oe_part_number_replaced, seller, purchase_date, cost_cents, cost_currency, \
             invoice_url, status, installed_date, installed_odometer, installed_service_id, \
             replaced_date, replaced_odometer, notes, created_at, updated_at, manufacturer_url, \
             retailer_url, build_id, location) SELECT id, vehicle_id, name, manufacturer, \
             part_number, oe_part_number_replaced, seller, purchase_date, cost_cents, \
             cost_currency, invoice_url, status, installed_date, installed_odometer, \
             installed_service_id, replaced_date, replaced_odometer, notes, created_at, \
             updated_at, manufacturer_url, retailer_url, build_id, (SELECT name FROM part_slots \
             WHERE part_slots.id = parts.slot_id) FROM parts",
        )
        .await?;

        // Drops idx_parts_vehicle and idx_parts_slot along with the table.
        db.execute_unprepared("DROP TABLE parts").await?;
        db.execute_unprepared("ALTER TABLE parts_new RENAME TO parts")
            .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_parts_vehicle ON parts (vehicle_id)")
            .await?;

        db.execute_unprepared("DROP TABLE IF EXISTS part_slots")
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Migration(
            "2hea unit C drops part_slots permanently; restore from a DB backup instead".into(),
        ))
    }
}
