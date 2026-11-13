//! Recalls: research findings with category `recall`, seen with a status that
//! says whether the work that addresses them is planned or done.
//!
//! A recall is `done` when a service is linked to it or its stored status is
//! `completed`, `not_applicable` when its stored status is `dismissed`,
//! `planned` when an open work item points at it, and `open` otherwise.

use std::collections::HashMap;

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{research_finding, research_report, service_recall_link, service_record, work_item},
    error::{DomainError, DomainResult},
    services::incident::ServiceRef,
};

/// The statuses a caller can set with [`set_status`].
pub const VALID_STATUSES: [&str; 3] = ["open", "done", "not_applicable"];

/// A recall as the owner sees it.
#[derive(Debug, Serialize)]
pub struct RecallWithStatus {
    pub finding_id: i32,
    /// The NHTSA campaign number, when the recall's source URL carries one.
    pub campaign_number: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub source_url: Option<String>,
    /// One of `open`, `planned`, `done` or `not_applicable`.
    pub status: String,
    /// The work item that addresses or addressed this recall: the planned
    /// one when there is one, otherwise the latest completed one. Only a
    /// planned item makes `status` read `planned`.
    pub work_item_id: Option<i32>,
    /// The latest service linked to this recall, if any.
    pub done_service: Option<ServiceRef>,
}

/// A recall as seen from a service that performed it.
#[derive(Clone, Debug, Serialize)]
pub struct RecallRef {
    pub finding_id: i32,
    pub campaign_number: Option<String>,
    pub title: String,
}

/// The value of the `nhtsaId` query parameter in a recall's source URL.
pub(crate) fn campaign_number(source_url: Option<&str>) -> Option<String> {
    let query = source_url?.split_once('?')?.1;
    let query = query.split('#').next().unwrap_or(query);
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("nhtsaId="))
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

fn not_found(finding_id: i32) -> DomainError {
    DomainError::NotFound(format!("Recall {finding_id} not found"))
}

/// Loads the findings behind `finding_ids` that are recalls on this vehicle.
/// A finding that does not exist, belongs to another vehicle or is not a
/// recall produces the same `NotFound`.
async fn require_recalls(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    finding_ids: &[i32],
) -> DomainResult<Vec<research_finding::Model>> {
    let report_ids: Vec<i32> = research_report::Entity::find()
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .all(db)
        .await?
        .into_iter()
        .map(|r| r.id)
        .collect();
    let found: HashMap<i32, research_finding::Model> = research_finding::Entity::find()
        .filter(research_finding::Column::Id.is_in(finding_ids.to_vec()))
        .filter(research_finding::Column::ReportId.is_in(report_ids))
        .filter(research_finding::Column::Category.eq("recall"))
        .all(db)
        .await?
        .into_iter()
        .map(|f| (f.id, f))
        .collect();
    finding_ids
        .iter()
        .map(|id| found.get(id).cloned().ok_or_else(|| not_found(*id)))
        .collect()
}

