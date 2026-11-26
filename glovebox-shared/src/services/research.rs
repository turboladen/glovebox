use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{
        research_finding::{self, FindingCategory, FindingSeverity, FindingStatus},
        research_report::{self, ReportType},
    },
    error::{DomainError, DomainResult},
    services::{
        nhtsa::{self, RecallInfo},
        recall::{self, RecallWithStatus},
    },
};

#[derive(Debug)]
struct NewRecall {
    category: FindingCategory,
    title: String,
    description: Option<String>,
    source_url: Option<String>,
    severity: Option<FindingSeverity>,
}

// --- Recall check ---

/// What a recall check found: the count of recalls stored for the first time
/// and every recall known for the vehicle with its status.
#[derive(Debug, Serialize)]
pub struct RecallCheck {
    pub newly_found: usize,
    pub recalls: Vec<RecallWithStatus>,
    pub message: Option<String>,
    /// The model line NHTSA was asked about, when it does not list the
    /// vehicle's own model. Its recalls cover every car of that line, so one
    /// may not apply to this vehicle.
    pub searched_model: Option<String>,
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

    match lookup(make, model.clone(), year).await {
        Ok(result) => {
            let own: Vec<&str> = model.split_whitespace().collect();
            let line = (!result.model.eq_ignore_ascii_case(&own.join(" "))).then_some(result.model);
            let mut check =
                record_recalls_under(db, vehicle_id, &result.recalls, line.as_deref()).await?;
            if let Some(line) = line {
                if check.recalls.is_empty() {
                    check.message = Some(format!(
                        "NHTSA does not list the {model}; it lists no recalls for the {line} line \
                         of this make and year."
                    ));
                }
                check.searched_model = Some(line);
            }
            Ok(check)
        }
        Err(e) => {
            tracing::warn!(vehicle_id, error = %e, "NHTSA recall lookup failed");
            Ok(RecallCheck {
                newly_found: 0,
                recalls: recall::list(db, vehicle_id).await?,
                message: Some(format!(
                    "NHTSA lookup failed: {e}. The recalls listed are the ones already known; try \
                     again later."
                )),
                searched_model: None,
            })
        }
    }
}

/// [`record_recalls_under`] for recalls NHTSA listed under the vehicle's own
/// model.
#[cfg(test)]
pub(crate) async fn record_recalls<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    found: &[RecallInfo],
) -> DomainResult<RecallCheck> {
    record_recalls_under(db, vehicle_id, found, None).await
}

