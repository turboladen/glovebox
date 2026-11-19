//! Adds CHECK constraints that keep a need's chosen option, decision date and
//! decision reason together, and an option's rejection and kept choice in
//! pairs. It also makes the chosen option's key `ON DELETE RESTRICT`, so
//! deleting an option a need has chosen fails.
//!
//! It refuses to finish, and changes nothing, while a stored row breaks one
//! of the rules. Running it again finds everything in place and edits nothing.

use sea_orm_migration::prelude::*;

use super::schema::{Check, add_checks, edit_definitions, scalar};

// The rules as they were written for this migration. They stay as they are,
// because a later migration that rewrites the rows depends on them.
const CHECKS: &[Check] = &[
    Check {
        table: "job_needs",
        name: "job_needs_choice_dated",
        holds: "(chosen_option_id IS NULL) = (decided_on IS NULL)",
    },
    Check {
        table: "job_needs",
        name: "job_needs_reason_held",
        holds: "decision_reason IS NULL OR chosen_option_id IS NOT NULL OR supplied_by = 'shop'",
    },
    Check {
        table: "need_options",
        name: "need_options_rejected_pair",
        holds: "(rejected_reason IS NULL) = (rejected_on IS NULL)",
    },
    Check {
        table: "need_options",
        name: "need_options_kept_choice",
        holds: "chosen_reason IS NULL OR chosen_on IS NOT NULL",
    },
];

const SET_NULL: &str =
    r#"FOREIGN KEY ("chosen_option_id") REFERENCES "need_options" ("id") ON DELETE SET NULL"#;
const RESTRICT: &str =
    r#"FOREIGN KEY ("chosen_option_id") REFERENCES "need_options" ("id") ON DELETE RESTRICT"#;

