//! Tables for appointments, job needs, need options and to-dos, plus the
//! columns that tie them to work items, services and parts.
//!
//! Every statement is rerun-safe because `SQLite` migrations are not
//! transactional: tables and indexes are `IF NOT EXISTS`, and each added
//! column is skipped when `pragma_table_info` already lists it.

// `up` is one linear list of table definitions.
#![allow(clippy::too_many_lines)]

use sea_orm_migration::{prelude::*, sea_orm::Statement};

use super::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Tables in dependency order; `down()` drops them in reverse.
const TABLES: &[&str] = &["appointments", "job_needs", "need_options", "todos"];

/// Adds `column` to `table` with the given SQL definition unless the table
/// already has it. `SQLite` accepts a `REFERENCES` clause here only because the
/// default is NULL.
pub(super) async fn add_column(
    manager: &SchemaManager<'_>,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), DbErr> {
    let db = manager.get_connection();
    let present = db
        .query_one_raw(Statement::from_string(
            manager.get_database_backend(),
            format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = '{column}'"),
        ))
        .await?;
    if present.is_none() {
        db.execute_unprepared(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {definition}"
        ))
        .await?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("appointments"))
                    .if_not_exists()
                    .col(pk())
                    .col(int_not_null("vehicle_id"))
                    .col(text_not_null("purpose"))
                    .col(text("shop_name"))
                    .col(int("shop_id"))
                    .col(text("planned_date"))
                    .col(text("notes"))
                    .col(text("cancelled_on"))
                    .col(text("closed_on"))
                    .col(stamp("created_at"))
                    .col(stamp("updated_at"))
                    .foreign_key(&mut fk(
                        "appointments",
                        "vehicle_id",
                        "vehicles",
                        ForeignKeyAction::Cascade,
                    ))
                    .foreign_key(&mut fk(
                        "appointments",
                        "shop_id",
                        "shops",
                        ForeignKeyAction::SetNull,
                    ))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(index(
                "idx_appointments_vehicle",
                "appointments",
                &["vehicle_id"],
            ))
            .await?;
        manager
            .create_index(index("idx_appointments_shop", "appointments", &["shop_id"]))
            .await?;

        // `chosen_option_id` points at `need_options`, which is created next.
        // SQLite resolves foreign key targets when a row is written, so the
        // forward reference is valid.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("job_needs"))
                    .if_not_exists()
                    .col(pk())
                    .col(int_not_null("work_item_id"))
                    .col(text_not_null("name"))
                    .col(c("quantity").integer().not_null().default(1).to_owned())
                    .col(text("supplied_by"))
                    .col(text("approach"))
                    .col(int("chosen_option_id"))
                    .col(text("decision_reason"))
                    .col(text("decided_on"))
                    .col(int("part_id"))
                    .col(stamp("created_at"))
                    .col(stamp("updated_at"))
                    .foreign_key(&mut fk(
                        "job_needs",
                        "work_item_id",
                        "work_items",
                        ForeignKeyAction::Cascade,
                    ))
                    .foreign_key(&mut fk(
                        "job_needs",
                        "chosen_option_id",
                        "need_options",
                        ForeignKeyAction::SetNull,
                    ))
                    .foreign_key(&mut fk(
                        "job_needs",
                        "part_id",
                        "parts",
                        ForeignKeyAction::SetNull,
                    ))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(index(
                "idx_job_needs_work_item",
                "job_needs",
                &["work_item_id"],
            ))
            .await?;
        manager
            .create_index(index(
                "idx_job_needs_chosen_option",
                "job_needs",
                &["chosen_option_id"],
            ))
            .await?;
        // A part belongs to at most one need.
        manager
            .create_index(
                index("idx_job_needs_part_unique", "job_needs", &["part_id"])
                    .unique()
                    .and_where(Expr::col(Alias::new("part_id")).is_not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Alias::new("need_options"))
                    .if_not_exists()
                    .col(pk())
                    .col(int_not_null("need_id"))
                    .col(text_not_null("description"))
                    .col(text("manufacturer"))
                    .col(text("part_number"))
                    .col(text("seller"))
                    .col(int("price_cents"))
                    .col(text("url"))
                    .col(
                        c("fitment")
                            .text()
                            .not_null()
                            .default("unconfirmed")
                            .to_owned(),
                    )
                    .col(text("fitment_note"))
                    .col(text("rejected_reason"))
                    .col(text("rejected_on"))
                    .col(stamp("created_at"))
                    .col(stamp("updated_at"))
                    .foreign_key(&mut fk(
                        "need_options",
                        "need_id",
                        "job_needs",
                        ForeignKeyAction::Cascade,
                    ))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(index("idx_need_options_need", "need_options", &["need_id"]))
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Alias::new("todos"))
                    .if_not_exists()
                    .col(pk())
                    .col(int_not_null("vehicle_id"))
                    .col(int("appointment_id"))
                    .col(text_not_null("title"))
                    .col(text("notes"))
                    .col(text("done_on"))
                    .col(stamp("created_at"))
                    .col(stamp("updated_at"))
                    .foreign_key(&mut fk(
                        "todos",
                        "vehicle_id",
                        "vehicles",
                        ForeignKeyAction::Cascade,
                    ))
                    .foreign_key(&mut fk(
                        "todos",
                        "appointment_id",
                        "appointments",
                        ForeignKeyAction::SetNull,
                    ))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(index("idx_todos_vehicle", "todos", &["vehicle_id"]))
            .await?;
        manager
            .create_index(index("idx_todos_appointment", "todos", &["appointment_id"]))
            .await?;

        let appointment_ref = "INTEGER REFERENCES appointments(id) ON DELETE SET NULL";
        add_column(manager, "work_items", "appointment_id", appointment_ref).await?;
        add_column(
            manager,
            "service_records",
            "appointment_id",
            appointment_ref,
        )
        .await?;
        manager
            .create_index(index(
                "idx_work_items_appointment",
                "work_items",
                &["appointment_id"],
            ))
            .await?;
        manager
            .create_index(index(
                "idx_service_records_appointment",
                "service_records",
                &["appointment_id"],
            ))
            .await?;

        add_column(manager, "parts", "received_on", "TEXT").await?;
        add_column(manager, "parts", "returned_on", "TEXT").await?;
        add_column(manager, "parts", "return_reason", "TEXT").await?;
        add_column(manager, "parts", "order_ref", "TEXT").await?;
        add_column(
            manager,
            "parts",
            "returned_from_need_id",
            "INTEGER REFERENCES job_needs(id) ON DELETE SET NULL",
        )
        .await?;
        manager
            .create_index(index(
                "idx_parts_returned_from_need",
                "parts",
                &["returned_from_need_id"],
            ))
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite cannot drop a column named in an FK clause, so `down` removes
        // only the new tables; the added columns stay and are inert.
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
