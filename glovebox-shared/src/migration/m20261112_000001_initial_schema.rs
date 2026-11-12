//! The complete schema, as a single migration.
//!
//! Tables and indexes go through the sea-query builder. The FTS5 virtual tables,
//! their triggers and one partial index with a `source IS NULL` predicate have
//! no builder form and use `execute_unprepared`. Every statement is
//! `IF NOT EXISTS`, so a crash in the middle of `up()` (SQLite migrations are
//! not transactional) reruns cleanly.
//!
//! Each FTS5 table is an external-content index over one content table, kept in
//! sync by AFTER INSERT/UPDATE/DELETE triggers. Every indexed column must exist
//! on the content table because FTS5 reads values back by rowid.

// Each function is one linear list of table or seed definitions.
#![allow(clippy::too_many_lines)]

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// One external-content FTS5 index over `content`'s `cols`.
struct FtsSpec {
    content: &'static str,
    fts: &'static str,
    cols: &'static [&'static str],
}

const FTS_SPECS: &[FtsSpec] = &[
    FtsSpec {
        content: "vehicles",
        fts: "fts_vehicles",
        cols: &[
            "name",
            "make",
            "model",
            "trim_level",
            "vin",
            "license_plate",
            "notes",
        ],
    },
    FtsSpec {
        content: "service_records",
        fts: "fts_service_records",
        cols: &["description", "shop_name", "notes"],
    },
    FtsSpec {
        content: "service_record_line_items",
        fts: "fts_service_record_line_items",
        cols: &["description"],
    },
    FtsSpec {
        content: "incidents",
        fts: "fts_incidents",
        cols: &["title", "description", "obd_codes", "notes"],
    },
    FtsSpec {
        content: "incident_followups",
        fts: "fts_incident_followups",
        cols: &["summary", "notes"],
    },
    FtsSpec {
        content: "research_findings",
        fts: "fts_research_findings",
        cols: &["title", "description"],
    },
    FtsSpec {
        content: "maintenance_schedule_items",
        fts: "fts_maintenance_schedule_items",
        cols: &["name", "description", "notes"],
    },
    FtsSpec {
        content: "work_items",
        fts: "fts_work_items",
        cols: &["title", "notes"],
    },
];

/// Tables in dependency order; `down()` drops them in reverse.
const TABLES: &[&str] = &[
    "platforms",
    "model_templates",
    "vehicles",
    "vehicle_attributes",
    "shops",
    "maintenance_schedule_items",
    "service_records",
    "service_schedule_links",
    "mileage_log",
    "research_reports",
    "research_findings",
    "service_record_line_items",
    "parts",
    "incidents",
    "incident_followups",
    "incident_service_links",
    "work_items",
    "service_recall_links",
];

fn c(name: &str) -> ColumnDef {
    ColumnDef::new(Alias::new(name))
}

fn pk() -> ColumnDef {
    c("id")
        .integer()
        .not_null()
        .auto_increment()
        .primary_key()
        .to_owned()
}

fn int(name: &str) -> ColumnDef {
    c(name).integer().to_owned()
}

fn text(name: &str) -> ColumnDef {
    c(name).text().to_owned()
}

fn int_not_null(name: &str) -> ColumnDef {
    c(name).integer().not_null().to_owned()
}

fn text_not_null(name: &str) -> ColumnDef {
    c(name).text().not_null().to_owned()
}

fn text_default(name: &str, default: &str) -> ColumnDef {
    c(name).text().not_null().default(default).to_owned()
}

/// A `TEXT NOT NULL` timestamp that defaults to the insert time.
fn stamp(name: &str) -> ColumnDef {
    c(name)
        .text()
        .not_null()
        .default(Expr::cust("datetime('now')"))
        .to_owned()
}

fn fk(
    table: &str,
    column: &str,
    parent: &str,
    on_delete: ForeignKeyAction,
) -> ForeignKeyCreateStatement {
    ForeignKey::create()
        .from(Alias::new(table), Alias::new(column))
        .to(Alias::new(parent), Alias::new("id"))
        .on_delete(on_delete)
        .to_owned()
}

