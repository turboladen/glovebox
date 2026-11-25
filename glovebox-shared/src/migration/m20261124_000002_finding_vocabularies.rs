//! Holds a finding's category and severity and a report's type to their
//! vocabularies.
//!
//! A value that differs from a listed one only by ASCII case or surrounding
//! spaces is rewritten as the listed one, and a blank severity or report type
//! becomes NULL. The migration refuses to finish, and changes nothing, while
//! a column holds any other value outside its list. It then adds one CHECK
//! per column. Running it again finds the values and the constraints in place.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

use super::schema::{Check, add_checks};

/// A vocabulary column, the values it may hold and whether it may be NULL.
struct Listed {
    table: &'static str,
    column: &'static str,
    allowed: &'static [&'static str],
    nullable: bool,
}

// The lists are the values allowed when this migration was written, and they
// stay as they are. A value an enum gains later needs a migration that
// rewrites the column's CHECK.
const COLUMNS: &[Listed] = &[
    Listed {
        table: "research_findings",
        column: "category",
        allowed: &[
            "recall",
            "forum_report",
            "suggested_maintenance",
            "upgrade_idea",
            "known_issue",
            "service_bulletin",
            "other",
        ],
        nullable: false,
    },
    Listed {
        table: "research_findings",
        column: "severity",
        allowed: &["critical", "recommended", "optional", "informational"],
        nullable: true,
    },
    Listed {
        table: "research_reports",
        column: "report_type",
        allowed: &["recalls_only", "external_research"],
        nullable: true,
    },
];

// A CHECK passes on NULL, so each rule leaves a nullable column free to hold
// none. `has_check` finds a constraint by its name, so a rule changes only
// through a new migration.
const CHECKS: &[Check] = &[
    Check {
        table: "research_findings",
        name: "research_findings_category_listed",
        holds: "category IN ('recall', 'forum_report', 'suggested_maintenance', 'upgrade_idea', \
                'known_issue', 'service_bulletin', 'other')",
    },
    Check {
        table: "research_findings",
        name: "research_findings_severity_listed",
        holds: "severity IN ('critical', 'recommended', 'optional', 'informational')",
    },
    Check {
        table: "research_reports",
        name: "research_reports_report_type_listed",
        holds: "report_type IN ('recalls_only', 'external_research')",
    },
];

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // One transaction pins one connection, which `PRAGMA writable_schema`
        // needs, holds the write lock from its start and leaves nothing half
        // done when a step fails.
        let txn = crate::services::begin_write(manager.get_connection()).await?;
        let mut unreadable = Vec::new();
        for listed in COLUMNS {
            unreadable.extend(describe_unlisted(&txn, listed).await?);
        }
        if !unreadable.is_empty() {
            return Err(DbErr::Migration(format!(
                "these columns hold a value that is not in the column's list: {}. Correct the \
                 values, then start again. Nothing was changed.",
                unreadable.join("; ")
            )));
        }
        for listed in COLUMNS {
            normalize(&txn, listed).await?;
        }
        add_checks(&txn, "adding the finding vocabulary checks", CHECKS).await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Nothing is undone: no code depends on the constraints being absent,
        // and the spelling a value had before is not kept.
        Ok(())
    }
}

/// Describes the values of a column that are outside its list even after
/// their case and surrounding spaces are set aside, as `Some` text such as
/// `2 values in research_findings.category ('tip') (allowed: recall, other)`,
/// or `None` when there are none.
async fn describe_unlisted(
    db: &impl ConnectionTrait,
    listed: &Listed,
) -> Result<Option<String>, DbErr> {
    let Listed {
        table,
        column,
        allowed,
        nullable,
    } = listed;
    // SQLite judges the spelling here and rewrites it in `normalize`, so the
    // two agree on what a space and a capital letter are. The column is cast
    // because SQLite lets a TEXT column hold a number.
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!(
                "SELECT CAST({column} AS TEXT), lower(trim({column})), count(*) FROM {table} \
                 WHERE {column} IS NOT NULL GROUP BY 1 ORDER BY 1"
            ),
        ))
        .await?;
    let mut count = 0;
    let mut unlisted = Vec::new();
    for row in rows {
        let text = row.try_get_by_index::<String>(0)?;
        let plain = row.try_get_by_index::<String>(1)?;
        let blank = plain.is_empty() && *nullable;
        if !blank && !allowed.contains(&plain.as_str()) {
            count += row.try_get_by_index::<i64>(2)?;
            unlisted.push(format!("'{text}'"));
        }
    }
    let noun = if count == 1 { "value" } else { "values" };
    Ok((count > 0).then(|| {
        format!(
            "{count} {noun} in {table}.{column} ({}) (allowed: {})",
            unlisted.join(", "),
            allowed.join(", ")
        )
    }))
}

