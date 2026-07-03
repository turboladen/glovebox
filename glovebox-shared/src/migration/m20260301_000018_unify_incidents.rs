//! Unify observations + accidents into one `incidents` primitive (2hea unit B).
//!
//! Creates `incidents` (union schema: observation fields + nullable accident
//! fields + `category` + `recurrence_of_id` self-FK), `incident_followups`
//! (generalized from `accident_correspondence`), and `incident_service_links`
//! (M2M; subsumes both `accident_service_links` and
//! `observations.resolved_service_id`), copies all rows with a deterministic
//! id-offset mapping (observations keep their ids; accidents land at
//! `MAX(observations.id) + id`), rebuilds the FTS arms (incidents + followups
//! + the builds arm from skde), and drops the old tables.
//!
//! DML and FTS DDL use `execute_unprepared` raw SQL (virtual-table/trigger DDL
//! and cross-table INSERT..SELECT have no query-builder representation; the
//! `Expr::cust()` convention applies to expressions inside builder queries).

use sea_orm_migration::prelude::*;

/// One FTS5 external-content index over `content` table's `cols` — the exact
/// pattern from migration 000013 (`IF NOT EXISTS` throughout: `up()` is not
/// wrapped in a transaction, so a crash mid-migration must not wedge the
/// re-run).
struct FtsSpec {
    content: &'static str,
    fts: &'static str,
    cols: &'static [&'static str],
}

/// Old FTS arms whose content tables die here.
const OLD_FTS: &[(&str, &str)] = &[
    ("observations", "fts_observations"),
    ("accidents", "fts_accidents"),
    ("accident_correspondence", "fts_accident_correspondence"),
];

/// New FTS arms: the two incident tables plus builds (skde — builds were
/// never indexed).
const NEW_FTS: &[FtsSpec] = &[
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
        content: "builds",
        fts: "fts_builds",
        cols: &["name", "description"],
    },
];

#[derive(DeriveMigrationName)]
pub struct Migration;