async fn on_delete(db: &impl ConnectionTrait) -> Result<String, DbErr> {
    scalar(
        db,
        "SELECT on_delete FROM pragma_foreign_key_list('job_needs') WHERE \"from\" = \
         'chosen_option_id'",
    )
    .await
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        const WHAT: &str = "adding the decision checks of needs";
        // One transaction pins one connection, which `PRAGMA writable_schema`
        // needs, holds the write lock from its start and leaves nothing half
        // done when a step fails.
        let txn = crate::services::begin_write(manager.get_connection()).await?;
        add_checks(&txn, WHAT, CHECKS).await?;
        if on_delete(&txn).await? != "RESTRICT" {
            edit_definitions(&txn, WHAT, async |txn| {
                txn.execute_unprepared(&format!(
                    "UPDATE sqlite_schema SET sql = replace(sql, '{SET_NULL}', '{RESTRICT}') \
                     WHERE type = 'table' AND name = 'job_needs'"
                ))
                .await?;
                Ok(true)
            })
            .await?;
            let now = on_delete(&txn).await?;
            if now != "RESTRICT" {
                return Err(DbErr::Migration(format!(
                    "{WHAT} did not finish: the chosen option's key still reads ON DELETE {now}. \
                     Nothing was changed."
                )));
            }
        }
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Nothing is undone: no code depends on the rules being absent.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection};
    use sea_orm_migration::MigratorTrait;

    use crate::migration::{
        Migrator,
        schema::testing::{count, fts_integrity, migrated_to, text},
    };

    // The number of migrations that run before this one.
    const BEFORE: u32 = 12;

    const STAMP: &str = "'2026-11-19T00:00:00+00:00'";

    // A database as the previous migration leaves it: a job with a need that
    // chose an option, a shop need with a reason, a rejected option and one
    // that kept an earlier choice.
    async fn db_before() -> DatabaseConnection {
        let db = migrated_to(BEFORE).await;
        db.execute_unprepared(&format!(
            "INSERT INTO vehicles (id, name, created_at, updated_at)
                 VALUES (1, 'Golf', {STAMP}, {STAMP});
             INSERT INTO work_items (id, vehicle_id, title, created_at, updated_at)
                 VALUES (1, 1, 'Brakes', {STAMP}, {STAMP});
             INSERT INTO job_needs (id, work_item_id, name, created_at, updated_at)
                 VALUES (1, 1, 'Pads', {STAMP}, {STAMP});
             INSERT INTO job_needs (id, work_item_id, name, supplied_by, decision_reason,
                     created_at, updated_at)
                 VALUES (2, 1, 'Fluid', 'shop', 'OEM only', {STAMP}, {STAMP});
             INSERT INTO need_options (id, need_id, description, created_at, updated_at)
                 VALUES (1, 1, 'Akebono', {STAMP}, {STAMP});
             INSERT INTO need_options (id, need_id, description, rejected_reason, rejected_on,
                     chosen_reason, chosen_on, created_at, updated_at)
                 VALUES (2, 1, 'Cheap', 'squeals', '2026-11-02', 'cheap', '2026-11-01', {STAMP}, \
             {STAMP});
             UPDATE job_needs SET chosen_option_id = 1, decided_on = '2026-11-03',
                 decision_reason = 'quiet' WHERE id = 1;"
        ))
        .await
        .unwrap();
        db
    }

    async fn definitions(db: &DatabaseConnection) -> String {
        text(
            db,
            "SELECT group_concat(sql, ';') FROM (SELECT sql FROM sqlite_schema WHERE name IN \
             ('job_needs', 'need_options') ORDER BY name)",
        )
        .await
    }

    // Everything in `sqlite_schema` that belongs to the two tables but their
    // own rows.
    async fn surroundings(db: &DatabaseConnection) -> String {
        text(
            db,
            "SELECT group_concat(sql, ';') FROM (SELECT sql FROM sqlite_schema WHERE tbl_name IN \
             ('job_needs', 'need_options') AND type <> 'table' ORDER BY name)",
        )
        .await
    }

    async fn rows(db: &DatabaseConnection) -> String {
        text(
            db,
            "SELECT group_concat(r, ',') FROM (SELECT id || ifnull(chosen_option_id, '-') || \
             ifnull(decided_on, '-') || ifnull(decision_reason, '-') AS r FROM job_needs UNION \
             ALL SELECT id || ifnull(rejected_on, '-') || ifnull(chosen_on, '-') FROM \
             need_options)",
        )
        .await
    }

    #[tokio::test]
    async fn the_tables_keep_their_rows_and_gain_the_checks_and_the_key() {
        let db = db_before().await;
        let before = rows(&db).await;
        let around = surroundings(&db).await;

        Migrator::up(&db, Some(1)).await.unwrap();

        let defs = definitions(&db).await;
        for name in [
            "job_needs_choice_dated",
            "job_needs_reason_held",
            "need_options_rejected_pair",
            "need_options_kept_choice",
        ] {
            assert!(
                defs.contains(&format!("CONSTRAINT \"{name}\" CHECK")),
                "{defs}"
            );
        }
        assert_eq!(super::on_delete(&db).await.unwrap(), "RESTRICT");
        assert_eq!(rows(&db).await, before);
        assert_eq!(surroundings(&db).await, around);
        fts_integrity(&db, &["fts_work_items"]).await;
    }

    #[tokio::test]
    async fn a_write_that_breaks_a_rule_is_refused_afterwards() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();
        for (rule, sql) in [
            (
                "job_needs_choice_dated",
                "UPDATE job_needs SET decided_on = NULL WHERE id = 1",
            ),
            (
                "job_needs_choice_dated",
                "UPDATE job_needs SET decided_on = '2026-11-03' WHERE id = 2",
            ),
            (
                "job_needs_reason_held",
                "UPDATE job_needs SET supplied_by = 'self' WHERE id = 2",
            ),
            (
                "need_options_rejected_pair",
                "UPDATE need_options SET rejected_on = NULL WHERE id = 2",
            ),
            (
                "need_options_kept_choice",
                "UPDATE need_options SET chosen_on = NULL WHERE id = 2",
            ),
            ("FOREIGN KEY", "DELETE FROM need_options WHERE id = 1"),
        ] {
            let err = db.execute_unprepared(sql).await.expect_err(sql).to_string();
            assert!(err.contains(rule), "{sql}: {err}");
        }
    }

    // The chosen option's key refuses a delete of the option alone, but not
    // one that takes its need along.
    #[tokio::test]
    async fn deleting_a_need_job_or_vehicle_still_takes_its_options() {
        for parent in [
            "DELETE FROM job_needs WHERE id = 1",
            "DELETE FROM work_items WHERE id = 1",
            "DELETE FROM vehicles WHERE id = 1",
        ] {
            let db = db_before().await;
            Migrator::up(&db, Some(1)).await.unwrap();
            db.execute_unprepared(parent).await.expect(parent);
            assert_eq!(
                count(&db, "SELECT count(*) FROM need_options WHERE need_id = 1").await,
                0,
                "{parent}"
            );
        }
    }

    #[tokio::test]
    async fn running_it_again_changes_nothing() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();
        let after = definitions(&db).await;
        Migrator::down(&db, Some(1)).await.unwrap();
        Migrator::up(&db, Some(1)).await.unwrap();
        assert_eq!(definitions(&db).await, after);
    }

    #[tokio::test]
    async fn a_row_that_breaks_a_rule_stops_it_before_anything_changes() {
        let db = db_before().await;
        db.execute_unprepared(
            "UPDATE job_needs SET supplied_by = 'self' WHERE id = 2;
             UPDATE need_options SET rejected_on = NULL WHERE id = 2;",
        )
        .await
        .unwrap();
        let before = definitions(&db).await;

        let err = Migrator::up(&db, Some(1)).await.unwrap_err().to_string();

        assert!(
            err.contains(
                "1 row of job_needs breaks job_needs_reason_held (id 2); 1 row of need_options \
                 breaks need_options_rejected_pair (id 2). Correct them, then start again. \
                 Nothing was changed."
            ),
            "{err}"
        );
        assert_eq!(definitions(&db).await, before);
        assert_eq!(super::on_delete(&db).await.unwrap(), "SET NULL");
    }
}