fn index(name: &str, table: &str, cols: &[&str]) -> IndexCreateStatement {
    let mut idx = Index::create();
    idx.if_not_exists().name(name).table(Alias::new(table));
    for col in cols {
        idx.col(Alias::new(*col));
    }
    idx
}

/// A partial index over `cols`, restricted to rows where `predicate_col IS NOT NULL`.
fn partial_index(
    name: &str,
    table: &str,
    cols: &[&str],
    predicate_col: &str,
    unique: bool,
) -> IndexCreateStatement {
    let mut idx = index(name, table, cols);
    if unique {
        idx.unique();
    }
    idx.and_where(Expr::col(Alias::new(predicate_col)).is_not_null())
        .to_owned()
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_reference_tables(manager).await?;
        create_vehicle_tables(manager).await?;
        create_service_tables(manager).await?;
        create_incident_tables(manager).await?;
        create_planning_tables(manager).await?;
        create_fts(manager).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for spec in FTS_SPECS {
            let FtsSpec { content, fts, .. } = spec;
            for suffix in ["ai", "ad", "au"] {
                db.execute_unprepared(&format!("DROP TRIGGER IF EXISTS {content}_fts_{suffix}"))
                    .await?;
            }
            db.execute_unprepared(&format!("DROP TABLE IF EXISTS {fts}"))
                .await?;
        }
        for table in TABLES.iter().rev() {
            manager
                .drop_table(
                    Table::drop()
                        .table(Alias::new(*table))
                        .if_exists()
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

async fn create_reference_tables(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("platforms"))
                .if_not_exists()
                .col(pk())
                .col(text_not_null("name"))
                .col(text("website_url"))
                .col(text("api_base_url"))
                .col(text("notes"))
                .col(stamp("created_at"))
                .col(stamp("updated_at"))
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Alias::new("model_templates"))
                .if_not_exists()
                .col(pk())
                .col(int("platform_id"))
                .col(text("platform_ref"))
                .col(int("year"))
                .col(text("make"))
                .col(text("model"))
                .col(text("trim_level"))
                .col(text("body_style"))
                .col(text("engine"))
                .col(text("transmission"))
                .col(text("drivetrain"))
                .col(stamp("created_at"))
                .col(stamp("updated_at"))
                .foreign_key(&mut fk(
                    "model_templates",
                    "platform_id",
                    "platforms",
                    ForeignKeyAction::SetNull,
                ))
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Alias::new("shops"))
                .if_not_exists()
                .col(pk())
                .col(text_not_null("name"))
                .col(text("address"))
                .col(text("phone"))
                .col(text("website"))
                .col(text("specialty"))
                .col(text("notes"))
                .col(stamp("created_at"))
                .col(stamp("updated_at"))
                .to_owned(),
        )
        .await
}

