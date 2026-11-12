use sea_orm::*;
use serde::{Deserialize, Serialize};

use crate::{
    entities::{research_finding, research_report},
    error::{DomainError, DomainResult},
    inputs::research::UpdateFinding,
    services::{
        nhtsa::{self, RecallInfo},
        recall::{self, RecallWithStatus},
    },
};

#[derive(Debug, Serialize)]
pub struct ReportWithFindings {
    #[serde(flatten)]
    pub report: research_report::Model,
    pub findings: Vec<research_finding::Model>,
}

#[derive(Debug, Serialize, Deserialize)]
struct NewFinding {
    category: String,
    title: String,
    description: Option<String>,
    source_url: Option<String>,
    severity: Option<String>,
}

/// Input for [`file_finding`] — a finding researched outside the app
/// (e.g. by an MCP client) to persist under the vehicle's
/// `external_research` anchor report.
#[derive(Debug)]
pub struct NewFiledFinding {
    pub category: String,
    pub title: String,
    pub description: Option<String>,
    pub source_url: Option<String>,
    pub severity: Option<String>,
}

// --- Recall check ---

/// What a recall check found: the count of recalls stored for the first time
/// and every recall known for the vehicle with its status.
#[derive(Debug, Serialize)]
pub struct RecallCheck {
    pub newly_found: usize,
    pub recalls: Vec<RecallWithStatus>,
    pub message: Option<String>,
}

/// Looks the vehicle up on NHTSA by make, model and year (a live web
/// request), stores recalls not seen before, and returns all known recalls
/// with their status.
///
/// A failed lookup is not an error: the result carries the recalls already
/// known, `newly_found: 0`, and a `message` that starts with
/// "NHTSA lookup failed:". A vehicle with no make, model or year is a
/// `BadRequest`.
pub async fn check_recalls<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
) -> DomainResult<RecallCheck> {
    check_recalls_with(db, vehicle_id, |make, model, year| async move {
        nhtsa::check_recalls(&make, &model, year).await
    })
    .await
}

/// [`check_recalls`] with the NHTSA lookup supplied by the caller, so a test
/// can stand in for the network.
pub(crate) async fn check_recalls_with<C, F, Fut>(
    db: &C,
    vehicle_id: i32,
    lookup: F,
) -> DomainResult<RecallCheck>
where
    C: ConnectionTrait + TransactionTrait,
    F: FnOnce(String, String, i32) -> Fut,
    Fut: std::future::Future<Output = Result<nhtsa::RecallCheckResult, String>>,
{
    let vehicle = crate::services::vehicle::require(db, vehicle_id).await?;

    let make = vehicle.make.ok_or_else(|| {
        DomainError::BadRequest("Vehicle has no make set — required for recall lookup".to_string())
    })?;
    let model = vehicle.model.ok_or_else(|| {
        DomainError::BadRequest("Vehicle has no model set — required for recall lookup".to_string())
    })?;
    let year = vehicle.year.ok_or_else(|| {
        DomainError::BadRequest("Vehicle has no year set — required for recall lookup".to_string())
    })?;

    match lookup(make, model, year).await {
        Ok(result) => record_recalls(db, vehicle_id, &result.recalls).await,
        Err(e) => {
            tracing::warn!(vehicle_id, error = %e, "NHTSA recall lookup failed");
            Ok(RecallCheck {
                newly_found: 0,
                recalls: recall::list(db, vehicle_id).await?,
                message: Some(format!(
                    "NHTSA lookup failed: {e}. The recalls listed are the ones already known; try \
                     again later."
                )),
            })
        }
    }
}