/// Attaches a status to each finding with a fixed number of queries.
async fn with_status(
    db: &impl ConnectionTrait,
    findings: Vec<research_finding::Model>,
) -> DomainResult<Vec<RecallWithStatus>> {
    if findings.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<i32> = findings.iter().map(|f| f.id).collect();

    let links = service_recall_link::Entity::find()
        .filter(service_recall_link::Column::ResearchFindingId.is_in(ids.clone()))
        .all(db)
        .await?;
    let services: HashMap<i32, service_record::Model> = service_record::Entity::find()
        .filter(service_record::Column::Id.is_in(links.iter().map(|l| l.service_record_id)))
        .all(db)
        .await?
        .into_iter()
        .map(|s| (s.id, s))
        .collect();
    let mut latest_service: HashMap<i32, &service_record::Model> = HashMap::new();
    for link in &links {
        let Some(service) = services.get(&link.service_record_id) else {
            continue;
        };
        let slot = latest_service
            .entry(link.research_finding_id)
            .or_insert(service);
        if (&service.service_date, service.id) > (&slot.service_date, slot.id) {
            *slot = service;
        }
    }

    // The planned item wins; otherwise the most recently completed one.
    let mut planned: HashMap<i32, i32> = HashMap::new();
    let mut completed: HashMap<i32, i32> = HashMap::new();
    for item in work_item::Entity::find()
        .filter(work_item::Column::ResearchFindingId.is_in(ids))
        .filter(work_item::Column::Status.is_in(["planned", "done"]))
        .order_by_asc(work_item::Column::Id)
        .all(db)
        .await?
    {
        let Some(finding_id) = item.research_finding_id else {
            continue;
        };
        if item.status == "planned" {
            planned.entry(finding_id).or_insert(item.id);
        } else {
            let slot = completed.entry(finding_id).or_insert(item.id);
            *slot = item.id;
        }
    }

    Ok(findings
        .into_iter()
        .map(|f| {
            let done_service = latest_service
                .get(&f.id)
                .map(|s| ServiceRef::from((*s).clone()));
            let planned_work_item_id = planned.get(&f.id).copied();
            let work_item_id = planned_work_item_id.or_else(|| completed.get(&f.id).copied());
            let status = if done_service.is_some() || f.status == "completed" {
                "done"
            } else if f.status == "dismissed" {
                "not_applicable"
            } else if planned_work_item_id.is_some() {
                "planned"
            } else {
                "open"
            };
            RecallWithStatus {
                finding_id: f.id,
                campaign_number: campaign_number(f.source_url.as_deref()),
                title: f.title,
                description: f.description,
                source_url: f.source_url,
                status: status.to_owned(),
                work_item_id,
                done_service,
            }
        })
        .collect())
}

/// Every recall on the vehicle with its status, oldest first.
pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<Vec<RecallWithStatus>> {
    let report_ids: Vec<i32> = research_report::Entity::find()
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .all(db)
        .await?
        .into_iter()
        .map(|r| r.id)
        .collect();
    let findings = research_finding::Entity::find()
        .filter(research_finding::Column::ReportId.is_in(report_ids))
        .filter(research_finding::Column::Category.eq("recall"))
        .order_by_asc(research_finding::Column::Id)
        .all(db)
        .await?;
    with_status(db, findings).await
}

/// [`list`] for a vehicle that must exist: a missing vehicle is a `NotFound`
/// rather than an empty list.
pub async fn list_for_vehicle(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<Vec<RecallWithStatus>> {
    super::vehicle::require(db, vehicle_id).await?;
    list(db, vehicle_id).await
}

/// The stored finding status that makes a recall with no service read as
/// `status`, or `None` for a status a caller cannot set.
fn stored_status(status: &str) -> Option<&'static str> {
    match status {
        "open" => Some("new"),
        "done" => Some("completed"),
        "not_applicable" => Some("dismissed"),
        _ => None,
    }
}

/// Sets a recall to `open`, `done` or `not_applicable` without a service.
///
/// A recall linked to a service is `done` for as long as the link exists, so
/// setting it to anything else is a `BadRequest`.
pub async fn set_status(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    finding_id: i32,
    status: &str,
) -> DomainResult<RecallWithStatus> {
    let Some(stored) = stored_status(status) else {
        return Err(DomainError::BadRequest(format!(
            "Invalid status '{status}'. Must be one of: {}",
            VALID_STATUSES.join(", ")
        )));
    };
    let finding = require_recalls(db, vehicle_id, &[finding_id])
        .await?
        .remove(0);

    if status != "done" {
        let linked = service_recall_link::Entity::find()
            .filter(service_recall_link::Column::ResearchFindingId.eq(finding_id))
            .count(db)
            .await?;
        if linked > 0 {
            return Err(DomainError::BadRequest(format!(
                "Recall {finding_id} is linked to a service, so it stays done until that service \
                 is deleted"
            )));
        }
    }

    let mut active: research_finding::ActiveModel = finding.into();
    active.status = Set(stored.to_owned());
    active.updated_at = Set(super::now_stamp());
    let updated = active.update(db).await?;
    Ok(with_status(db, vec![updated]).await?.remove(0))
}