async fn create_vehicle_tables(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("vehicles"))
                .if_not_exists()
                .col(pk())
                .col(int("model_template_id"))
                .col(text_not_null("name"))
                .col(int("year"))
                .col(text("make"))
                .col(text("model"))
                .col(text("trim_level"))
                .col(text("body_style"))
                .col(text("engine"))
                .col(text("transmission"))
                .col(text("drivetrain"))
                .col(text("vin"))
                .col(text("license_plate"))
                .col(text("color"))
                .col(text("purchase_date"))
                .col(int("purchase_price_cents"))
                .col(text("purchase_price_currency"))
                .col(int("purchase_mileage"))
                .col(text("sold_date"))
                .col(int("sold_price_cents"))
                .col(text("sold_price_currency"))
                .col(int("sold_mileage"))
                .col(text("notes"))
                .col(stamp("created_at"))
                .col(stamp("updated_at"))
                .col(text("archived_at"))
                .col(text("warranty_expires_on"))
                .col(int("warranty_expires_miles"))
                .foreign_key(&mut fk(
                    "vehicles",
                    "model_template_id",
                    "model_templates",
                    ForeignKeyAction::SetNull,
                ))
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Alias::new("vehicle_attributes"))
                .if_not_exists()
                .col(pk())
                .col(int_not_null("vehicle_id"))
                .col(text_not_null("key"))
                .col(text_not_null("value"))
                .col(text("source"))
                .col(int("superseded_by"))
                .col(stamp("created_at"))
                .foreign_key(&mut fk(
                    "vehicle_attributes",
                    "vehicle_id",
                    "vehicles",
                    ForeignKeyAction::Cascade,
                ))
                .foreign_key(&mut fk(
                    "vehicle_attributes",
                    "superseded_by",
                    "vehicle_attributes",
                    ForeignKeyAction::SetNull,
                ))
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            index(
                "idx_vehicle_attributes_unique",
                "vehicle_attributes",
                &["vehicle_id", "key", "source"],
            )
            .unique()
            .to_owned(),
        )
        .await?;
    manager
        .create_index(index(
            "idx_vehicle_attributes_vehicle_key",
            "vehicle_attributes",
            &["vehicle_id", "key"],
        ))
        .await?;
    // A NULL `source` never collides under the plain unique index above, so
    // sourceless attributes get their own partial unique index.
    manager
        .get_connection()
        .execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_vehicle_attributes_unique_no_source ON \
             vehicle_attributes(vehicle_id, key) WHERE source IS NULL",
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Alias::new("maintenance_schedule_items"))
                .if_not_exists()
                .col(pk())
                .col(int("platform_id"))
                .col(int("model_template_id"))
                .col(int("vehicle_id"))
                .col(int("overrides_item_id"))
                .col(text_not_null("name"))
                .col(text("description"))
                .col(int("interval_miles"))
                .col(int("interval_months"))
                .col(text("labor_categories"))
                .col(stamp("created_at"))
                .col(stamp("updated_at"))
                .col(c("warning_miles").integer().default(1000))
                .col(c("warning_days").integer().default(30))
                .col(c("enabled").boolean().not_null().default(true))
                .col(text("source"))
                .col(text("notes"))
                .col(c("is_factory_recommended").boolean().default(false))
                .col(int("est_cost_cents"))
                .foreign_key(&mut fk(
                    "maintenance_schedule_items",
                    "platform_id",
                    "platforms",
                    ForeignKeyAction::SetNull,
                ))
                .foreign_key(&mut fk(
                    "maintenance_schedule_items",
                    "model_template_id",
                    "model_templates",
                    ForeignKeyAction::SetNull,
                ))
                .foreign_key(&mut fk(
                    "maintenance_schedule_items",
                    "vehicle_id",
                    "vehicles",
                    ForeignKeyAction::Cascade,
                ))
                .foreign_key(&mut fk(
                    "maintenance_schedule_items",
                    "overrides_item_id",
                    "maintenance_schedule_items",
                    ForeignKeyAction::SetNull,
                ))
                // Exactly one owner level per row.
                .check(Expr::cust(
                    "(platform_id IS NOT NULL AND model_template_id IS NULL AND vehicle_id IS \
                     NULL) OR (platform_id IS NULL AND model_template_id IS NOT NULL AND \
                     vehicle_id IS NULL) OR (platform_id IS NULL AND model_template_id IS NULL \
                     AND vehicle_id IS NOT NULL)",
                ))
                .to_owned(),
        )
        .await?;
    for (name, col) in [
        ("idx_msi_platform", "platform_id"),
        ("idx_msi_model_template", "model_template_id"),
        ("idx_msi_vehicle", "vehicle_id"),
    ] {
        manager
            .create_index(partial_index(
                name,
                "maintenance_schedule_items",
                &[col],
                col,
                false,
            ))
            .await?;
    }
    Ok(())
}