#[allow(clippy::too_many_lines)]
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // --- incidents -------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(Incidents::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Incidents::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Incidents::VehicleId).integer().not_null())
                    .col(ColumnDef::new(Incidents::Category).text().not_null())
                    .col(ColumnDef::new(Incidents::Title).text().not_null())
                    .col(ColumnDef::new(Incidents::Description).text())
                    .col(ColumnDef::new(Incidents::Odometer).integer())
                    .col(
                        ColumnDef::new(Incidents::OccurredAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .col(ColumnDef::new(Incidents::ObdCodes).text())
                    .col(
                        ColumnDef::new(Incidents::Resolved)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(Incidents::Notes).text())
                    .col(ColumnDef::new(Incidents::Fault).text())
                    .col(ColumnDef::new(Incidents::OtherPartyName).text())
                    .col(ColumnDef::new(Incidents::OtherPartyPhone).text())
                    .col(ColumnDef::new(Incidents::OtherPartyEmail).text())
                    .col(ColumnDef::new(Incidents::OtherPartyInsurance).text())
                    .col(ColumnDef::new(Incidents::OtherPartyPolicyNumber).text())
                    .col(ColumnDef::new(Incidents::InsuranceClaimNumber).text())
                    .col(ColumnDef::new(Incidents::InsuranceAdjuster).text())
                    .col(ColumnDef::new(Incidents::InsuranceAdjusterPhone).text())
                    .col(ColumnDef::new(Incidents::TotalRepairCostCents).integer())
                    .col(ColumnDef::new(Incidents::TotalRepairCostCurrency).text())
                    .col(ColumnDef::new(Incidents::DeductibleCents).integer())
                    .col(ColumnDef::new(Incidents::DeductibleCurrency).text())
                    .col(ColumnDef::new(Incidents::InsurancePayoutCents).integer())
                    .col(ColumnDef::new(Incidents::InsurancePayoutCurrency).text())
                    .col(ColumnDef::new(Incidents::RecurrenceOfId).integer())
                    // Plain nullable INT like the other build links (000014):
                    // the service layer enforces build ownership.
                    .col(ColumnDef::new(Incidents::BuildId).integer())
                    .col(
                        ColumnDef::new(Incidents::CreatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .col(
                        ColumnDef::new(Incidents::UpdatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Incidents::Table, Incidents::VehicleId)
                            .to(Vehicles::Table, Vehicles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Incidents::Table, Incidents::RecurrenceOfId)
                            .to(Incidents::Table, Incidents::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_incidents_vehicle")
                    .table(Incidents::Table)
                    .col(Incidents::VehicleId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // --- incident_followups ----------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(IncidentFollowups::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(IncidentFollowups::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(IncidentFollowups::IncidentId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(IncidentFollowups::OccurredAt)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(IncidentFollowups::ContactMethod).text())
                    .col(ColumnDef::new(IncidentFollowups::ContactWith).text())
                    .col(ColumnDef::new(IncidentFollowups::Summary).text().not_null())
                    .col(ColumnDef::new(IncidentFollowups::Notes).text())
                    .col(
                        ColumnDef::new(IncidentFollowups::CreatedAt)
                            .text()
                            .not_null()
                            .default(Expr::cust("(datetime('now'))")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(IncidentFollowups::Table, IncidentFollowups::IncidentId)
                            .to(Incidents::Table, Incidents::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_incident_followups_incident")
                    .table(IncidentFollowups::Table)
                    .col(IncidentFollowups::IncidentId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // --- incident_service_links (mirrors accident_service_links) ----------
        manager
            .create_table(
                Table::create()
                    .table(IncidentServiceLinks::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(IncidentServiceLinks::IncidentId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(IncidentServiceLinks::ServiceRecordId)
                            .integer()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(IncidentServiceLinks::IncidentId)
                            .col(IncidentServiceLinks::ServiceRecordId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(
                                IncidentServiceLinks::Table,
                                IncidentServiceLinks::IncidentId,
                            )
                            .to(Incidents::Table, Incidents::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(
                                IncidentServiceLinks::Table,
                                IncidentServiceLinks::ServiceRecordId,
                            )
                            .to(ServiceRecords::Table, ServiceRecords::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        let db = manager.get_connection();

        // --- Data copy (deterministic id-offset mapping) ----------------------
        //
        // Observations keep their ids; accidents land at `offset + id` where
        // `offset = MAX(observations.id)` — inlined as a subselect so the
        // mapping is computed by SQLite itself, not in Rust.

        // 1. Observations copy, ids preserved.
        db.execute_unprepared(
            "INSERT INTO incidents (id, vehicle_id, category, title, description, odometer, \
             occurred_at, obd_codes, resolved, notes, build_id, created_at, updated_at) SELECT \
             id, vehicle_id, category, title, description, odometer, observed_at, obd_codes, \
             resolved, notes, build_id, created_at, updated_at FROM observations",
        )
        .await?;

        // 2+3. Accidents copy at `id + offset`, category 'accident', title =
        // first 100 chars of the NOT NULL description.
        db.execute_unprepared(
            "INSERT INTO incidents (id, vehicle_id, category, title, description, odometer, \
             occurred_at, resolved, notes, fault, other_party_name, other_party_phone, \
             other_party_email, other_party_insurance, other_party_policy_number, \
             insurance_claim_number, insurance_adjuster, insurance_adjuster_phone, \
             total_repair_cost_cents, total_repair_cost_currency, deductible_cents, \
             deductible_currency, insurance_payout_cents, insurance_payout_currency, created_at, \
             updated_at) SELECT id + (SELECT COALESCE(MAX(id), 0) FROM observations), vehicle_id, \
             'accident', substr(description, 1, 100), description, odometer, occurred_at, \
             resolved, notes, fault, other_party_name, other_party_phone, other_party_email, \
             other_party_insurance, other_party_policy_number, insurance_claim_number, \
             insurance_adjuster, insurance_adjuster_phone, total_repair_cost_cents, \
             total_repair_cost_currency, deductible_cents, deductible_currency, \
             insurance_payout_cents, insurance_payout_currency, created_at, updated_at FROM \
             accidents",
        )
        .await?;

        // 4. Correspondence -> followups (followup ids NOT preserved — fresh
        // autoincrement is fine, nothing references followup ids).
        db.execute_unprepared(
            "INSERT INTO incident_followups (incident_id, occurred_at, contact_method, \
             contact_with, summary, notes, created_at) SELECT accident_id + (SELECT \
             COALESCE(MAX(id), 0) FROM observations), occurred_at, contact_method, contact_with, \
             summary, notes, created_at FROM accident_correspondence",
        )
        .await?;

        // 5. Service links: accident M2M links shift by the offset; observation
        // `resolved_service_id` single-links convert to M2M rows. OR IGNORE on
        // the second insert in case a duplicate pair ever exists.
        db.execute_unprepared(
            "INSERT INTO incident_service_links (incident_id, service_record_id) SELECT \
             accident_id + (SELECT COALESCE(MAX(id), 0) FROM observations), service_record_id \
             FROM accident_service_links",
        )
        .await?;
        db.execute_unprepared(
            "INSERT OR IGNORE INTO incident_service_links (incident_id, service_record_id) SELECT \
             id, resolved_service_id FROM observations WHERE resolved_service_id IS NOT NULL",
        )
        .await?;

        // 6. Fix the AUTOINCREMENT sequence so fresh inserts continue past the
        // migrated ids. SQLite maintains sqlite_sequence on explicit-id inserts
        // into AUTOINCREMENT tables, so this is belt-and-braces; the guard uses
        // WHERE NOT EXISTS (not OR IGNORE — sqlite_sequence has no unique
        // constraint, so OR IGNORE could insert a duplicate 'incidents' row).
        db.execute_unprepared(
            "INSERT INTO sqlite_sequence (name, seq) SELECT 'incidents', 0 WHERE NOT EXISTS \
             (SELECT 1 FROM sqlite_sequence WHERE name = 'incidents')",
        )
        .await?;
        db.execute_unprepared(
            "UPDATE sqlite_sequence SET seq = (SELECT COALESCE(MAX(id), 0) FROM incidents) WHERE \
             name = 'incidents'",
        )
        .await?;

        // --- FTS rebuild -------------------------------------------------------
        // Drop the three retired arms (9 triggers + 3 virtual tables)…
        for (content, fts) in OLD_FTS {
            for suffix in ["ai", "ad", "au"] {
                db.execute_unprepared(&format!("DROP TRIGGER IF EXISTS {content}_fts_{suffix}"))
                    .await?;
            }
            db.execute_unprepared(&format!("DROP TABLE IF EXISTS {fts}"))
                .await?;
        }

        // …and create the new arms with the canonical external-content trigger
        // trio from 000013, then 'rebuild' to index the rows copied above.
        for spec in NEW_FTS {
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
                "CREATE TRIGGER IF NOT EXISTS {content}_fts_ai AFTER INSERT ON {content} BEGIN \
                 INSERT INTO {fts}(rowid, {cols}) VALUES (new.id, {new_vals}); END"
            ))
            .await?;

            db.execute_unprepared(&format!(
                "CREATE TRIGGER IF NOT EXISTS {content}_fts_ad AFTER DELETE ON {content} BEGIN \
                 INSERT INTO {fts}({fts}, rowid, {cols}) VALUES ('delete', old.id, {old_vals}); \
                 END"
            ))
            .await?;

            db.execute_unprepared(&format!(
                "CREATE TRIGGER IF NOT EXISTS {content}_fts_au AFTER UPDATE ON {content} BEGIN \
                 INSERT INTO {fts}({fts}, rowid, {cols}) VALUES ('delete', old.id, {old_vals}); \
                 INSERT INTO {fts}(rowid, {cols}) VALUES (new.id, {new_vals}); END"
            ))
            .await?;

            db.execute_unprepared(&format!("INSERT INTO {fts}({fts}) VALUES ('rebuild')"))
                .await?;
        }

        // --- Drop the old tables (children before parents) ---------------------
        for table in [
            "accident_correspondence",
            "accident_service_links",
            "accidents",
            "observations",
        ] {
            db.execute_unprepared(&format!("DROP TABLE IF EXISTS {table}"))
                .await?;
        }

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Intentionally irreversible: the observation/accident split is
        // retired, not versioned (house pattern from 000015/000016).
        Err(DbErr::Migration(
            "2hea unit B merges observations+accidents into incidents permanently; restore from a \
             DB backup instead"
                .into(),
        ))
    }
}

#[derive(DeriveIden)]
enum Incidents {
    Table,
    Id,
    VehicleId,
    Category,
    Title,
    Description,
    Odometer,
    OccurredAt,
    ObdCodes,
    Resolved,
    Notes,
    Fault,
    OtherPartyName,
    OtherPartyPhone,
    OtherPartyEmail,
    OtherPartyInsurance,
    OtherPartyPolicyNumber,
    InsuranceClaimNumber,
    InsuranceAdjuster,
    InsuranceAdjusterPhone,
    TotalRepairCostCents,
    TotalRepairCostCurrency,
    DeductibleCents,
    DeductibleCurrency,
    InsurancePayoutCents,
    InsurancePayoutCurrency,
    RecurrenceOfId,
    BuildId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum IncidentFollowups {
    Table,
    Id,
    IncidentId,
    OccurredAt,
    ContactMethod,
    ContactWith,
    Summary,
    Notes,
    CreatedAt,
}

#[derive(DeriveIden)]
enum IncidentServiceLinks {
    Table,
    IncidentId,
    ServiceRecordId,
}

#[derive(DeriveIden)]
enum Vehicles {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum ServiceRecords {
    Table,
    Id,
}