/// Stores the recalls in `found` that this vehicle does not have yet and
/// lists every recall on the vehicle with its status. `line` is the model
/// line NHTSA listed them under when that is not the vehicle's own model, and
/// `None` otherwise. Every recall in `found` records it, a stored one
/// included, so a recall found again under the vehicle's own model loses it.
pub(crate) async fn record_recalls_under<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    found: &[RecallInfo],
    line: Option<&str>,
) -> DomainResult<RecallCheck> {
    let newly_found = persist_recall_findings(db, vehicle_id, found, line).await?;
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
        searched_model: None,
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
    line: Option<&str>,
) -> DomainResult<usize> {
    let txn = super::begin_write(db).await?;
    let db = &txn;
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
                .filter(research_finding::Column::Category.eq(FindingCategory::Recall))
                .all(db)
                .await?
                .into_iter()
                .filter_map(|f| f.source_url.clone().map(|u| (u, f)))
                .collect()
        };

    let mut new_findings: Vec<NewRecall> = Vec::new();
    let mut seen_urls: std::collections::HashSet<String> = std::collections::HashSet::new();
    for recall in recalls {
        let url = format!(
            "https://www.nhtsa.gov/recalls?nhtsaId={}",
            recall.campaign_number
        );
        if let Some(existing) = known.remove(&url) {
            // A campaign stored with a blank title, or with its raw NHTSA
            // component as the title, gets the current one. A campaign found
            // again takes the model line it was found under this time.
            let stored_raw = recall
                .component
                .as_deref()
                .is_some_and(|c| c.trim() == existing.title.trim());
            let retitle =
                (existing.title.trim().is_empty() || stored_raw) && !recall.title.trim().is_empty();
            let title = if retitle {
                &recall.title
            } else {
                &existing.title
            };
            if *title != existing.title || existing.searched_model.as_deref() != line {
                let title = title.clone();
                let mut active: research_finding::ActiveModel = existing.into();
                active.title = Set(title);
                active.searched_model = Set(line.map(str::to_owned));
                active.updated_at = Set(super::now());
                active.update(db).await?;
            }
            // Stops a campaign repeated within `recalls` from being stored twice.
            seen_urls.insert(url);
            continue;
        }
        if !seen_urls.insert(url.clone()) {
            continue;
        }
        new_findings.push(NewRecall {
            category: FindingCategory::Recall,
            title: recall.title.clone(),
            description: recall_description(recall),
            source_url: Some(url),
            severity: Some(FindingSeverity::Critical),
        });
    }

    if new_findings.is_empty() {
        txn.commit().await?;
        return Ok(0);
    }
    let count = new_findings.len();

    let report = research_report::ActiveModel {
        vehicle_id: Set(vehicle_id),
        report_type: Set(Some(ReportType::RecallsOnly)),
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
            status: Set(FindingStatus::New),
            searched_model: Set(line.map(str::to_owned)),
            ..Default::default()
        };
        finding.insert(&txn).await?;
    }

    txn.commit().await?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_db;

    use crate::test_support::seed_vehicle;

    // A 2017 Volkswagen Golf GTI, whose recalls NHTSA lists under the Golf
    // line.
    async fn golf_gti(db: &DatabaseConnection) -> i32 {
        crate::entities::vehicle::ActiveModel {
            name: Set("GTI".into()),
            make: Set(Some("Volkswagen".into())),
            model: Set(Some("Golf GTI".into())),
            year: Set(Some(2017)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    // A lookup that answers with `recalls` listed under `model`.
    fn listing(
        model: &'static str,
        recalls: Vec<RecallInfo>,
    ) -> impl FnOnce(String, String, i32) -> std::future::Ready<Result<nhtsa::RecallCheckResult, String>>
    {
        move |make, _, model_year| {
            std::future::ready(Ok(nhtsa::RecallCheckResult {
                make,
                model: model.to_owned(),
                model_year,
                recall_count: i32::try_from(recalls.len()).unwrap(),
                recalls,
            }))
        }
    }

    #[tokio::test]
    async fn a_failed_lookup_still_returns_the_known_recalls() {
        let db = test_db().await;
        let vid = golf_gti(&db).await;
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
        assert_eq!(
            check.recalls[0].status,
            crate::services::recall::ShownRecallStatus::Open
        );
        let message = check.message.unwrap_or_default();
        assert!(
            message.starts_with("NHTSA lookup failed: connection refused"),
            "got: {message}"
        );

        let missing = check_recalls(&db, 99_999).await.unwrap_err();
        assert_eq!(missing.to_string(), "Vehicle 99999 not found");
    }

    #[tokio::test]
    async fn a_lookup_by_the_model_line_names_the_line_it_asked_about() {
        let db = test_db().await;
        let vid = golf_gti(&db).await;
        let answer = |model| listing(model, Vec::new());

        let by_line = check_recalls_with(&db, vid, answer("Golf")).await.unwrap();
        assert_eq!(by_line.searched_model.as_deref(), Some("Golf"));
        assert_eq!(
            by_line.message.as_deref(),
            Some(
                "NHTSA does not list the Golf GTI; it lists no recalls for the Golf line of this \
                 make and year."
            )
        );
        // The tool description and the project prompt promise this key.
        assert_eq!(
            serde_json::to_value(&by_line).unwrap()["searched_model"],
            "Golf"
        );
        let by_model = check_recalls_with(&db, vid, answer("golf gti"))
            .await
            .unwrap();
        assert_eq!(by_model.searched_model, None);
    }

    #[tokio::test]
    async fn a_recall_records_the_line_it_was_last_found_under() {
        let db = test_db().await;
        let vid = golf_gti(&db).await;
        let answer = |model| listing(model, vec![full_recall("17V1", "Fuel pump")]);
        let line = |check: &RecallCheck| check.recalls[0].searched_model.clone();

        let by_line = check_recalls_with(&db, vid, answer("Golf")).await.unwrap();
        assert_eq!(line(&by_line).as_deref(), Some("Golf"));
        // A read that does not look the vehicle up shows the stored line.
        let listed = recall::list(&db, vid).await.unwrap();
        assert_eq!(listed[0].searched_model.as_deref(), Some("Golf"));
        // The vehicle's own model lists it now, so the line is cleared.
        let by_model = check_recalls_with(&db, vid, answer("Golf GTI"))
            .await
            .unwrap();
        assert_eq!(by_model.newly_found, 0);
        assert_eq!(line(&by_model), None);
        // A campaign stored without a line takes one when found under it.
        let again = check_recalls_with(&db, vid, answer("Golf")).await.unwrap();
        assert_eq!(line(&again).as_deref(), Some("Golf"));
        assert_eq!(again.recalls.len(), 1);
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
        recall::set_status(&db, vid, id, recall::RecallStatus::NotApplicable)
            .await
            .unwrap();

        let second = record_recalls(&db, vid, &[full_recall("17V1", "FUEL SYSTEM")])
            .await
            .unwrap();
        assert_eq!(second.newly_found, 0);
        assert_eq!(second.recalls.len(), 1);
        assert_eq!(second.recalls[0].finding_id, id);
        assert_eq!(second.recalls[0].title, "FUEL SYSTEM");
        assert_eq!(
            second.recalls[0].status,
            crate::services::recall::ShownRecallStatus::NotApplicable
        );

        let third = record_recalls(&db, vid, &[full_recall("17V1", "OTHER")])
            .await
            .unwrap();
        assert_eq!(third.recalls[0].title, "FUEL SYSTEM");
    }

    #[tokio::test]
    async fn checking_again_reformats_a_title_stored_in_the_raw_component_form() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let raw = "FUEL SYSTEM, GASOLINE:DELIVERY:FUEL PUMP";
        let with_component = |title: &str| RecallInfo {
            component: Some(raw.into()),
            ..full_recall("17V1", title)
        };
        record_recalls(&db, vid, &[with_component(raw)])
            .await
            .unwrap();

        let again = record_recalls(
            &db,
            vid,
            &[with_component("Fuel System, Gasoline: Delivery: Fuel Pump")],
        )
        .await
        .unwrap();
        assert_eq!(again.newly_found, 0);
        assert_eq!(
            again.recalls[0].title,
            "Fuel System, Gasoline: Delivery: Fuel Pump"
        );

        // A title someone chose is not the raw component, so it is kept.
        let id = again.recalls[0].finding_id;
        let mut active: research_finding::ActiveModel = research_finding::Entity::find_by_id(id)
            .one(&db)
            .await
            .unwrap()
            .unwrap()
            .into();
        active.title = Set("Fuel pump recall".into());
        active.update(&db).await.unwrap();
        let last = record_recalls(
            &db,
            vid,
            &[with_component("Fuel System, Gasoline: Delivery: Fuel Pump")],
        )
        .await
        .unwrap();
        assert_eq!(last.recalls[0].title, "Fuel pump recall");
    }
}