async fn create_service_tables(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("service_records"))
                .if_not_exists()
                .col(pk())
                .col(int_not_null("vehicle_id"))
                .col(text_not_null("service_date"))
                .col(int("mileage"))
                .col(text("description"))
                .col(int("parts_cost_cents"))
                .col(text("parts_cost_currency"))
                .col(int("labor_cost_cents"))
                .col(text("labor_cost_currency"))
                .col(int("total_cost_cents"))
                .col(text("total_cost_currency"))
                .col(text("shop_name"))
                .col(text("notes"))
                .col(stamp("created_at"))
                .col(stamp("updated_at"))
                .col(int("shop_id"))
                .col(text_default("paid_by", "self"))
                .col(text("payer_note"))
                .col(text("invoice_ref"))
                .col(text_default("performed_by", "shop"))
                .col(text("end_date"))
                .foreign_key(&mut fk(
                    "service_records",
                    "vehicle_id",
                    "vehicles",
                    ForeignKeyAction::Cascade,
                ))
                .foreign_key(&mut fk(
                    "service_records",
                    "shop_id",
                    "shops",
                    ForeignKeyAction::SetNull,
                ))
                .to_owned(),
        )
        .await?;
    manager
        .create_index(partial_index(
            "ux_service_records_vehicle_invoice_ref",
            "service_records",
            &["vehicle_id", "invoice_ref"],
            "invoice_ref",
            true,
        ))
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Alias::new("service_schedule_links"))
                .if_not_exists()
                .col(int_not_null("service_record_id"))
                .col(int_not_null("schedule_item_id"))
                .primary_key(
                    Index::create()
                        .col(Alias::new("service_record_id"))
                        .col(Alias::new("schedule_item_id")),
                )
                .foreign_key(&mut fk(
                    "service_schedule_links",
                    "service_record_id",
                    "service_records",
                    ForeignKeyAction::Cascade,
                ))
                .foreign_key(&mut fk(
                    "service_schedule_links",
                    "schedule_item_id",
                    "maintenance_schedule_items",
                    ForeignKeyAction::Cascade,
                ))
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Alias::new("mileage_log"))
                .if_not_exists()
                .col(pk())
                .col(int_not_null("vehicle_id"))
                .col(int_not_null("mileage"))
                .col(text_not_null("recorded_at"))
                .col(text("source"))
                .col(text("notes"))
                .col(stamp("created_at"))
                .col(int("service_record_id"))
                .foreign_key(&mut fk(
                    "mileage_log",
                    "vehicle_id",
                    "vehicles",
                    ForeignKeyAction::Cascade,
                ))
                .foreign_key(&mut fk(
                    "mileage_log",
                    "service_record_id",
                    "service_records",
                    ForeignKeyAction::SetNull,
                ))
                .to_owned(),
        )
        .await?;
    manager
        .create_index(index(
            "idx_mileage_log_vehicle_recorded",
            "mileage_log",
            &["vehicle_id", "recorded_at"],
        ))
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Alias::new("service_record_line_items"))
                .if_not_exists()
                .col(pk())
                .col(int_not_null("service_record_id"))
                .col(text_not_null("description"))
                .col(c("quantity").double())
                .col(int("unit_cost_cents"))
                .col(int("cost_cents"))
                .col(stamp("created_at"))
                .col(stamp("updated_at"))
                .col(text_not_null("work_kind"))
                .foreign_key(&mut fk(
                    "service_record_line_items",
                    "service_record_id",
                    "service_records",
                    ForeignKeyAction::Cascade,
                ))
                .to_owned(),
        )
        .await?;
    manager
        .create_index(index(
            "idx_service_record_line_items_service_record_id",
            "service_record_line_items",
            &["service_record_id"],
        ))
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Alias::new("parts"))
                .if_not_exists()
                .col(pk())
                .col(int_not_null("vehicle_id"))
                .col(text_not_null("name"))
                .col(text("manufacturer"))
                .col(text("part_number"))
                .col(text("oe_part_number_replaced"))
                .col(text("seller"))
                .col(text("purchase_date"))
                .col(int("cost_cents"))
                .col(c("cost_currency").text().default("USD"))
                .col(text_default("status", "purchased"))
                .col(text("installed_date"))
                .col(int("installed_odometer"))
                .col(int("installed_service_id"))
                .col(text("replaced_date"))
                .col(int("replaced_odometer"))
                .col(text("notes"))
                .col(stamp("created_at"))
                .col(stamp("updated_at"))
                .col(text("manufacturer_url"))
                .col(text("retailer_url"))
                .col(text("location"))
                .col(text("warranty_expires_on"))
                .col(int("warranty_expires_miles"))
                .col(text_default("supplied_by", "self"))
                .foreign_key(&mut fk(
                    "parts",
                    "vehicle_id",
                    "vehicles",
                    ForeignKeyAction::Cascade,
                ))
                .foreign_key(&mut fk(
                    "parts",
                    "installed_service_id",
                    "service_records",
                    ForeignKeyAction::SetNull,
                ))
                .to_owned(),
        )
        .await?;
    manager
        .create_index(index("idx_parts_vehicle", "parts", &["vehicle_id"]))
        .await
}

