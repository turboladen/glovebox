//! Drops the columns no code reads and indexes a service's shop.
//!
//! `service_records.shop_name` and `appointments.shop_name` go with the
//! checks that held them empty, and the full-text index over services covers
//! `description` and `notes`. `research_reports` loses `summary`, `raw_data`
//! and `notes`, and `vehicle_attributes` loses `superseded_by` and its foreign
//! key. Every other column keeps its place, and no row is added, removed or
//! given another id.

use sea_orm_migration::prelude::*;

use super::schema::{FtsSpec, create_fts, drop_fts, edit_definitions, has_column, index, scalar};

#[derive(DeriveMigrationName)]
pub struct Migration;

const WHAT: &str = "dropping the unused columns";

const DROPPED: &[(&str, &str)] = &[
    ("service_records", "shop_name"),
    ("appointments", "shop_name"),
    ("research_reports", "summary"),
    ("research_reports", "raw_data"),
    ("research_reports", "notes"),
    ("vehicle_attributes", "superseded_by"),
];

/// A rewrite replaces text of a stored table definition that names a dropped
/// column outside the column's own declaration.
struct Rewrite {
    table: &'static str,
    from: &'static str,
    to: &'static str,
}

// SQLite refuses to drop a column that a CHECK or a foreign key names, so
// each of these leaves the definition first. The spellings are the ones
// `schema::add_checks` and the initial schema's builder store.
const REWRITES: &[Rewrite] = &[
    Rewrite {
        table: "service_records",
        from: ", CONSTRAINT \"service_records_shop_name_is_empty\" CHECK (shop_name IS NULL)",
        to: "",
    },
    // A self-performed service names no shop. The rule keeps its name and
    // loses only the half about the dropped column.
    Rewrite {
        table: "service_records",
        from: "(shop_name IS NULL AND shop_id IS NULL)",
        to: "shop_id IS NULL",
    },
    Rewrite {
        table: "appointments",
        from: ", CONSTRAINT \"appointments_shop_name_is_empty\" CHECK (shop_name IS NULL)",
        to: "",
    },
    Rewrite {
        table: "vehicle_attributes",
        from: ", FOREIGN KEY (\"superseded_by\") REFERENCES \"vehicle_attributes\" (\"id\") ON \
               DELETE SET NULL",
        to: "",
    },
];

const FTS: &[FtsSpec] = &[FtsSpec {
    content: "service_records",
    fts: "fts_service_records",
    cols: &["description", "notes"],
}];

// Counts how many times the stored definition of `table` holds `text`.
async fn occurrences(db: &impl ConnectionTrait, table: &str, text: &str) -> Result<i64, DbErr> {
    let text = text.replace('\'', "''");
    scalar::<i64>(
        db,
        &format!(
            "SELECT (length(sql) - length(replace(sql, '{text}', ''))) / length('{text}') FROM \
             sqlite_schema WHERE type = 'table' AND name = '{table}'"
        ),
    )
    .await
}