/// Rewrites each value of a column in lower case without surrounding spaces,
/// and a blank value of a nullable column as NULL. Only rows whose value
/// changes are written, so a second run writes none.
async fn normalize(db: &impl ConnectionTrait, listed: &Listed) -> Result<(), DbErr> {
    let Listed { table, column, .. } = listed;
    let plain = format!("lower(trim({column}))");
    if listed.nullable {
        let blank_to_null = Query::update()
            .table(Alias::new(*table))
            .value(Alias::new(*column), Option::<String>::None)
            .cond_where(Expr::cust(format!("trim({column}) = ''")))
            .to_owned();
        db.execute(&blank_to_null).await?;
    }
    let respell = Query::update()
        .table(Alias::new(*table))
        .value(Alias::new(*column), Expr::cust(plain.clone()))
        .cond_where(Expr::cust(format!("{column} <> {plain}")))
        .to_owned();
    db.execute(&respell).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection};
    use sea_orm_migration::MigratorTrait;

    use super::{CHECKS, COLUMNS};
    use crate::{
        entities::{
            research_finding::{FindingCategory, FindingSeverity},
            research_report::ReportType,
        },
        migration::{
            Migrator,
            schema::testing::{
                count, fts_integrity, migrated_to, surrounding_sql, table_sql, text,
            },
        },
        vocab::Vocabulary,
    };

    // The number of migrations that run before this one.
    const BEFORE: u32 = 21;

    const STAMP: &str = "'2026-11-24T00:00:00+00:00'";

    const TABLES: &[&str] = &["research_findings", "research_reports"];

    // A database as the previous migration leaves it: a recall report with a
    // recall, a hand-filed report whose values differ from the listed ones
    // by case and spaces, and a report and a finding that leave the nullable
    // columns empty or blank.
    async fn db_before() -> DatabaseConnection {
        let db = migrated_to(BEFORE).await;
        db.execute_unprepared(&format!(
            "INSERT INTO vehicles (id, name, created_at, updated_at)
                 VALUES (1, 'Golf', {STAMP}, {STAMP});
             INSERT INTO research_reports
                     (id, vehicle_id, report_type, generated_at, created_at)
                 VALUES (1, 1, 'recalls_only', {STAMP}, {STAMP}),
                        (2, 1, ' External_Research ', {STAMP}, {STAMP}),
                        (3, 1, NULL, {STAMP}, {STAMP}),
                        (4, 1, ' ', {STAMP}, {STAMP});
             INSERT INTO research_findings
                     (id, report_id, category, title, description, source_url, severity,
                      status, created_at, updated_at)
                 VALUES (1, 1, 'recall', 'Fuel pump', 'May fail', 'example.com/r',
                         'critical', 'dismissed', {STAMP}, {STAMP}),
                        (2, 2, ' Forum_Report', 'Water pump', NULL, NULL, 'Recommended ',
                         'new', {STAMP}, {STAMP}),
                        (5, 3, 'RECALL', 'Airbag', NULL, NULL, NULL, 'new', {STAMP},
                         {STAMP}),
                        (6, 4, 'other', 'Trim rattle', NULL, NULL, '', 'completed',
                         {STAMP}, {STAMP});"
        ))
        .await
        .unwrap();
        db
    }

    async fn findings(db: &DatabaseConnection) -> String {
        text(
            db,
            "SELECT group_concat(row, ';') FROM (SELECT id || ',' || report_id || ',' || \
             quote(category) || ',' || title || ',' || quote(description) || ',' || \
             quote(source_url) || ',' || quote(severity) || ',' || status || ',' || created_at || \
             ',' || updated_at AS row FROM research_findings ORDER BY id)",
        )
        .await
    }

    async fn reports(db: &DatabaseConnection) -> String {
        text(
            db,
            "SELECT group_concat(id || ',' || vehicle_id || ',' || quote(report_type), ';') FROM \
             (SELECT * FROM research_reports ORDER BY id)",
        )
        .await
    }

    async fn whole_schema(db: &DatabaseConnection) -> String {
        text(
            db,
            "SELECT group_concat(type || ' ' || name || ' ' || tbl_name || ' ' || ifnull(sql, \
             '-'), ';') FROM (SELECT * FROM sqlite_schema ORDER BY type, name)",
        )
        .await
    }

    #[tokio::test]
    async fn values_take_their_listed_spelling_and_every_row_keeps_the_rest() {
        let db = db_before().await;
        let around = surrounding_sql(&db, TABLES).await;

        Migrator::up(&db, Some(1)).await.unwrap();

        let stamps = format!("{0},{0}", STAMP.trim_matches('\''));
        let kept = [
            "1,1,'recall',Fuel pump,'May fail','example.com/r','critical',dismissed",
            "2,2,'forum_report',Water pump,NULL,NULL,'recommended',new",
            "5,3,'recall',Airbag,NULL,NULL,NULL,new",
            "6,4,'other',Trim rattle,NULL,NULL,NULL,completed",
        ]
        .map(|row| format!("{row},{stamps}"));
        assert_eq!(findings(&db).await, kept.join(";"));
        assert_eq!(
            reports(&db).await,
            "1,1,'recalls_only';2,1,'external_research';3,1,NULL;4,1,NULL"
        );
        let defs = table_sql(&db, TABLES).await;
        for check in CHECKS {
            assert!(
                defs.contains(&format!("CONSTRAINT \"{}\" CHECK", check.name)),
                "{defs}"
            );
        }
        assert_eq!(surrounding_sql(&db, TABLES).await, around);
        fts_integrity(&db, &["fts_research_findings"]).await;
        assert_eq!(
            count(
                &db,
                "SELECT count(*) FROM fts_research_findings WHERE fts_research_findings MATCH \
                 'pump'"
            )
            .await,
            2
        );
    }

    // Every stored row reads as its entity afterwards, which a value outside
    // an enum's list would stop.
    #[tokio::test]
    async fn every_row_reads_as_its_entity_afterwards() {
        use sea_orm::EntityTrait;

        use crate::entities::{research_finding, research_report};

        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();

        let findings = research_finding::Entity::find().all(&db).await.unwrap();
        let read: Vec<_> = findings.iter().map(|f| (f.category, f.severity)).collect();
        assert_eq!(
            read,
            vec![
                (FindingCategory::Recall, Some(FindingSeverity::Critical)),
                (
                    FindingCategory::ForumReport,
                    Some(FindingSeverity::Recommended)
                ),
                (FindingCategory::Recall, None),
                (FindingCategory::Other, None),
            ]
        );
        let reports = research_report::Entity::find().all(&db).await.unwrap();
        let read: Vec<_> = reports.iter().map(|r| r.report_type).collect();
        assert_eq!(
            read,
            vec![
                Some(ReportType::RecallsOnly),
                Some(ReportType::ExternalResearch),
                None,
                None
            ]
        );
    }

    #[tokio::test]
    async fn a_value_outside_the_list_stops_it_before_anything_changes() {
        for listed in COLUMNS {
            let (table, column) = (listed.table, listed.column);
            let db = db_before().await;
            db.execute_unprepared(&format!(
                "UPDATE {table} SET {column} = 'Bogus' WHERE id = 2"
            ))
            .await
            .unwrap();
            let (schema, rows) = (
                whole_schema(&db).await,
                (findings(&db).await, reports(&db).await),
            );

            let err = Migrator::up(&db, Some(1)).await.unwrap_err().to_string();

            assert!(
                err.contains(&format!("1 value in {table}.{column} ('Bogus') (allowed: "))
                    && err.contains("Nothing was changed."),
                "{err}"
            );
            assert_eq!(whole_schema(&db).await, schema);
            assert_eq!((findings(&db).await, reports(&db).await), rows);
        }
    }

    // A category is required, so a blank one is not a value the list holds.
    #[tokio::test]
    async fn a_blank_category_stops_it() {
        let db = db_before().await;
        db.execute_unprepared("UPDATE research_findings SET category = ' ' WHERE id = 6")
            .await
            .unwrap();

        let err = Migrator::up(&db, Some(1)).await.unwrap_err().to_string();

        assert!(
            err.contains("1 value in research_findings.category (' ')"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn a_write_that_breaks_a_rule_is_refused_afterwards() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();
        for (rule, sql) in [
            (
                "research_findings_category_listed",
                "UPDATE research_findings SET category = 'Recall' WHERE id = 1",
            ),
            (
                "research_findings_severity_listed",
                "UPDATE research_findings SET severity = 'info' WHERE id = 1",
            ),
            (
                "research_reports_report_type_listed",
                "UPDATE research_reports SET report_type = 'full' WHERE id = 1",
            ),
        ] {
            let err = db.execute_unprepared(sql).await.expect_err(sql).to_string();
            assert!(err.contains(rule), "{sql}: {err}");
        }
        db.execute_unprepared(
            "UPDATE research_findings SET severity = NULL WHERE id = 1;
             UPDATE research_reports SET report_type = NULL WHERE id = 1;",
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn running_it_again_changes_nothing() {
        let db = db_before().await;
        Migrator::up(&db, Some(1)).await.unwrap();
        let (schema, rows) = (
            whole_schema(&db).await,
            (findings(&db).await, reports(&db).await),
        );

        Migrator::down(&db, Some(1)).await.unwrap();
        Migrator::up(&db, Some(1)).await.unwrap();

        assert_eq!(whole_schema(&db).await, schema);
        assert_eq!((findings(&db).await, reports(&db).await), rows);
        fts_integrity(&db, &["fts_research_findings"]).await;
    }

    #[tokio::test]
    async fn a_fresh_database_ends_with_the_schema_of_a_migrated_one() {
        let migrated = db_before().await;
        Migrator::up(&migrated, Some(1)).await.unwrap();
        let fresh = migrated_to(BEFORE + 1).await;

        assert_eq!(whole_schema(&fresh).await, whole_schema(&migrated).await);
    }

    // A value added to one of these enums needs a migration that rewrites
    // its rule, so an enum, its guard list and its CHECK cannot part.
    #[test]
    fn each_rule_and_each_guard_list_holds_exactly_its_enums_values() {
        fn check<V: Vocabulary>(table: &str, column: &str) {
            let texts: Vec<&str> = V::ALL.iter().map(|v| v.as_str()).collect();
            let listed = COLUMNS
                .iter()
                .find(|l| (l.table, l.column) == (table, column))
                .unwrap_or_else(|| panic!("{table}.{column} must be guarded"));
            assert_eq!(listed.allowed, texts.as_slice(), "{table}.{column}");
            let quoted: Vec<String> = texts.iter().map(|t| format!("'{t}'")).collect();
            let rule = CHECKS
                .iter()
                .find(|c| c.name == format!("{table}_{column}_listed"))
                .unwrap_or_else(|| panic!("{table}.{column} must have a rule"));
            assert_eq!(rule.holds, format!("{column} IN ({})", quoted.join(", ")));
        }

        check::<FindingCategory>("research_findings", "category");
        check::<FindingSeverity>("research_findings", "severity");
        check::<ReportType>("research_reports", "report_type");
    }
}