/// Stores the recalls in `found` that this vehicle does not have yet and
/// lists every recall on the vehicle with its status.
pub(crate) async fn record_recalls<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    found: &[RecallInfo],
) -> DomainResult<RecallCheck> {
    let newly_found = persist_recall_findings(db, vehicle_id, found).await?;
    let recalls = recall::list(db, vehicle_id).await?;
    let message = if recalls.is_empty() {
        Some("NHTSA lists no recalls for this make, model and year.".to_owned())
    } else if newly_found == 0 {
        Some("No new recalls; every recall below was already known.".to_owned())
    } else {
        None
    };
    Ok(RecallCheck {
        newly_found,
        recalls,
        message,
    })
}

/// Persist NHTSA recalls as research findings, skipping any already saved for
/// this vehicle. Returns how many were stored.
async fn persist_recall_findings<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    recalls: &[RecallInfo],
) -> DomainResult<usize> {
    // Collect existing recall source_urls for this vehicle to deduplicate
    let existing_reports = research_report::Entity::find()
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .all(db)
        .await?;
    let report_ids: Vec<i32> = existing_reports.iter().map(|r| r.id).collect();

    let mut known_urls: std::collections::HashSet<String> = if report_ids.is_empty() {
        std::collections::HashSet::new()
    } else {
        research_finding::Entity::find()
            .filter(research_finding::Column::ReportId.is_in(report_ids))
            .filter(research_finding::Column::Category.eq("recall"))
            .all(db)
            .await?
            .into_iter()
            .filter_map(|f| f.source_url)
            .collect()
    };

    let mut new_findings: Vec<NewFinding> = Vec::new();
    for recall in recalls {
        let url = format!(
            "https://www.nhtsa.gov/recalls?nhtsaId={}",
            recall.campaign_number
        );
        // `insert` also drops a campaign repeated within `recalls`.
        if !known_urls.insert(url.clone()) {
            continue;
        }
        new_findings.push(NewFinding {
            category: "recall".to_string(),
            title: recall.subject.clone(),
            description: recall.summary.clone(),
            source_url: Some(url),
            severity: Some("critical".to_string()),
        });
    }

    if new_findings.is_empty() {
        return Ok(0);
    }
    let count = new_findings.len();

    let summary = format!(
        "Found {} new recall{} from NHTSA.",
        count,
        if count == 1 { "" } else { "s" }
    );
    let raw_data = serde_json::to_string(&new_findings).ok();

    let txn = db.begin().await?;

    let report = research_report::ActiveModel {
        vehicle_id: Set(vehicle_id),
        report_type: Set(Some("recalls_only".to_string())),
        summary: Set(Some(summary)),
        raw_data: Set(raw_data),
        ..Default::default()
    };
    let report = report.insert(&txn).await?;

    for f in new_findings {
        let finding = research_finding::ActiveModel {
            report_id: Set(report.id),
            category: Set(f.category),
            title: Set(f.title),
            description: Set(f.description),
            source_url: Set(f.source_url),
            severity: Set(f.severity),
            status: Set("new".to_string()),
            ..Default::default()
        };
        finding.insert(&txn).await?;
    }

    txn.commit().await?;
    Ok(count)
}

// --- Research reports ---

