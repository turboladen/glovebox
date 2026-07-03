use sea_orm::*;
use serde::{Deserialize, Serialize};

use crate::{
    entities::{part, research_finding, research_report, service_record, vehicle},
    error::{DomainError, DomainResult},
    inputs::research::UpdateFinding,
    services::{
        ai::{self, AiRequest, ChatMessage, Role, context, registry::AiProviderRegistry},
        nhtsa::{self, RecallCheckResult, RecallInfo},
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

pub async fn generate<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    registry: &AiProviderRegistry,
    vehicle_id: i32,
    report_type: Option<String>,
    provider_id: Option<i32>,
) -> DomainResult<ReportWithFindings> {
    let vehicle = vehicle::Entity::find_by_id(vehicle_id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound("Vehicle not found".to_string()))?;

    let report_type = report_type.unwrap_or_else(|| "full_check".to_string());

    let mut all_findings: Vec<NewFinding> = Vec::new();

    // Check NHTSA recalls if vehicle has make/model/year
    if let (Some(make), Some(model), Some(year)) = (&vehicle.make, &vehicle.model, vehicle.year) {
        match nhtsa::check_recalls(make, model, year).await {
            Ok(result) => {
                for recall in &result.recalls {
                    all_findings.push(NewFinding {
                        category: "recall".to_string(),
                        title: recall.subject.clone(),
                        description: recall.summary.clone(),
                        source_url: Some(format!(
                            "https://www.nhtsa.gov/recalls?nhtsaId={}",
                            recall.campaign_number
                        )),
                        severity: Some("critical".to_string()),
                    });
                }
            }
            Err(e) => {
                tracing::warn!("NHTSA recall check failed: {}", e);
            }
        }
    }

    // If AI is configured and report type includes community wisdom, query AI
    if (report_type == "full_check" || report_type == "community_wisdom")
        && let Ok(provider) = registry.resolve(provider_id).await
    {
        let prompt = build_community_wisdom_prompt(&vehicle);
        match provider
            .complete(AiRequest {
                system_prompt: format!(
                    "{}\n\nReturn ONLY a valid JSON array — no narrative text, no markdown. \
                     Provide findings as a JSON array of objects with fields: title, description, \
                     severity (critical/recommended/optional/informational), category (one of: \
                     forum_report, suggested_maintenance, upgrade_idea).",
                    context::GLOVEBOX_PREAMBLE
                ),
                messages: vec![ChatMessage {
                    role: Role::User,
                    content: prompt,
                }],
                attachments: vec![],
                max_tokens: None,
            })
            .await
        {
            Ok(response) => {
                if let Ok(ai_findings) = parse_ai_findings(&response.content) {
                    all_findings.extend(ai_findings);
                }
            }
            Err(e) => {
                tracing::warn!("AI community wisdom query failed: {:?}", e);
            }
        }
    }

    let summary = if all_findings.is_empty() {
        "No findings. Vehicle appears to be in good standing.".to_string()
    } else {
        format!("Found {} items requiring attention.", all_findings.len())
    };

    let raw_data = serde_json::to_string(&all_findings).ok();

    // Save report and findings in a transaction
    let txn = db.begin().await?;

    let report = research_report::ActiveModel {
        vehicle_id: Set(vehicle_id),
        report_type: Set(Some(report_type)),
        summary: Set(Some(summary)),
        raw_data: Set(raw_data),
        ..Default::default()
    };
    let report = report.insert(&txn).await?;

    let mut saved_findings = Vec::new();
    for f in all_findings {
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
        saved_findings.push(finding.insert(&txn).await?);
    }

    txn.commit().await?;

    Ok(ReportWithFindings {
        report,
        findings: saved_findings,
    })
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

    // Validate the effective linked-entity pair AFTER applying this update: a
    // linked id must point at one of THIS vehicle's records (same invariant as
    // observation.resolved_service_id / part.installed_service_id — no storing
    // pointers into another vehicle's data), and its type must be a known kind.
    let effective_type = match &input.linked_entity_type {
        Some(t) => t.clone(),
        None => finding.linked_entity_type.clone(),
    };
    let effective_id = match input.linked_entity_id {
        Some(v) => v,
        None => finding.linked_entity_id,
    };
    if let Some(linked_id) = effective_id {
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

// --- Helpers ---

fn build_community_wisdom_prompt(vehicle: &vehicle::Model) -> String {
    let year_str = vehicle
        .year
        .map_or_else(|| "unknown year".to_string(), |y| y.to_string());
    let mut prompt = String::from("For a ");
    prompt.push_str(&year_str);
    prompt.push(' ');
    if let Some(ref make) = vehicle.make {
        prompt.push_str(make);
        prompt.push(' ');
    }
    if let Some(ref model) = vehicle.model {
        prompt.push_str(model);
        prompt.push(' ');
    }
    if let Some(ref trim) = vehicle.trim_level {
        prompt.push_str(trim);
        prompt.push(' ');
    }
    if let Some(ref drivetrain) = vehicle.drivetrain {
        prompt.push_str(drivetrain);
        prompt.push(' ');
    }
    if let Some(ref body_style) = vehicle.body_style {
        prompt.push_str(body_style);
        prompt.push(' ');
    }
    if let Some(ref engine) = vehicle.engine {
        prompt.push_str("with ");
        prompt.push_str(engine);
        prompt.push_str(" engine ");
    }
    if let Some(ref transmission) = vehicle.transmission {
        prompt.push_str("and ");
        prompt.push_str(transmission);
        prompt.push_str(" transmission ");
    }
    prompt.push_str(
        "— what are the most common issues, recommended preventive maintenance items beyond \
         factory schedule, and popular upgrades reported by owners? Only include issues that \
         apply to this exact configuration (e.g., do not include AWD-specific issues for a FWD \
         vehicle, or manual transmission issues for an automatic). Return as JSON array.",
    );
    prompt
}

fn parse_ai_findings(content: &str) -> Result<Vec<NewFinding>, serde_json::Error> {
    let cleaned = ai::strip_code_fences(content);
    serde_json::from_str(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_db;

    fn full_vehicle() -> vehicle::Model {
        vehicle::Model {
            id: 1,
            model_template_id: None,
            name: "Test".to_string(),
            year: Some(2017),
            make: Some("Volkswagen".to_string()),
            model: Some("Golf GTI".to_string()),
            trim_level: Some("SE".to_string()),
            body_style: Some("Hatchback".to_string()),
            engine: Some("2.0L TSI".to_string()),
            transmission: Some("6-speed DSG".to_string()),
            drivetrain: Some("FWD".to_string()),
            vin: None,
            license_plate: None,
            color: None,
            purchase_date: None,
            purchase_price_cents: None,
            purchase_price_currency: None,
            purchase_mileage: None,
            sold_date: None,
            sold_price_cents: None,
            sold_price_currency: None,
            sold_mileage: None,
            photo_path: None,
            notes: None,
            created_at: String::new(),
            updated_at: String::new(),
            archived_at: None,
        }
    }

    #[test]
    fn build_prompt_full_vehicle() {
        let prompt = build_community_wisdom_prompt(&full_vehicle());
        assert!(prompt.contains("2017"));
        assert!(prompt.contains("Volkswagen"));
        assert!(prompt.contains("Golf GTI"));
        assert!(prompt.contains("SE"));
        assert!(prompt.contains("FWD"));
        assert!(prompt.contains("Hatchback"));
        assert!(prompt.contains("2.0L TSI"));
        assert!(prompt.contains("6-speed DSG"));
        assert!(prompt.contains("exact configuration"));
    }

    #[test]
    fn build_prompt_minimal_vehicle() {
        let mut v = full_vehicle();
        v.year = None;
        v.make = None;
        v.model = None;
        v.trim_level = None;
        v.body_style = None;
        v.engine = None;
        v.transmission = None;
        v.drivetrain = None;
        let prompt = build_community_wisdom_prompt(&v);
        assert!(prompt.contains("unknown year"));
    }

    #[test]
    fn parse_ai_findings_plain_json() {
        let json = r#"[{"category":"forum_report","title":"Water pump failure","description":"Common at 60k miles","severity":"recommended"}]"#;
        let findings = parse_ai_findings(json).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].title, "Water pump failure");
    }

    #[test]
    fn parse_ai_findings_code_fenced() {
        let json = "```json\n[{\"category\":\"suggested_maintenance\",\"title\":\"Carbon \
                    buildup\",\"description\":\"Direct injection causes carbon \
                    buildup\",\"severity\":\"recommended\"}]\n```";
        let findings = parse_ai_findings(json).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "suggested_maintenance");
    }

    #[test]
    fn parse_ai_findings_invalid_json() {
        let result = parse_ai_findings("not json at all");
        assert!(result.is_err());
    }

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
    async fn generate_persists_report_and_get_reads_it_back() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let registry = AiProviderRegistry::new(db.clone());

        // No make/model/year and no AI provider configured -> empty findings report
        let report = generate(&db, &registry, vid, Some("community_wisdom".into()), None)
            .await
            .unwrap();
        assert_eq!(
            report.report.summary.as_deref(),
            Some("No findings. Vehicle appears to be in good standing.")
        );

        let listed = list_reports(&db, vid).await.unwrap();
        assert_eq!(listed.len(), 1);

        let fetched = get_report(&db, vid, report.report.id).await.unwrap();
        assert_eq!(fetched.report.id, report.report.id);
        assert!(fetched.findings.is_empty());
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
    }
}
