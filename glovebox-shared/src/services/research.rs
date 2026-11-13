use sea_orm::*;
use serde::{Deserialize, Serialize};

use crate::{
    entities::{research_finding, research_report},
    error::{DomainError, DomainResult},
    services::{
        nhtsa::{self, RecallInfo},
        recall::{self, RecallWithStatus},
    },
};

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

/// The summary, consequence and remedy of a recall as one text, or `None`
/// when NHTSA sent none of them.
fn recall_description(recall: &RecallInfo) -> Option<String> {
    let parts: Vec<String> = [
        (None, &recall.summary),
        (Some("Consequence"), &recall.consequence),
        (Some("Remedy"), &recall.remedy),
    ]
    .into_iter()
    .filter_map(|(label, text)| {
        let text = text.as_deref()?.trim();
        (!text.is_empty())
            .then(|| label.map_or_else(|| text.to_owned(), |l| format!("{l}: {text}")))
    })
    .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
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

    // Maps each stored recall's source URL to its finding, so a repeated
    // campaign can be recognized and a blank title repaired.
    let mut known: std::collections::HashMap<String, research_finding::Model> =
        if report_ids.is_empty() {
            std::collections::HashMap::new()
        } else {
            research_finding::Entity::find()
                .filter(research_finding::Column::ReportId.is_in(report_ids))
                .filter(research_finding::Column::Category.eq("recall"))
                .all(db)
                .await?
                .into_iter()
                .filter_map(|f| f.source_url.clone().map(|u| (u, f)))
                .collect()
        };

    let mut new_findings: Vec<NewFinding> = Vec::new();
    let mut seen_urls: std::collections::HashSet<String> = std::collections::HashSet::new();
    for recall in recalls {
        let url = format!(
            "https://www.nhtsa.gov/recalls?nhtsaId={}",
            recall.campaign_number
        );
        if let Some(existing) = known.remove(&url) {
            // A campaign stored earlier with a blank title gets the current one.
            if existing.title.trim().is_empty() && !recall.title.trim().is_empty() {
                let mut active: research_finding::ActiveModel = existing.into();
                active.title = Set(recall.title.clone());
                active.updated_at = Set(super::now_stamp());
                active.update(db).await?;
            }
            // Stops a campaign repeated within `recalls` from being stored twice.
            seen_urls.insert(url);
            continue;
        }
        if !seen_urls.insert(url.clone()) {
            continue;
        }
        new_findings.push(NewFinding {
            category: "recall".to_string(),
            title: recall.title.clone(),
            description: recall_description(recall),
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
            // Touch generated_at so the anchor dates from its newest filing,
            // not a stale first-filing date.
            let mut active: research_report::ActiveModel = r.into();
            active.generated_at = Set(super::now_stamp());
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
            title: "Suction jet pump".into(),
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

    fn full_recall(campaign: &str, title: &str) -> RecallInfo {
        RecallInfo {
            campaign_number: campaign.into(),
            manufacturer: None,
            title: title.into(),
            summary: Some("Pump may fail.".into()),
            consequence: Some("Engine stall.".into()),
            remedy: Some("Replace the pump.".into()),
            report_date: None,
            component: None,
            action_number: None,
        }
    }

    #[tokio::test]
    async fn a_stored_recall_keeps_its_title_and_the_whole_notice_in_its_description() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let check = record_recalls(&db, vid, &[full_recall("17V1", "FUEL SYSTEM")])
            .await
            .unwrap();
        assert_eq!(check.recalls[0].title, "FUEL SYSTEM");
        let description = check.recalls[0].description.clone().unwrap();
        assert_eq!(
            description,
            "Pump may fail.\n\nConsequence: Engine stall.\n\nRemedy: Replace the pump."
        );
    }

    #[tokio::test]
    async fn checking_again_fills_in_a_blank_title_without_touching_status_or_count() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let first = record_recalls(&db, vid, &[full_recall("17V1", "")])
            .await
            .unwrap();
        let id = first.recalls[0].finding_id;
        recall::set_status(&db, vid, id, "not_applicable")
            .await
            .unwrap();

        let second = record_recalls(&db, vid, &[full_recall("17V1", "FUEL SYSTEM")])
            .await
            .unwrap();
        assert_eq!(second.newly_found, 0);
        assert_eq!(second.recalls.len(), 1);
        assert_eq!(second.recalls[0].finding_id, id);
        assert_eq!(second.recalls[0].title, "FUEL SYSTEM");
        assert_eq!(second.recalls[0].status, "not_applicable");

        let third = record_recalls(&db, vid, &[full_recall("17V1", "OTHER")])
            .await
            .unwrap();
        assert_eq!(third.recalls[0].title, "FUEL SYSTEM");
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
        let report = research_report::Entity::find_by_id(f1.report_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(report.report_type.as_deref(), Some("external_research"));
        let findings = research_finding::Entity::find()
            .filter(research_finding::Column::ReportId.eq(f1.report_id))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(findings, 2);
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
}