pub async fn list_reports(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<Vec<research_report::Model>> {
    crate::services::vehicle::require(db, vehicle_id).await?;

    Ok(research_report::Entity::find()
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(research_report::Column::GeneratedAt)
        .all(db)
        .await?)
}

pub async fn get_report(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<ReportWithFindings> {
    let report = research_report::Entity::find_by_id(id)
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound("Research report not found".to_string()))?;

    let findings = research_finding::Entity::find()
        .filter(research_finding::Column::ReportId.eq(id))
        .order_by_asc(research_finding::Column::Id)
        .all(db)
        .await?;

    Ok(ReportWithFindings { report, findings })
}

// --- List findings by status (cross-report) ---

pub async fn list_findings(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    status: Option<String>,
) -> DomainResult<Vec<research_finding::Model>> {
    // Get all report IDs for this vehicle
    let report_ids: Vec<i32> = research_report::Entity::find()
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .all(db)
        .await?
        .into_iter()
        .map(|r| r.id)
        .collect();

    if report_ids.is_empty() {
        return Ok(vec![]);
    }

    let mut select = research_finding::Entity::find()
        .filter(research_finding::Column::ReportId.is_in(report_ids));

    if let Some(status) = status {
        select = select.filter(research_finding::Column::Status.eq(status));
    }

    Ok(select
        .order_by_desc(research_finding::Column::Id)
        .all(db)
        .await?)
}

// --- Finding management ---

pub async fn update_finding(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    report_id: i32,
    id: i32,
    input: UpdateFinding,
) -> DomainResult<research_finding::Model> {
    // Verify the report belongs to the vehicle
    research_report::Entity::find_by_id(report_id)
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound("Research report not found".to_string()))?;

    let finding = research_finding::Entity::find_by_id(id)
        .filter(research_finding::Column::ReportId.eq(report_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound("Finding not found".to_string()))?;

    let mut active: research_finding::ActiveModel = finding.into();

    if let Some(status) = input.status {
        let valid = ["new", "dismissed", "planned", "completed"];
        if !valid.contains(&status.as_str()) {
            return Err(DomainError::BadRequest(format!(
                "Invalid status '{}'. Must be one of: {}",
                status,
                valid.join(", ")
            )));
        }
        active.status = Set(status);
    }

    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

    Ok(active.update(db).await?)
}

/// Persist an externally-researched finding (e.g. filed by an MCP client)
/// under this vehicle's `external_research` anchor report — created on first
/// use, reused thereafter (mirrors how recalls anchor to `recalls_only`).
pub async fn file_finding(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    input: NewFiledFinding,
) -> DomainResult<research_finding::Model> {
    crate::services::vehicle::require(db, vehicle_id).await?;

    // order_by_asc(Id): should concurrent first-filings ever race-create two
    // anchors (no unique index on vehicle+type), every later call still picks
    // the same one deterministically.
    let report = match research_report::Entity::find()
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .filter(research_report::Column::ReportType.eq("external_research"))
        .order_by_asc(research_report::Column::Id)
        .one(db)
        .await?
    {
        Some(r) => {
            // Touch generated_at so the anchor sorts by its NEWEST filing in
            // list_reports (order_by_desc(GeneratedAt)) instead of sinking
            // below old recall reports with a stale first-filing date.
            let mut active: research_report::ActiveModel = r.into();
            active.generated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
            active.update(db).await?
        }
        None => {
            research_report::ActiveModel {
                vehicle_id: Set(vehicle_id),
                report_type: Set(Some("external_research".to_string())),
                summary: Set(Some("Findings filed via MCP".to_string())),
                ..Default::default()
            }
            .insert(db)
            .await?
        }
    };

    Ok(research_finding::ActiveModel {
        report_id: Set(report.id),
        category: Set(input.category),
        title: Set(input.title),
        description: Set(input.description),
        source_url: Set(input.source_url),
        severity: Set(input.severity),
        status: Set("new".to_string()),
        ..Default::default()
    }
    .insert(db)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{VehicleFixture, test_db};

    async fn seed_vehicle(db: &impl ConnectionTrait) -> i32 {
        VehicleFixture::new().insert_id(db).await
    }

    #[tokio::test]
    async fn a_failed_lookup_still_returns_the_known_recalls() {
        let db = test_db().await;
        let vid = crate::entities::vehicle::ActiveModel {
            name: Set("GTI".into()),
            make: Set(Some("Volkswagen".into())),
            model: Set(Some("Golf GTI".into())),
            year: Set(Some(2017)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        let known = RecallInfo {
            campaign_number: "24V110000".into(),
            manufacturer: None,
            subject: "Suction jet pump".into(),
            summary: None,
            consequence: None,
            remedy: None,
            report_date: None,
            component: Some("FUEL SYSTEM".into()),
            action_number: None,
        };
        record_recalls(&db, vid, &[known]).await.unwrap();

        let check = check_recalls_with(&db, vid, |_, _, _| async {
            Err::<nhtsa::RecallCheckResult, _>("connection refused".to_string())
        })
        .await
        .unwrap();
        assert_eq!(check.newly_found, 0);
        assert_eq!(check.recalls.len(), 1);
        assert_eq!(check.recalls[0].status, "open");
        let message = check.message.unwrap_or_default();
        assert!(
            message.starts_with("NHTSA lookup failed: connection refused"),
            "got: {message}"
        );

        let missing = check_recalls(&db, 99_999).await.unwrap_err();
        assert_eq!(missing.to_string(), "Vehicle 99999 not found");
    }

    #[tokio::test]
    async fn file_finding_creates_and_reuses_external_research_report() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let f1 = file_finding(
            &db,
            vid,
            NewFiledFinding {
                category: "maintenance".into(),
                title: "DSG service interval is 40k, not 60k".into(),
                description: Some("Per community consensus for this gearbox".into()),
                source_url: Some("https://example.com/thread".into()),
                severity: Some("info".into()),
            },
        )
        .await
        .unwrap();
        let f2 = file_finding(
            &db,
            vid,
            NewFiledFinding {
                category: "recall".into(),
                title: "Second finding".into(),
                description: None,
                source_url: None,
                severity: None,
            },
        )
        .await
        .unwrap();
        // Same anchor report, created once, type external_research.
        assert_eq!(f1.report_id, f2.report_id);
        let report = get_report(&db, vid, f1.report_id).await.unwrap();
        assert_eq!(
            report.report.report_type.as_deref(),
            Some("external_research")
        );
        assert_eq!(report.findings.len(), 2);
        // Another vehicle gets its own anchor, never vehicle A's, which is
        // what a lookup missing its VehicleId filter would return.
        let other = seed_vehicle(&db).await;
        let f3 = file_finding(
            &db,
            other,
            NewFiledFinding {
                category: "forum_report".into(),
                title: "Other car's finding".into(),
                description: None,
                source_url: None,
                severity: None,
            },
        )
        .await
        .unwrap();
        assert_ne!(f3.report_id, f1.report_id);

        // Missing vehicle is indistinguishable from nonexistent.
        assert!(matches!(
            file_finding(
                &db,
                999,
                NewFiledFinding {
                    category: "note".into(),
                    title: "x".into(),
                    description: None,
                    source_url: None,
                    severity: None,
                }
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn update_finding_validates_status_and_persists() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        // Manually create a report + finding to update
        let report = research_report::ActiveModel {
            vehicle_id: Set(vid),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let finding = research_finding::ActiveModel {
            report_id: Set(report.id),
            category: Set("recall".into()),
            title: Set("Fix".into()),
            status: Set("new".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        // Invalid status is rejected
        let err = update_finding(
            &db,
            vid,
            report.id,
            finding.id,
            UpdateFinding {
                status: Some("bogus".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)));

        // Valid status is applied
        let updated = update_finding(
            &db,
            vid,
            report.id,
            finding.id,
            UpdateFinding {
                status: Some("dismissed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.status, "dismissed");
    }

    #[tokio::test]
    async fn list_reports_missing_vehicle_is_not_found() {
        let db = test_db().await;
        assert!(matches!(
            list_reports(&db, 999).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn update_finding_sets_status_and_rejects_unknown_values() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let report = research_report::ActiveModel {
            vehicle_id: Set(vid),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let finding = research_finding::ActiveModel {
            report_id: Set(report.id),
            category: Set("recall".into()),
            title: Set("Fix".into()),
            status: Set("new".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let dismissed = update_finding(
            &db,
            vid,
            report.id,
            finding.id,
            UpdateFinding {
                status: Some("dismissed".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(dismissed.status, "dismissed");

        let err = update_finding(
            &db,
            vid,
            report.id,
            finding.id,
            UpdateFinding {
                status: Some("bogus".into()),
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)));
    }
}