async fn create_incident_tables(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("research_reports"))
                .if_not_exists()
                .col(pk())
                .col(int_not_null("vehicle_id"))
                .col(text("report_type"))
                .col(text("summary"))
                .col(text("raw_data"))
                .col(text("notes"))
                .col(stamp("generated_at"))
                .col(stamp("created_at"))
                .foreign_key(&mut fk(
                    "research_reports",
                    "vehicle_id",
                    "vehicles",
                    ForeignKeyAction::Cascade,
                ))
                .to_owned(),
        )
        .await?;
    manager
        .create_index(index(
            "idx_research_reports_vehicle",
            "research_reports",
            &["vehicle_id"],
        ))
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Alias::new("research_findings"))
                .if_not_exists()
                .col(pk())
                .col(int_not_null("report_id"))
                .col(text_not_null("category"))
                .col(text_not_null("title"))
                .col(text("description"))
                .col(text("source_url"))
                .col(text("severity"))
                .col(text_default("status", "new"))
                .col(stamp("created_at"))
                .col(stamp("updated_at"))
                .foreign_key(&mut fk(
                    "research_findings",
                    "report_id",
                    "research_reports",
                    ForeignKeyAction::Cascade,
                ))
                .to_owned(),
        )
        .await?;
    manager
        .create_index(index(
            "idx_research_findings_report",
            "research_findings",
            &["report_id"],
        ))
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Alias::new("incidents"))
                .if_not_exists()
                .col(pk())
                .col(int_not_null("vehicle_id"))
                .col(text_not_null("category"))
                .col(text_not_null("title"))
                .col(text("description"))
                .col(int("odometer"))
                .col(stamp("occurred_at"))
                .col(text("obd_codes"))
                .col(c("resolved").boolean().not_null().default(false))
                .col(text("notes"))
                .col(text("fault"))
                .col(text("other_party_name"))
                .col(text("other_party_phone"))
                .col(text("other_party_email"))
                .col(text("other_party_insurance"))
                .col(text("other_party_policy_number"))
                .col(text("insurance_claim_number"))
                .col(text("insurance_adjuster"))
                .col(text("insurance_adjuster_phone"))
                .col(int("total_repair_cost_cents"))
                .col(text("total_repair_cost_currency"))
                .col(int("deductible_cents"))
                .col(text("deductible_currency"))
                .col(int("insurance_payout_cents"))
                .col(text("insurance_payout_currency"))
                .col(int("recurrence_of_id"))
                .col(stamp("created_at"))
                .col(stamp("updated_at"))
                .foreign_key(&mut fk(
                    "incidents",
                    "vehicle_id",
                    "vehicles",
                    ForeignKeyAction::Cascade,
                ))
                .foreign_key(&mut fk(
                    "incidents",
                    "recurrence_of_id",
                    "incidents",
                    ForeignKeyAction::SetNull,
                ))
                .to_owned(),
        )
        .await?;
    manager
        .create_index(index("idx_incidents_vehicle", "incidents", &["vehicle_id"]))
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Alias::new("incident_followups"))
                .if_not_exists()
                .col(pk())
                .col(int_not_null("incident_id"))
                .col(text_not_null("occurred_at"))
                .col(text("contact_method"))
                .col(text("contact_with"))
                .col(text_not_null("summary"))
                .col(text("notes"))
                .col(stamp("created_at"))
                .foreign_key(&mut fk(
                    "incident_followups",
                    "incident_id",
                    "incidents",
                    ForeignKeyAction::Cascade,
                ))
                .to_owned(),
        )
        .await?;
    manager
        .create_index(index(
            "idx_incident_followups_incident",
            "incident_followups",
            &["incident_id"],
        ))
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Alias::new("incident_service_links"))
                .if_not_exists()
                .col(int_not_null("incident_id"))
                .col(int_not_null("service_record_id"))
                .col(text_not_null("role"))
                .primary_key(
                    Index::create()
                        .col(Alias::new("incident_id"))
                        .col(Alias::new("service_record_id"))
                        .col(Alias::new("role")),
                )
                .foreign_key(&mut fk(
                    "incident_service_links",
                    "incident_id",
                    "incidents",
                    ForeignKeyAction::Cascade,
                ))
                .foreign_key(&mut fk(
                    "incident_service_links",
                    "service_record_id",
                    "service_records",
                    ForeignKeyAction::Cascade,
                ))
                .to_owned(),
        )
        .await
}

