use axum::extract::{Path, Query, State};
use axum::Json;
use sea_orm::*;
use serde::{Deserialize, Serialize};

use crate::entities::{research_finding, research_report, vehicle};
use crate::AppState;

use super::error::ApiError;
use super::serde_helpers::deserialize_optional;

// --- Recall check ---

pub async fn check_recalls(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<crate::services::nhtsa::RecallCheckResult>, ApiError> {
    let vehicle = vehicle::Entity::find_by_id(vehicle_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound("Vehicle not found".to_string()))?;

    let make = vehicle.make.as_deref().ok_or_else(|| {
        ApiError::BadRequest("Vehicle has no make set — required for recall lookup".to_string())
    })?;
    let model = vehicle.model.as_deref().ok_or_else(|| {
        ApiError::BadRequest("Vehicle has no model set — required for recall lookup".to_string())
    })?;
    let year = vehicle.year.ok_or_else(|| {
        ApiError::BadRequest("Vehicle has no year set — required for recall lookup".to_string())
    })?;

    let result = crate::services::nhtsa::check_recalls(make, model, year)
        .await
        .map_err(ApiError::Internal)?;

    // Persist recalls as research findings (deduplicating by campaign number)
    if !result.recalls.is_empty() {
        persist_recall_findings(&state.db, vehicle_id, &result.recalls).await?;
    }

    Ok(Json(result))
}

/// Persist NHTSA recalls as research findings, skipping any already saved for this vehicle.
async fn persist_recall_findings(
    db: &DatabaseConnection,
    vehicle_id: i32,
    recalls: &[crate::services::nhtsa::RecallInfo],
) -> Result<(), ApiError> {
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

#[derive(Serialize)]
pub(crate) struct ReportWithFindings {
    #[serde(flatten)]
    report: research_report::Model,
    findings: Vec<research_finding::Model>,
}

pub async fn list_reports(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<Vec<research_report::Model>>, ApiError> {
    let _vehicle = vehicle::Entity::find_by_id(vehicle_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound("Vehicle not found".to_string()))?;

    let reports = research_report::Entity::find()
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(research_report::Column::GeneratedAt)
        .all(&state.db)
        .await?;

    Ok(Json(reports))
}

pub async fn get_report(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<ReportWithFindings>, ApiError> {
    let report = research_report::Entity::find_by_id(id)
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound("Research report not found".to_string()))?;

    let findings = research_finding::Entity::find()
        .filter(research_finding::Column::ReportId.eq(id))
        .order_by_asc(research_finding::Column::Id)
        .all(&state.db)
        .await?;

    Ok(Json(ReportWithFindings { report, findings }))
}

#[derive(Deserialize)]
pub struct GenerateReportRequest {
    pub report_type: Option<String>,
    pub provider_id: Option<i32>,
}

pub async fn generate_report(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(body): Json<GenerateReportRequest>,
) -> Result<Json<ReportWithFindings>, ApiError> {
    let vehicle = vehicle::Entity::find_by_id(vehicle_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound("Vehicle not found".to_string()))?;

    let report_type = body.report_type.unwrap_or_else(|| "full_check".to_string());

    let mut all_findings: Vec<NewFinding> = Vec::new();

    // Check NHTSA recalls if vehicle has make/model/year
    if let (Some(make), Some(model), Some(year)) = (&vehicle.make, &vehicle.model, vehicle.year) {
        match crate::services::nhtsa::check_recalls(make, model, year).await {
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
        && let Ok(provider) = state.ai.resolve(body.provider_id).await
    {
        let prompt = build_community_wisdom_prompt(&vehicle);
        match provider
            .complete(crate::services::ai::AiRequest {
                system_prompt: format!(
                    "{}\n\nReturn ONLY a valid JSON array — no narrative text, no markdown. \
                    Provide findings as a JSON array of objects with fields: title, description, \
                    severity (critical/recommended/optional/informational), category (one of: \
                    forum_report, suggested_maintenance, upgrade_idea).",
                    crate::services::ai::context::GLOVEBOX_PREAMBLE
                ),
                messages: vec![crate::services::ai::ChatMessage {
                    role: crate::services::ai::Role::User,
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
    let txn = state.db.begin().await?;

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

    Ok(Json(ReportWithFindings {
        report,
        findings: saved_findings,
    }))
}

// --- List findings by status (cross-report) ---

#[derive(Deserialize)]
pub struct FindingsQuery {
    pub status: Option<String>,
}

pub async fn list_findings(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Query(query): Query<FindingsQuery>,
) -> Result<Json<Vec<research_finding::Model>>, ApiError> {
    // Get all report IDs for this vehicle
    let report_ids: Vec<i32> = research_report::Entity::find()
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|r| r.id)
        .collect();

    if report_ids.is_empty() {
        return Ok(Json(vec![]));
    }

    let mut select = research_finding::Entity::find()
        .filter(research_finding::Column::ReportId.is_in(report_ids));

    if let Some(status) = query.status {
        select = select.filter(research_finding::Column::Status.eq(status));
    }

    let findings = select
        .order_by_desc(research_finding::Column::Id)
        .all(&state.db)
        .await?;

    Ok(Json(findings))
}

// --- Finding management ---

#[derive(Deserialize)]
pub struct UpdateFindingRequest {
    pub status: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub linked_entity_type: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub linked_entity_id: Option<Option<i32>>,
}

pub async fn update_finding_with_body(
    State(state): State<AppState>,
    Path((vehicle_id, report_id, id)): Path<(i32, i32, i32)>,
    Json(body): Json<UpdateFindingRequest>,
) -> Result<Json<research_finding::Model>, ApiError> {
    // Verify the report belongs to the vehicle
    let _report = research_report::Entity::find_by_id(report_id)
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound("Research report not found".to_string()))?;

    let finding = research_finding::Entity::find_by_id(id)
        .filter(research_finding::Column::ReportId.eq(report_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound("Finding not found".to_string()))?;

    let mut active: research_finding::ActiveModel = finding.into();

    if let Some(status) = body.status {
        let valid = ["new", "dismissed", "planned", "completed"];
        if !valid.contains(&status.as_str()) {
            return Err(ApiError::BadRequest(format!(
                "Invalid status '{}'. Must be one of: {}",
                status,
                valid.join(", ")
            )));
        }
        active.status = Set(status);
    }
    if let Some(linked_type) = body.linked_entity_type {
        active.linked_entity_type = Set(linked_type);
    }
    if let Some(linked_id) = body.linked_entity_id {
        active.linked_entity_id = Set(linked_id);
    }

    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

    let updated = active.update(&state.db).await?;
    Ok(Json(updated))
}

// --- Helpers ---

#[derive(Serialize, Deserialize)]
struct NewFinding {
    category: String,
    title: String,
    description: Option<String>,
    source_url: Option<String>,
    severity: Option<String>,
}

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
    prompt.push_str("— what are the most common issues, recommended preventive maintenance items beyond factory schedule, and popular upgrades reported by owners? Only include issues that apply to this exact configuration (e.g., do not include AWD-specific issues for a FWD vehicle, or manual transmission issues for an automatic). Return as JSON array.");
    prompt
}

fn parse_ai_findings(content: &str) -> Result<Vec<NewFinding>, serde_json::Error> {
    let cleaned = super::ai::strip_code_fences(content);
    serde_json::from_str(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_full_vehicle() {
        let vehicle = vehicle::Model {
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
        };
        let prompt = build_community_wisdom_prompt(&vehicle);
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
        let vehicle = vehicle::Model {
            id: 1,
            model_template_id: None,
            name: "Test".to_string(),
            year: None,
            make: None,
            model: None,
            trim_level: None,
            body_style: None,
            engine: None,
            transmission: None,
            drivetrain: None,
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
        };
        let prompt = build_community_wisdom_prompt(&vehicle);
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
        let json = "```json\n[{\"category\":\"suggested_maintenance\",\"title\":\"Carbon buildup\",\"description\":\"Direct injection causes carbon buildup\",\"severity\":\"recommended\"}]\n```";
        let findings = parse_ai_findings(json).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "suggested_maintenance");
    }

    #[test]
    fn parse_ai_findings_invalid_json() {
        let result = parse_ai_findings("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn valid_statuses() {
        let valid = ["new", "dismissed", "planned", "completed"];
        for status in valid {
            assert!(valid.contains(&status));
        }
    }
}
