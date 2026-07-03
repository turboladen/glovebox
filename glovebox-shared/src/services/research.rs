use sea_orm::*;
use serde::{Deserialize, Serialize};

use crate::{
    entities::{part, research_finding, research_report, service_record, vehicle},
    error::{DomainError, DomainResult},
    inputs::research::UpdateFinding,
    services::nhtsa::{self, RecallCheckResult, RecallInfo},
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

pub async fn check_recalls<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
) -> DomainResult<RecallCheckResult> {
    let vehicle = vehicle::Entity::find_by_id(vehicle_id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound("Vehicle not found".to_string()))?;

    let make = vehicle.make.as_deref().ok_or_else(|| {
        DomainError::BadRequest("Vehicle has no make set — required for recall lookup".to_string())
    })?;
    let model = vehicle.model.as_deref().ok_or_else(|| {
        DomainError::BadRequest("Vehicle has no model set — required for recall lookup".to_string())
    })?;
    let year = vehicle.year.ok_or_else(|| {
        DomainError::BadRequest("Vehicle has no year set — required for recall lookup".to_string())
    })?;

    let result = nhtsa::check_recalls(make, model, year)
        .await
        .map_err(DomainError::Internal)?;

    // Persist recalls as research findings (deduplicating by campaign number)
    if !result.recalls.is_empty() {
        persist_recall_findings(db, vehicle_id, &result.recalls).await?;
    }

    Ok(result)
}

/// Persist NHTSA recalls as research findings, skipping any already saved for this vehicle.
async fn persist_recall_findings<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    recalls: &[RecallInfo],
) -> DomainResult<()> {
    // Collect existing recall source_urls for this vehicle to deduplicate
    let existing_reports = research_report::Entity::find()
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .all(db)
        .await?;
    let report_ids: Vec<i32> = existing_reports.iter().map(|r| r.id).collect();

    let existing_urls: std::collections::HashSet<String> = if report_ids.is_empty() {
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

    let new_findings: Vec<NewFinding> = recalls
        .iter()
        .filter(|recall| {
            let url = format!(
                "https://www.nhtsa.gov/recalls?nhtsaId={}",
                recall.campaign_number
            );
            !existing_urls.contains(&url)
        })
        .map(|recall| NewFinding {
            category: "recall".to_string(),
            title: recall.subject.clone(),
            description: recall.summary.clone(),
            source_url: Some(format!(
                "https://www.nhtsa.gov/recalls?nhtsaId={}",
                recall.campaign_number
            )),
            severity: Some("critical".to_string()),
        })
        .collect();

    if new_findings.is_empty() {
        return Ok(());
    }

    let summary = format!(
        "Found {} new recall{} from NHTSA.",
        new_findings.len(),
        if new_findings.len() == 1 { "" } else { "s" }
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
    Ok(())
}

// --- Research reports ---

pub async fn list_reports(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<Vec<research_report::Model>> {
    vehicle::Entity::find_by_id(vehicle_id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound("Vehicle not found".to_string()))?;

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

    // When this update SETS link fields, validate the effective post-update
    // pair: a linked id must point at one of THIS vehicle's records (same
    // invariant as observation.resolved_service_id / part.installed_service_id
    // — no storing pointers into another vehicle's data), and its type must be
    // a known kind. Unrelated updates (e.g. status-only) skip this so a link
    // whose target was later deleted can't block them.
    let sets_link = matches!(input.linked_entity_type, Some(Some(_)))
        || matches!(input.linked_entity_id, Some(Some(_)));
    let effective_type = match &input.linked_entity_type {
        Some(t) => t.clone(),
        None => finding.linked_entity_type.clone(),
    };
    let effective_id = match input.linked_entity_id {
        Some(v) => v,
        None => finding.linked_entity_id,
    };
    if let (true, Some(linked_id)) = (sets_link, effective_id) {
        match effective_type.as_deref() {
            Some("service") => {
                service_record::Entity::find_by_id(linked_id)
                    .filter(service_record::Column::VehicleId.eq(vehicle_id))
                    .one(db)
                    .await?
                    .ok_or_else(|| {
                        DomainError::NotFound(format!("Service record {linked_id} not found"))
                    })?;
            }
            Some("part") => {
                part::Entity::find_by_id(linked_id)
                    .filter(part::Column::VehicleId.eq(vehicle_id))
                    .one(db)
                    .await?
                    .ok_or_else(|| DomainError::NotFound(format!("Part {linked_id} not found")))?;
            }
            other => {
                return Err(DomainError::BadRequest(format!(
                    "Invalid linked_entity_type '{}'. Must be one of: service, part",
                    other.unwrap_or("")
                )));
            }
        }
    }

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
    if let Some(linked_type) = input.linked_entity_type {
        active.linked_entity_type = Set(linked_type);
    }
    if let Some(linked_id) = input.linked_entity_id {
        active.linked_entity_id = Set(linked_id);
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
    vehicle::Entity::find_by_id(vehicle_id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound("Vehicle not found".to_string()))?;

    let report = match research_report::Entity::find()
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .filter(research_report::Column::ReportType.eq("external_research"))
        .one(db)
        .await?
    {
        Some(r) => r,
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
    use crate::test_support::test_db;

    async fn seed_vehicle(db: &impl ConnectionTrait) -> i32 {
        vehicle::ActiveModel {
            name: Set("Car".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
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
    async fn update_finding_scopes_linked_entity_to_vehicle() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
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
        let own_svc = service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set("2026-01-01".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let foreign_svc = service_record::ActiveModel {
            vehicle_id: Set(other),
            service_date: Set("2026-01-01".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        // Another vehicle's service record is indistinguishable from nonexistent
        let err = update_finding(
            &db,
            vid,
            report.id,
            finding.id,
            UpdateFinding {
                linked_entity_type: Some(Some("service".into())),
                linked_entity_id: Some(Some(foreign_svc.id)),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)));

        // Unknown entity type is rejected outright
        let err = update_finding(
            &db,
            vid,
            report.id,
            finding.id,
            UpdateFinding {
                linked_entity_type: Some(Some("vehicle".into())),
                linked_entity_id: Some(Some(own_svc.id)),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)));

        // Linking this vehicle's own record works, and an explicit clear works
        let updated = update_finding(
            &db,
            vid,
            report.id,
            finding.id,
            UpdateFinding {
                status: Some("completed".into()),
                linked_entity_type: Some(Some("service".into())),
                linked_entity_id: Some(Some(own_svc.id)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.linked_entity_id, Some(own_svc.id));
        let cleared = update_finding(
            &db,
            vid,
            report.id,
            finding.id,
            UpdateFinding {
                linked_entity_type: Some(None),
                linked_entity_id: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.linked_entity_id, None);

        // A stale stored link (target deleted after linking) must NOT block
        // unrelated updates like a status-only change from the UI.
        update_finding(
            &db,
            vid,
            report.id,
            finding.id,
            UpdateFinding {
                linked_entity_type: Some(Some("service".into())),
                linked_entity_id: Some(Some(own_svc.id)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        service_record::Entity::delete_by_id(own_svc.id)
            .exec(&db)
            .await
            .unwrap();
        let dismissed = update_finding(
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
        assert_eq!(dismissed.status, "dismissed");
    }
}