/// The ids among `finding_ids` whose findings are recalls, in one query.
/// Ownership is not checked; callers pass ids that are already vehicle-scoped.
pub(crate) async fn recall_ids_among(
    db: &impl ConnectionTrait,
    finding_ids: &[i32],
) -> DomainResult<Vec<i32>> {
    if finding_ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(research_finding::Entity::find()
        .filter(research_finding::Column::Id.is_in(finding_ids.to_vec()))
        .filter(research_finding::Column::Category.eq("recall"))
        .all(db)
        .await?
        .into_iter()
        .map(|f| f.id)
        .collect())
}

/// Links a service to the recalls it performed, inside the caller's
/// transaction when there is one. Every id must be a recall on the vehicle,
/// and linking the same recall twice is a no-op.
pub(crate) async fn link_service(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    finding_ids: &[i32],
    service_record_id: i32,
) -> DomainResult<()> {
    let unique = super::dedup_ids(finding_ids);
    if unique.is_empty() {
        return Ok(());
    }
    require_recalls(db, vehicle_id, &unique).await?;
    for finding_id in unique {
        let exists = service_recall_link::Entity::find_by_id((service_record_id, finding_id))
            .one(db)
            .await?
            .is_some();
        if !exists {
            service_recall_link::ActiveModel {
                service_record_id: Set(service_record_id),
                research_finding_id: Set(finding_id),
            }
            .insert(db)
            .await?;
        }
    }
    Ok(())
}

/// What happens to the recalls a deleted service performed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecallsOnDelete {
    /// Each recall reads `open` again, unless another service also performed it.
    Reopen,
    /// Each recall reads `done` with no service, as `recall::set_status(.., "done")` stores it.
    KeepDone,
}

/// The recalls a deleted service performed, by what became of them.
#[derive(Debug, Default)]
pub(crate) struct ReleasedRecalls {
    pub reopened: Vec<i32>,
    pub kept_done: Vec<i32>,
    pub still_done_elsewhere: Vec<i32>,
}

/// Applies the owner's choice to the recalls a deleted service performed, on
/// the caller's transaction. Call it once that service's recall links are
/// gone: a reopened recall that still reads `done` is one another service
/// also performed.
pub(crate) async fn release_from_deleted_service(
    db: &impl ConnectionTrait,
    finding_ids: Vec<i32>,
    choice: RecallsOnDelete,
) -> DomainResult<ReleasedRecalls> {
    let mut out = ReleasedRecalls::default();
    if finding_ids.is_empty() {
        return Ok(out);
    }
    let shown = match choice {
        RecallsOnDelete::Reopen => "open",
        RecallsOnDelete::KeepDone => "done",
    };
    let stored = stored_status(shown).expect("open and done are settable statuses");
    research_finding::Entity::update_many()
        .set(research_finding::ActiveModel {
            status: Set(stored.to_owned()),
            updated_at: Set(super::now_stamp()),
            ..Default::default()
        })
        .filter(research_finding::Column::Id.is_in(finding_ids.clone()))
        .exec(db)
        .await?;
    if choice == RecallsOnDelete::KeepDone {
        out.kept_done = finding_ids;
        return Ok(out);
    }
    let findings = research_finding::Entity::find()
        .filter(research_finding::Column::Id.is_in(finding_ids.clone()))
        .all(db)
        .await?;
    let done: std::collections::HashSet<i32> = with_status(db, findings)
        .await?
        .into_iter()
        .filter(|r| r.status == "done")
        .map(|r| r.finding_id)
        .collect();
    (out.still_done_elsewhere, out.reopened) =
        finding_ids.into_iter().partition(|id| done.contains(id));
    Ok(out)
}