/// Takes every constraint that names a dropped column out of the stored
/// definitions, and leaves a definition that holds none as it is.
async fn remove_constraints<C: ConnectionTrait>(txn: &C) -> Result<(), DbErr> {
    let mut pending = Vec::new();
    for rewrite in REWRITES {
        if occurrences(txn, rewrite.table, rewrite.from).await? > 0 {
            pending.push(rewrite);
        }
    }
    if pending.is_empty() {
        return Ok(());
    }
    edit_definitions(txn, WHAT, async |txn: &C| {
        for Rewrite { table, from, to } in &pending {
            txn.execute_unprepared(&format!(
                "UPDATE sqlite_schema SET sql = replace(sql, '{from}', '{to}') WHERE type = \
                 'table' AND name = '{table}'",
                from = from.replace('\'', "''"),
            ))
            .await?;
        }
        Ok(true)
    })
    .await
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // One transaction holds every step, so a failure leaves the database
        // as it was and a rerun starts from there.
        let txn = crate::services::begin_write(manager.get_connection()).await?;
        let schema = SchemaManager::new(&txn);

        schema
            .create_index(index(
                "idx_service_records_shop",
                "service_records",
                &["shop_id"],
            ))
            .await?;

        // The full-text index and its triggers name `shop_name`, and SQLite
        // refuses to drop a column a trigger names, so they go first.
        if has_column(&txn, "fts_service_records", "shop_name").await? {
            drop_fts(&schema, FTS).await?;
        }
        remove_constraints(&txn).await?;

        for (table, column) in DROPPED {
            if !has_column(&txn, table, column).await? {
                continue;
            }
            // The column's own declaration is the one place left that may
            // name it. A definition spelled another way would keep a
            // constraint on the column, and SQLite would refuse the drop
            // with a message that does not say why.
            let named = occurrences(&txn, table, column).await?;
            if named != 1 {
                return Err(DbErr::Migration(format!(
                    "{WHAT} stopped: the stored definition of {table} names {column} {named} \
                     times where one declaration is expected. Nothing was changed."
                )));
            }
            // DROP COLUMN rewrites the rows in place, so every rowid stays
            // and no foreign key action runs, which a `DROP TABLE` of a
            // parent table would set off.
            let drop = Table::alter()
                .table(Alias::new(*table))
                .drop_column(Alias::new(*column))
                .to_owned();
            schema.alter_table(drop).await?;
        }

        create_fts(&schema, FTS).await?;
        // `rebuild` fills the index from the stored rows, and running it
        // again gives the same index.
        txn.execute_unprepared(
            "INSERT INTO fts_service_records(fts_service_records) VALUES ('rebuild')",
        )
        .await?;
        drop(schema);
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The values of the dropped columns are gone and no code reads the
        // columns, so `down` leaves the tables as `up` made them.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, IdenStatic, Iterable, TransactionTrait};
    use sea_orm_migration::MigratorTrait;

    use super::DROPPED;
    use crate::{
        entities::{appointment, research_report, service_record, vehicle_attribute},
        migration::{
            Migrator,
            schema::{
                edit_definitions,
                testing::{count, fts_integrity, migrated_to, surrounding_sql, table_sql, text},
            },
        },
        services::search::{SearchScope, search},
    };

    // Twenty migrations run before this one.
    const BEFORE: u32 = 20;

    const STAMP: &str = "'2026-11-24T00:00:00+00:00'";

    const TABLES: [&str; 4] = [
        "appointments",
        "research_reports",
        "service_records",
        "vehicle_attributes",
    ];

    // Returns a database as the previous migration leaves it. Each table
    // that loses a column holds two rows with a gap between their ids, and
    // one row of each parent table has a child.
    async fn db_before() -> DatabaseConnection {
        let db = migrated_to(BEFORE).await;
        db.execute_unprepared(&format!(
            "INSERT INTO vehicles (id, name, created_at, updated_at)
                 VALUES (1, 'Golf', {STAMP}, {STAMP});
             INSERT INTO shops (id, name, created_at, updated_at)
                 VALUES (4, 'KW Automotive', {STAMP}, {STAMP});
             INSERT INTO appointments (id, vehicle_id, purpose, shop_id, planned_date, notes,
                     closed_on, created_at, updated_at)
                 VALUES (2, 1, 'Brakes', 4, '2026-12-01', 'Bring pads', NULL, {STAMP}, {STAMP}),
                        (6, 1, 'Tint', NULL, NULL, NULL, '2026-11-01', {STAMP}, {STAMP});
             INSERT INTO service_records (id, vehicle_id, service_date, mileage, description,
                     total_cost_cents, notes, shop_id, invoice_ref, performed_by, end_date,
                     appointment_id, created_at, updated_at)
                 VALUES (3, 1, '2026-10-01', 41000, 'Rear brake pads', 12345, 'Squeal gone', 4,
                            'INV-1', 'shop', '2026-10-02', 2, {STAMP}, {STAMP}),
                        (7, 1, '2026-11-01', NULL, 'Wiper blades', NULL, NULL, NULL, NULL, 'self',
                            NULL, NULL, {STAMP}, {STAMP});
             INSERT INTO service_record_line_items (id, service_record_id, description, work_kind,
                     created_at, updated_at)
                 VALUES (11, 3, 'Pads', 'repair', {STAMP}, {STAMP});
             INSERT INTO todos (id, vehicle_id, appointment_id, title, created_at, updated_at)
                 VALUES (5, 1, 2, 'Call ahead', {STAMP}, {STAMP});
             INSERT INTO research_reports (id, vehicle_id, report_type, summary, raw_data, notes,
                     generated_at, created_at)
                 VALUES (8, 1, 'recalls_only', 'Found 1 new recall from NHTSA.', '[]', 'n',
                            {STAMP}, {STAMP}),
                        (12, 1, 'external_research', NULL, NULL, NULL, {STAMP}, {STAMP});
             INSERT INTO research_findings (id, report_id, category, title, status, created_at,
                     updated_at)
                 VALUES (9, 8, 'recall', 'Fuel pump', 'new', {STAMP}, {STAMP});
             INSERT INTO vehicle_attributes (id, vehicle_id, key, value, source, superseded_by,
                     created_at)
                 VALUES (5, 1, 'engine', '2.0T', 'vin_decode', NULL, {STAMP}),
                        (9, 1, 'color', 'gray', NULL, 5, {STAMP});"
        ))
        .await
        .unwrap();
        db
    }

    // Lists the columns a table keeps, in the order the migrated table holds
    // them.
    fn kept(table: &str) -> Vec<&'static str> {
        match table {
            "appointments" => names::<appointment::Column>(),
            "research_reports" => names::<research_report::Column>(),
            "service_records" => names::<service_record::Column>(),
            "vehicle_attributes" => names::<vehicle_attribute::Column>(),
            other => panic!("{other} is not a table this migration changes"),
        }
    }

    fn names<C: Iterable + IdenStatic>() -> Vec<&'static str> {
        C::iter().map(|column| column.as_str()).collect()
    }

    // Returns every row of each changed table, ordered by id, with the value
    // of every kept column.
    async fn rows(db: &DatabaseConnection) -> String {
        let mut out = Vec::new();
        for table in TABLES {
            let values = kept(table)
                .iter()
                .map(|column| format!("quote({column})"))
                .collect::<Vec<_>>()
                .join(" || ',' || ");
            let dump = text(
                db,
                &format!(
                    "SELECT group_concat(row, ';') FROM (SELECT {values} AS row FROM {table} \
                     ORDER BY id)"
                ),
            )
            .await;
            out.push(format!("{table}: {dump}"));
        }
        out.join("\n")
    }

    async fn whole_schema(db: &DatabaseConnection) -> String {
        text(
            db,
            "SELECT group_concat(type || ' ' || name || ' ' || tbl_name || ' ' || ifnull(sql, \
             '-'), ';') FROM (SELECT * FROM sqlite_schema ORDER BY type, name)",
        )
        .await
    }

    async fn matches(db: &DatabaseConnection, word: &str) -> i64 {
        count(
            db,
            &format!(
                "SELECT count(*) FROM fts_service_records WHERE fts_service_records MATCH \
                 '\"{word}\"'"
            ),
        )
        .await
    }

    #[tokio::test]
    async fn every_row_keeps_its_id_its_values_and_its_children() {
        let db = db_before().await;
        let before = rows(&db).await;
        assert!(
            before.contains("3,1,'2026-10-01',41000,'Rear brake pads'"),
            "{before}"
        );
        let children = "SELECT (SELECT count(*) FROM service_record_line_items li JOIN \
                        service_records s ON s.id = li.service_record_id WHERE li.id = 11 AND \
                        s.id = 3) + (SELECT count(*) FROM todos t JOIN appointments a ON a.id = \
                        t.appointment_id WHERE t.id = 5 AND a.id = 2) + (SELECT count(*) FROM \
                        service_records s JOIN appointments a ON a.id = s.appointment_id WHERE \
                        s.id = 3 AND a.id = 2) + (SELECT count(*) FROM research_findings f JOIN \
                        research_reports r ON r.id = f.report_id WHERE f.id = 9 AND r.id = 8)";
        assert_eq!(count(&db, children).await, 4);

        Migrator::up(&db, Some(1)).await.unwrap();

        assert_eq!(rows(&db).await, before);
        assert_eq!(count(&db, children).await, 4);
        assert_eq!(
            count(&db, "SELECT count(*) FROM pragma_foreign_key_check").await,
            0
        );
        assert_eq!(text(&db, "PRAGMA integrity_check").await, "ok");
        // A new row takes the id after the highest one the table ever held.
        db.execute_unprepared(&format!(
            "DELETE FROM vehicle_attributes WHERE id = 9;
             INSERT INTO vehicle_attributes (vehicle_id, key, value, created_at)
                 VALUES (1, 'trim', 'SE', {STAMP});"
        ))
        .await
        .unwrap();
        assert_eq!(
            count(&db, "SELECT max(id) FROM vehicle_attributes").await,
            10
        );
    }

    #[tokio::test]
    async fn the_columns_are_gone_and_each_entity_lists_the_columns_left() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();

        for (table, column) in DROPPED {
            let found = format!(
                "SELECT count(*) FROM pragma_table_info('{table}') WHERE name = '{column}'"
            );
            assert_eq!(count(&db, &found).await, 0, "{table}.{column}");
        }
        for table in TABLES {
            let stored = text(
                &db,
                &format!(
                    "SELECT group_concat(name, ',') FROM (SELECT name FROM \
                     pragma_table_info('{table}') ORDER BY cid)"
                ),
            )
            .await;
            assert_eq!(stored, kept(table).join(","), "{table}");
        }
        let fts_columns = text(
            &db,
            "SELECT group_concat(name, ',') FROM (SELECT name FROM \
             pragma_table_info('fts_service_records') ORDER BY cid)",
        )
        .await;
        assert_eq!(fts_columns, "description,notes");
    }

    #[tokio::test]
    async fn the_shop_index_is_added_and_every_other_index_and_key_stays() {
        let db = db_before().await;
        let keys = "SELECT group_concat(child || '.' || \"from\" || '>' || \"table\" || ':' || \
                    on_delete, ' ') FROM (SELECT m.name AS child, k.* FROM sqlite_schema m, \
                    pragma_foreign_key_list(m.name) k WHERE m.type = 'table' ORDER BY m.name, \
                    k.\"from\")";
        let keys_before = text(&db, keys).await;
        let indexes = "SELECT group_concat(sql, ';') FROM (SELECT sql FROM sqlite_schema WHERE \
                       type = 'index' AND sql IS NOT NULL AND name <> 'idx_service_records_shop' \
                       ORDER BY name)";
        let indexes_before = text(&db, indexes).await;

        Migrator::up(&db, Some(1)).await.unwrap();

        assert_eq!(
            text(
                &db,
                "SELECT sql FROM sqlite_schema WHERE name = 'idx_service_records_shop'"
            )
            .await,
            "CREATE INDEX \"idx_service_records_shop\" ON \"service_records\" (\"shop_id\")"
        );
        assert_eq!(text(&db, indexes).await, indexes_before);
        assert_eq!(
            text(&db, keys).await,
            keys_before.replace(
                " vehicle_attributes.superseded_by>vehicle_attributes:SET NULL",
                ""
            )
        );
        assert!(keys_before.contains("superseded_by"), "{keys_before}");
    }

    #[tokio::test]
    async fn the_rules_left_on_each_table_still_refuse() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();

        let refusals = [
            (
                "service_records_end_after_start",
                "UPDATE service_records SET end_date = '2026-01-01' WHERE id = 3",
            ),
            (
                "service_records_self_performed_has_no_shop",
                "UPDATE service_records SET performed_by = 'self' WHERE id = 3",
            ),
            (
                "appointments_one_ending",
                "UPDATE appointments SET cancelled_on = '2026-11-02' WHERE id = 6",
            ),
            // Neither of these tables has a CHECK, so a key and a unique
            // index stand in for one.
            (
                "FOREIGN KEY constraint failed",
                "UPDATE research_reports SET vehicle_id = 99 WHERE id = 8",
            ),
            (
                "UNIQUE constraint failed",
                "INSERT INTO vehicle_attributes (vehicle_id, key, value, source, created_at) \
                 VALUES (1, 'engine', '1.8T', 'vin_decode', 'x')",
            ),
        ];
        for (rule, sql) in refusals {
            let err = db.execute_unprepared(sql).await.unwrap_err().to_string();
            assert!(err.contains(rule), "{rule}: {err}");
        }
        // Deleting a parent still acts on its children as each key says.
        db.execute_unprepared(
            "DELETE FROM appointments WHERE id = 2; DELETE FROM service_records WHERE id = 3; \
             DELETE FROM research_reports WHERE id = 8;",
        )
        .await
        .unwrap();
        let left = "SELECT (SELECT count(*) FROM service_record_line_items) + (SELECT count(*) \
                    FROM research_findings) + (SELECT count(*) FROM todos WHERE appointment_id IS \
                    NOT NULL)";
        assert_eq!(count(&db, left).await, 0);
        assert_eq!(count(&db, "SELECT count(*) FROM todos").await, 1);
    }

    #[tokio::test]
    async fn the_search_index_follows_services_after_the_migration() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();

        fts_integrity(&db, &["fts_service_records"]).await;
        assert_eq!(matches(&db, "brake").await, 1);
        assert_eq!(matches(&db, "squeal").await, 1);
        let hits = search(&db, "wiper", SearchScope::Services, None)
            .await
            .unwrap();
        let found: Vec<_> = hits.iter().map(|h| (h.kind.as_str(), h.id)).collect();
        assert_eq!(found, [("service", 7)]);

        db.execute_unprepared(&format!(
            "INSERT INTO service_records (id, vehicle_id, service_date, description, \
             performed_by, created_at, updated_at) VALUES (20, 1, '2026-11-20', 'Coolant flush', \
             'shop', {STAMP}, {STAMP});
             UPDATE service_records SET description = 'Front rotors' WHERE id = 3;
             DELETE FROM service_records WHERE id = 7;"
        ))
        .await
        .unwrap();
        assert_eq!(matches(&db, "coolant").await, 1);
        assert_eq!(matches(&db, "rotors").await, 1);
        assert_eq!(matches(&db, "brake").await, 0);
        assert_eq!(matches(&db, "wiper").await, 0);
        fts_integrity(&db, &["fts_service_records"]).await;
    }

    #[tokio::test]
    async fn running_it_again_changes_nothing() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();
        let schema = whole_schema(&db).await;
        let stored = rows(&db).await;

        Migrator::down(&db, Some(1)).await.unwrap();
        Migrator::up(&db, Some(1)).await.unwrap();

        assert_eq!(whole_schema(&db).await, schema);
        assert_eq!(rows(&db).await, stored);
        assert_eq!(matches(&db, "brake").await, 1);
        fts_integrity(&db, &["fts_service_records"]).await;

        // Every migration runs again over the finished schema, the earlier
        // ones that name the dropped columns included.
        db.execute_unprepared("DELETE FROM seaql_migrations")
            .await
            .unwrap();
        Migrator::up(&db, None).await.unwrap();
        assert_eq!(whole_schema(&db).await, schema);
        assert_eq!(rows(&db).await, stored);
        fts_integrity(&db, &["fts_service_records"]).await;
    }

    #[tokio::test]
    async fn a_fresh_database_ends_with_the_schema_of_a_migrated_one() {
        let migrated = db_before().await;
        Migrator::up(&migrated, Some(1)).await.unwrap();
        let fresh = migrated_to(BEFORE + 1).await;

        assert_eq!(whole_schema(&fresh).await, whole_schema(&migrated).await);
        let with_index = [TABLES.as_slice(), &["fts_service_records"]].concat();
        assert_eq!(
            table_sql(&fresh, &with_index).await,
            table_sql(&migrated, &with_index).await
        );
        let around = surrounding_sql(&migrated, &TABLES).await;
        assert_eq!(surrounding_sql(&fresh, &TABLES).await, around);
        assert!(!around.contains("shop_name"), "{around}");
        assert!(around.contains("service_records_fts_au"), "{around}");
    }

    // A constraint on a dropped column that is spelled another way stays in
    // the definition, and the migration stops with nothing changed. The
    // count covers the declaration, the constraint's name and its rule.
    #[tokio::test]
    async fn a_definition_spelled_another_way_stops_it_before_anything_changes() {
        let db = db_before().await;
        let txn = db.begin().await.unwrap();
        edit_definitions(&txn, "the test edit", async |txn| {
            txn.execute_unprepared(
                "UPDATE sqlite_schema SET sql = replace(sql, 'CHECK (shop_name IS NULL)', 'CHECK \
                 (shop_name  IS NULL)') WHERE name = 'appointments'",
            )
            .await?;
            Ok(true)
        })
        .await
        .unwrap();
        txn.commit().await.unwrap();
        let schema = whole_schema(&db).await;
        let stored = rows(&db).await;

        let err = Migrator::up(&db, Some(1)).await.unwrap_err().to_string();

        assert!(
            err.contains(
                "the stored definition of appointments names shop_name 3 times where one \
                 declaration is expected. Nothing was changed."
            ),
            "{err}"
        );
        assert_eq!(whole_schema(&db).await, schema);
        assert_eq!(rows(&db).await, stored);
        assert_eq!(matches(&db, "brake").await, 1);
    }
}