async fn create_planning_tables(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    // The schedule, finding and incident ids are plain integers checked by the
    // service layer. `completed_service_id` is a real foreign key so that
    // deleting the service returns the item to the plan.
    manager
        .create_table(
            Table::create()
                .table(Alias::new("work_items"))
                .if_not_exists()
                .col(pk())
                .col(int_not_null("vehicle_id"))
                .col(text_not_null("title"))
                .col(text("notes"))
                .col(int("schedule_item_id"))
                .col(int("research_finding_id"))
                .col(int("incident_id"))
                .col(int("est_cost_cents"))
                .col(text_default("status", "planned"))
                .col(stamp("created_at"))
                .col(stamp("updated_at"))
                .col(text("revisit_by_date"))
                .col(int("revisit_by_miles"))
                .col(int("completed_service_id"))
                .foreign_key(&mut fk(
                    "work_items",
                    "vehicle_id",
                    "vehicles",
                    ForeignKeyAction::Cascade,
                ))
                .foreign_key(&mut fk(
                    "work_items",
                    "completed_service_id",
                    "service_records",
                    ForeignKeyAction::SetNull,
                ))
                .to_owned(),
        )
        .await?;
    manager
        .create_index(index(
            "idx_work_items_vehicle",
            "work_items",
            &["vehicle_id"],
        ))
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Alias::new("service_recall_links"))
                .if_not_exists()
                .col(int_not_null("service_record_id"))
                .col(int_not_null("research_finding_id"))
                .primary_key(
                    Index::create()
                        .col(Alias::new("service_record_id"))
                        .col(Alias::new("research_finding_id")),
                )
                .foreign_key(&mut fk(
                    "service_recall_links",
                    "service_record_id",
                    "service_records",
                    ForeignKeyAction::Cascade,
                ))
                .foreign_key(&mut fk(
                    "service_recall_links",
                    "research_finding_id",
                    "research_findings",
                    ForeignKeyAction::Cascade,
                ))
                .to_owned(),
        )
        .await
}

async fn create_fts(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let db = manager.get_connection();
    for spec in FTS_SPECS {
        let FtsSpec { content, fts, .. } = spec;
        let cols = spec.cols.join(", ");
        let new_vals = spec
            .cols
            .iter()
            .map(|c| format!("new.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let old_vals = spec
            .cols
            .iter()
            .map(|c| format!("old.{c}"))
            .collect::<Vec<_>>()
            .join(", ");

        db.execute_unprepared(&format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS {fts} USING fts5({cols}, content='{content}', \
             content_rowid='id')"
        ))
        .await?;
        db.execute_unprepared(&format!(
            "CREATE TRIGGER IF NOT EXISTS {content}_fts_ai AFTER INSERT ON {content} BEGIN INSERT \
             INTO {fts}(rowid, {cols}) VALUES (new.id, {new_vals}); END"
        ))
        .await?;
        // External-content FTS5 deletes take the OLD column values alongside the
        // special 'delete' command.
        db.execute_unprepared(&format!(
            "CREATE TRIGGER IF NOT EXISTS {content}_fts_ad AFTER DELETE ON {content} BEGIN INSERT \
             INTO {fts}({fts}, rowid, {cols}) VALUES ('delete', old.id, {old_vals}); END"
        ))
        .await?;
        db.execute_unprepared(&format!(
            "CREATE TRIGGER IF NOT EXISTS {content}_fts_au AFTER UPDATE ON {content} BEGIN INSERT \
             INTO {fts}({fts}, rowid, {cols}) VALUES ('delete', old.id, {old_vals}); INSERT INTO \
             {fts}(rowid, {cols}) VALUES (new.id, {new_vals}); END"
        ))
        .await?;
    }
    Ok(())
}