/// The recalls each of the given services performed, keyed by service id.
pub(crate) async fn load_performed(
    db: &impl ConnectionTrait,
    service_ids: &[i32],
) -> DomainResult<HashMap<i32, Vec<RecallRef>>> {
    let mut out: HashMap<i32, Vec<RecallRef>> = HashMap::new();
    if service_ids.is_empty() {
        return Ok(out);
    }
    let links = service_recall_link::Entity::find()
        .filter(service_recall_link::Column::ServiceRecordId.is_in(service_ids.to_vec()))
        .all(db)
        .await?;
    let findings: HashMap<i32, research_finding::Model> = research_finding::Entity::find()
        .filter(research_finding::Column::Id.is_in(links.iter().map(|l| l.research_finding_id)))
        .all(db)
        .await?
        .into_iter()
        .map(|f| (f.id, f))
        .collect();
    for link in links {
        if let Some(f) = findings.get(&link.research_finding_id) {
            out.entry(link.service_record_id)
                .or_default()
                .push(RecallRef {
                    finding_id: f.id,
                    campaign_number: campaign_number(f.source_url.as_deref()),
                    title: f.title.clone(),
                });
        }
    }
    for refs in out.values_mut() {
        refs.sort_by_key(|r| r.finding_id);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::{mileage_log, part, service_record_line_item},
        inputs::{
            part::NewPart,
            service_record::{NewLineItem, NewServiceRecord},
            work_item::NewWorkItem,
        },
        services::{
            nhtsa::RecallInfo,
            research,
            service_record::{self as service_svc, ServiceRecordWithLinks},
            work_item as work_item_svc,
        },
        test_support::{VehicleFixture, test_db},
    };

    fn recall_info(campaign: &str, subject: &str) -> RecallInfo {
        RecallInfo {
            campaign_number: campaign.into(),
            manufacturer: None,
            title: subject.into(),
            summary: Some("Summary".into()),
            consequence: None,
            remedy: None,
            report_date: None,
            component: None,
            action_number: None,
        }
    }

    async fn seed_vehicle(db: &DatabaseConnection) -> i32 {
        VehicleFixture::new().insert_id(db).await
    }

    async fn plan_work_for_recall(
        db: &DatabaseConnection,
        vehicle_id: i32,
        finding_id: i32,
    ) -> work_item::Model {
        work_item_svc::create(
            db,
            vehicle_id,
            NewWorkItem {
                title: "Fix recall".into(),
                notes: None,
                schedule_item_id: None,
                research_finding_id: Some(finding_id),
                incident_id: None,
                est_cost_cents: None,
                revisit_by_date: None,
                revisit_by_miles: None,
            },
        )
        .await
        .unwrap()
    }

    fn service(date: &str) -> NewServiceRecord {
        NewServiceRecord {
            service_date: date.into(),
            ..Default::default()
        }
    }

    async fn record_service_with_recalls(
        db: &DatabaseConnection,
        vehicle_id: i32,
        date: &str,
        recall_finding_ids: Vec<i32>,
    ) -> ServiceRecordWithLinks {
        service_svc::create(
            db,
            vehicle_id,
            NewServiceRecord {
                recall_finding_ids: Some(recall_finding_ids),
                ..service(date)
            },
        )
        .await
        .unwrap()
    }

    async fn seed_recall(db: &DatabaseConnection, vehicle_id: i32, campaign: &str) -> i32 {
        research::record_recalls(db, vehicle_id, &[recall_info(campaign, "Pump")])
            .await
            .unwrap()
            .recalls
            .into_iter()
            .find(|r| r.campaign_number.as_deref() == Some(campaign))
            .unwrap()
            .finding_id
    }

    async fn seed_non_recall(db: &DatabaseConnection, vehicle_id: i32) -> i32 {
        research::file_finding(
            db,
            vehicle_id,
            research::NewFiledFinding {
                category: "maintenance".into(),
                title: "Community fix".into(),
                description: None,
                source_url: None,
                severity: None,
            },
        )
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn a_recall_moves_from_open_to_planned_to_done() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let found = [recall_info("24V110000", "Suction Jet Pump")];
        let check = research::record_recalls(&db, v, &found).await.unwrap();
        assert_eq!(
            (check.newly_found, check.recalls[0].status.as_str()),
            (1, "open")
        );
        assert_eq!(
            check.recalls[0].campaign_number.as_deref(),
            Some("24V110000")
        );

        let item = plan_work_for_recall(&db, v, check.recalls[0].finding_id).await;
        let r = &list(&db, v).await.unwrap()[0];
        assert_eq!(
            (r.status.as_str(), r.work_item_id),
            ("planned", Some(item.id))
        );

        let svc = record_service_with_recalls(&db, v, "2025-03-25", vec![r.finding_id]).await;
        let r = &list(&db, v).await.unwrap()[0];
        assert_eq!(r.status, "done");
        assert_eq!(r.done_service.as_ref().unwrap().service_date, "2025-03-25");
        assert_eq!(r.done_service.as_ref().unwrap().id, svc.record.id);
        assert_eq!(svc.performed_recalls.len(), 1);
        assert_eq!(
            svc.performed_recalls[0].campaign_number.as_deref(),
            Some("24V110000")
        );
        let read = service_svc::get(&db, v, svc.record.id).await.unwrap();
        assert_eq!(read.performed_recalls[0].finding_id, r.finding_id);
    }

    #[tokio::test]
    async fn checking_again_reports_known_recalls_with_their_status_and_zero_new() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let found = [recall_info("24V110000", "Pump")];
        let first = research::record_recalls(&db, v, &found).await.unwrap();
        set_status(&db, v, first.recalls[0].finding_id, "done")
            .await
            .unwrap();

        let second = research::record_recalls(&db, v, &found).await.unwrap();
        assert_eq!(second.newly_found, 0);
        assert_eq!(second.recalls.len(), 1);
        assert_eq!(second.recalls[0].status, "done");
    }

    #[tokio::test]
    async fn a_recall_can_be_marked_done_or_not_applicable_without_a_service() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let id = seed_recall(&db, v, "24V110000").await;
        assert_eq!(
            set_status(&db, v, id, "not_applicable")
                .await
                .unwrap()
                .status,
            "not_applicable"
        );
        assert_eq!(set_status(&db, v, id, "done").await.unwrap().status, "done");
        assert_eq!(set_status(&db, v, id, "open").await.unwrap().status, "open");
        assert!(set_status(&db, v, id, "closed").await.is_err());
    }

    #[tokio::test]
    async fn a_recall_linked_to_a_service_cannot_be_reopened() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let id = seed_recall(&db, v, "24V110000").await;
        record_service_with_recalls(&db, v, "2025-03-25", vec![id]).await;
        for status in ["open", "not_applicable"] {
            let err = set_status(&db, v, id, status).await.unwrap_err();
            assert!(
                matches!(&err, DomainError::BadRequest(m) if m.contains("linked to a service")),
                "got {err:?}"
            );
        }
        assert_eq!(list(&db, v).await.unwrap()[0].status, "done");
    }

    #[tokio::test]
    async fn a_recall_from_another_vehicle_or_a_non_recall_finding_is_not_found() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign = seed_recall(&db, other, "24V999000").await;
        let note = research::file_finding(
            &db,
            v,
            research::NewFiledFinding {
                category: "maintenance".into(),
                title: "Not a recall".into(),
                description: None,
                source_url: None,
                severity: None,
            },
        )
        .await
        .unwrap()
        .id;

        let missing = set_status(&db, v, 9999, "done").await.unwrap_err();
        for id in [foreign, note] {
            let err = set_status(&db, v, id, "done").await.unwrap_err();
            assert!(matches!(err, DomainError::NotFound(_)));
            assert_eq!(
                err.to_string().replace(&id.to_string(), "N"),
                missing.to_string().replace("9999", "N")
            );
        }
        assert_eq!(list(&db, other).await.unwrap()[0].status, "open");

        let sched = crate::entities::maintenance_schedule_item::ActiveModel {
            vehicle_id: Set(Some(v)),
            name: Set("Oil".into()),
            enabled: Set(true),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        for id in [foreign, note, 9999] {
            let full = NewServiceRecord {
                mileage: Some(50_000),
                line_items: Some(vec![NewLineItem {
                    description: "Oil".into(),
                    work_kind: "maintenance".into(),
                    quantity: None,
                    unit_cost_cents: None,
                    cost_cents: None,
                }]),
                parts: Some(vec![NewPart {
                    name: "Filter".into(),
                    ..Default::default()
                }]),
                schedule_item_ids: Some(vec![sched]),
                recall_finding_ids: Some(vec![id]),
                ..service("2026-11-12")
            };
            let err = service_svc::create(&db, v, full).await.unwrap_err();
            assert!(matches!(err, DomainError::NotFound(_)), "got {err:?}");
            assert_eq!(
                err.to_string().replace(&id.to_string(), "N"),
                missing.to_string().replace("9999", "N")
            );
            assert_eq!(service_record::Entity::find().count(&db).await.unwrap(), 0);
            assert_eq!(
                service_record_line_item::Entity::find()
                    .count(&db)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(part::Entity::find().count(&db).await.unwrap(), 0);
            assert_eq!(mileage_log::Entity::find().count(&db).await.unwrap(), 0);
            assert_eq!(
                service_recall_link::Entity::find()
                    .count(&db)
                    .await
                    .unwrap(),
                0
            );
        }
    }

    #[tokio::test]
    async fn completing_a_recall_work_item_links_its_recall_once() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let id = seed_recall(&db, v, "24V110000").await;
        let item = plan_work_for_recall(&db, v, id).await;

        // The recall is named explicitly and also comes from the work item.
        let svc = service_svc::create(
            &db,
            v,
            NewServiceRecord {
                work_item_ids: Some(vec![item.id]),
                recall_finding_ids: Some(vec![id]),
                ..service("2025-03-25")
            },
        )
        .await
        .unwrap();
        assert_eq!(svc.performed_recalls.len(), 1);
        let r = &list(&db, v).await.unwrap()[0];
        assert_eq!((r.status.as_str(), r.work_item_id), ("done", Some(item.id)));
        assert_eq!(r.done_service.as_ref().unwrap().id, svc.record.id);
    }

    #[tokio::test]
    async fn completing_work_from_a_non_recall_finding_links_no_recall() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let tip = seed_non_recall(&db, v).await;
        let item = plan_work_for_recall(&db, v, tip).await;

        let svc = service_svc::create(
            &db,
            v,
            NewServiceRecord {
                work_item_ids: Some(vec![item.id]),
                ..service("2025-03-25")
            },
        )
        .await
        .unwrap();
        assert!(svc.performed_recalls.is_empty());
        assert_eq!(
            work_item_svc::get(&db, v, item.id).await.unwrap().status,
            "done"
        );
        assert_eq!(
            service_recall_link::Entity::find()
                .count(&db)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn a_mixed_batch_of_work_items_links_only_the_recall() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let recall_id = seed_recall(&db, v, "24V110000").await;
        let tip = seed_non_recall(&db, v).await;
        let from_recall = plan_work_for_recall(&db, v, recall_id).await;
        let from_tip = plan_work_for_recall(&db, v, tip).await;

        let svc = service_svc::create(
            &db,
            v,
            NewServiceRecord {
                work_item_ids: Some(vec![from_recall.id, from_tip.id]),
                ..service("2025-03-25")
            },
        )
        .await
        .unwrap();
        let linked: Vec<i32> = svc.performed_recalls.iter().map(|r| r.finding_id).collect();
        assert_eq!(linked, vec![recall_id]);
        assert_eq!(
            service_recall_link::Entity::find()
                .count(&db)
                .await
                .unwrap(),
            1
        );
    }

    #[test]
    fn the_campaign_number_is_the_nhtsa_id_query_value() {
        assert_eq!(
            campaign_number(Some("https://www.nhtsa.gov/recalls?nhtsaId=20V123&x=1")),
            Some("20V123".into())
        );
        assert_eq!(campaign_number(Some("https://example.com/x")), None);
        assert_eq!(campaign_number(None), None);
    }
}
