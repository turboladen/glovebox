//! The `visit` planning primitive (2hea unit G, decision ⑦): a shop trip or
//! DIY session grouping work items, with a cost rollup. [`complete`] is the
//! loop-closer: one transaction creates the service record (clearing
//! satisfied reminders via the items' schedule links), links
//! incident-sourced items, resolves recall findings, marks the items done,
//! and stamps the visit completed.
//!
//! Nested-transaction note (the plan's read-first decision): `complete`
//! opens ONE outer transaction and calls `service_record::create(&txn, …)`,
//! which internally calls `begin()` again. `SeaORM`'s `DatabaseTransaction`
//! implements `TransactionTrait`, and a nested `begin()` on `SQLite` issues a
//! SAVEPOINT — so the record creation nests inside the outer unit and an
//! outer rollback undoes it. `create` is reused rather than inlined. The
//! rollback test below empirically proves outer-drop rollback and
//! savepoint-commit visibility; the savepoint-internal-error path is
//! unexercised, because `create`'s guards all run before its savepoint opens.

use std::collections::BTreeSet;

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{incident_service_link, research_finding, visit, work_item},
    error::{DomainError, DomainResult},
    inputs::{
        service_record::NewServiceRecord,
        visit::{CompleteVisit, NewVisit, UpdateVisit},
    },
    services::{
        service_record, service_record::ServiceRecordWithLinks, work_item as work_item_svc,
    },
};

/// Lifecycle whitelist for `visits.status`. `completed` is only reachable
/// through [`complete`]; `update` accepts the other three.
const VALID_STATUSES: [&str; 4] = ["planned", "scheduled", "completed", "canceled"];

/// A visit is **open** — accepts attaches, edits, and completion — only
/// while `planned` or `scheduled`. Completed/canceled visits are history.
pub(crate) fn is_open(status: &str) -> bool {
    matches!(status, "planned" | "scheduled")
}

fn now_stamp() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// A visit with its attached work items and their estimated-cost rollup.
#[derive(Debug, Serialize)]
pub struct VisitWithItems {
    #[serde(flatten)]
    pub visit: visit::Model,
    pub items: Vec<work_item::Model>,
    /// Σ attached items' `est_cost_cents`. Integer cents only.
    pub est_total_cents: i64,
}

/// Everything [`complete`] produced, returned from the one transaction.
#[derive(Debug, Serialize)]
pub struct CompletedVisit {
    pub visit: visit::Model,
    pub service_record: ServiceRecordWithLinks,
    pub items: Vec<work_item::Model>,
}

fn with_items(visit: visit::Model, items: Vec<work_item::Model>) -> VisitWithItems {
    let est_total_cents = items
        .iter()
        .filter_map(|i| i.est_cost_cents)
        .map(i64::from)
        .sum();
    VisitWithItems {
        visit,
        items,
        est_total_cents,
    }
}

async fn load_items(
    db: &impl ConnectionTrait,
    visit_id: i32,
) -> DomainResult<Vec<work_item::Model>> {
    Ok(work_item::Entity::find()
        .filter(work_item::Column::VisitId.eq(visit_id))
        .order_by_asc(work_item::Column::Id)
        .all(db)
        .await?)
}

/// Attach work items to a visit: each must belong to the vehicle
/// (cross-vehicle indistinguishable from nonexistent), must be
/// participating (`planned`/`scheduled` — done/dropped items are history),
/// and must not currently sit on a COMPLETED visit (provenance: completed
/// visits' items never move; canceled-visit items may). Sets `visit_id`
/// and flips status to `scheduled`.
async fn attach_items(
    txn: &impl ConnectionTrait,
    vehicle_id: i32,
    visit_id: i32,
    item_ids: &[i32],
) -> DomainResult<()> {
    if item_ids.is_empty() {
        return Ok(());
    }
    let owned = work_item::Entity::find()
        .filter(work_item::Column::Id.is_in(item_ids.to_vec()))
        .filter(work_item::Column::VehicleId.eq(vehicle_id))
        .all(txn)
        .await?;
    let owned_ids: BTreeSet<i32> = owned.iter().map(|i| i.id).collect();
    if let Some(missing) = item_ids.iter().find(|id| !owned_ids.contains(id)) {
        return Err(DomainError::NotFound(format!(
            "Work item {missing} not found"
        )));
    }

    // Provenance guard: batch-load the items' current visits (excluding
    // this one) and reject any move out of a completed visit.
    let source_ids: Vec<i32> = owned
        .iter()
        .filter_map(|i| i.visit_id)
        .filter(|v| *v != visit_id)
        .collect();
    let completed_sources: BTreeSet<i32> = if source_ids.is_empty() {
        BTreeSet::new()
    } else {
        visit::Entity::find()
            .filter(visit::Column::Id.is_in(source_ids))
            .filter(visit::Column::Status.eq("completed"))
            .all(txn)
            .await?
            .into_iter()
            .map(|v| v.id)
            .collect()
    };

    for item in owned {
        if item.visit_id == Some(visit_id) && !work_item_svc::participates(&item.status) {
            // Already-attached history (done/dropped) re-listed in a
            // replace-all update: leave it untouched.
            continue;
        }
        if let Some(src) = item.visit_id
            && completed_sources.contains(&src)
        {
            return Err(DomainError::BadRequest(format!(
                "Work item {} is attached to completed visit {src} and cannot be moved",
                item.id
            )));
        }
        if !work_item_svc::participates(&item.status) {
            return Err(DomainError::BadRequest(format!(
                "Work item {} is {} and cannot be attached to a visit",
                item.id, item.status
            )));
        }
        let mut active: work_item::ActiveModel = item.into();
        active.visit_id = Set(Some(visit_id));
        active.status = Set("scheduled".to_string());
        active.updated_at = Set(now_stamp());
        active.update(txn).await?;
    }
    Ok(())
}

/// Detach the given items from their visit: `visit_id` NULL, status back to
/// `planned` when it was `scheduled` (done/dropped items keep their status).
async fn detach_items(
    txn: &impl ConnectionTrait,
    items: Vec<work_item::Model>,
) -> DomainResult<()> {
    for item in items {
        let was_scheduled = item.status == "scheduled";
        let mut active: work_item::ActiveModel = item.into();
        active.visit_id = Set(None);
        if was_scheduled {
            active.status = Set("planned".to_string());
        }
        active.updated_at = Set(now_stamp());
        active.update(txn).await?;
    }
    Ok(())
}

pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    include_closed: bool,
) -> DomainResult<Vec<VisitWithItems>> {
    crate::services::vehicle::require(db, vehicle_id).await?;
    let mut query = visit::Entity::find().filter(visit::Column::VehicleId.eq(vehicle_id));
    if !include_closed {
        query = query.filter(visit::Column::Status.is_not_in(["completed", "canceled"]));
    }
    let visits = query
        .order_by_desc(visit::Column::CreatedAt)
        .order_by_desc(visit::Column::Id)
        .all(db)
        .await?;

    // Batch-load all attached items (avoids N+1).
    let visit_ids: Vec<i32> = visits.iter().map(|v| v.id).collect();
    let all_items = if visit_ids.is_empty() {
        vec![]
    } else {
        work_item::Entity::find()
            .filter(work_item::Column::VisitId.is_in(visit_ids))
            .order_by_asc(work_item::Column::Id)
            .all(db)
            .await?
    };

    Ok(visits
        .into_iter()
        .map(|v| {
            let items: Vec<work_item::Model> = all_items
                .iter()
                .filter(|i| i.visit_id == Some(v.id))
                .cloned()
                .collect();
            with_items(v, items)
        })
        .collect())
}

/// Fetch a visit scoped to its owning vehicle. Wrong-vehicle lookups are
/// indistinguishable from nonexistent visits.
async fn require_visit(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<visit::Model> {
    work_item_svc::require_visit_owned(db, vehicle_id, id).await
}

pub async fn get(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<VisitWithItems> {
    let v = require_visit(db, vehicle_id, id).await?;
    let items = load_items(db, v.id).await?;
    Ok(with_items(v, items))
}

pub async fn create<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    input: NewVisit,
) -> DomainResult<VisitWithItems> {
    crate::services::vehicle::require(db, vehicle_id).await?;

    let txn = db.begin().await?;

    let model = visit::ActiveModel {
        vehicle_id: Set(vehicle_id),
        planned_date: Set(input.planned_date),
        shop_name: Set(input.shop_name),
        shop_id: Set(input.shop_id),
        notes: Set(input.notes),
        ..Default::default()
    };
    let v = model.insert(&txn).await?;

    let item_ids = input.work_item_ids.unwrap_or_default();
    attach_items(&txn, vehicle_id, v.id, &item_ids).await?;
    let items = load_items(&txn, v.id).await?;

    txn.commit().await?;

    Ok(with_items(v, items))
}

pub async fn update<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    input: UpdateVisit,
) -> DomainResult<VisitWithItems> {
    let existing = require_visit(db, vehicle_id, id).await?;

    // Completed/canceled visits are immutable history — no reopen, no
    // re-titling, no attach churn (mirrors `complete`'s own guard).
    if !is_open(&existing.status) {
        return Err(DomainError::BadRequest(format!(
            "Visit {id} is already {} and cannot be updated",
            existing.status
        )));
    }

    if let Some(status) = &input.status {
        if status == "completed" {
            return Err(DomainError::BadRequest(
                "Status 'completed' is set by completing the visit — use complete, which creates \
                 the service record and closes out the attached work items"
                    .to_string(),
            ));
        }
        if !VALID_STATUSES.contains(&status.as_str()) {
            return Err(DomainError::BadRequest(format!(
                "Invalid status '{}'. Must be one of: {}",
                status,
                VALID_STATUSES.join(", ")
            )));
        }
    }

    let txn = db.begin().await?;

    let mut active: visit::ActiveModel = existing.into();
    if let Some(v) = input.planned_date {
        active.planned_date = Set(v);
    }
    if let Some(v) = input.shop_name {
        active.shop_name = Set(v);
    }
    if let Some(v) = input.shop_id {
        active.shop_id = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }
    if let Some(v) = input.status {
        active.status = Set(v);
    }
    active.updated_at = Set(now_stamp());
    let v = active.update(&txn).await?;

    // Replace-all attach semantics when work_item_ids is provided.
    if let Some(new_ids) = input.work_item_ids {
        let current = load_items(&txn, v.id).await?;
        let to_detach: Vec<work_item::Model> = current
            .into_iter()
            .filter(|i| !new_ids.contains(&i.id))
            .collect();
        detach_items(&txn, to_detach).await?;
        attach_items(&txn, vehicle_id, v.id, &new_ids).await?;
    }

    // Canceling detaches exactly like `delete`: the items go back to the
    // backlog (visit_id NULL, scheduled → planned) instead of hanging off
    // a visit that will never happen.
    if v.status == "canceled" {
        let items = load_items(&txn, v.id).await?;
        detach_items(&txn, items).await?;
    }
    let items = load_items(&txn, v.id).await?;

    txn.commit().await?;

    Ok(with_items(v, items))
}

/// Delete a visit: detach its items first (they return to the backlog as
/// `planned`), then remove the visit — one transaction.
pub async fn delete<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<()> {
    let existing = require_visit(db, vehicle_id, id).await?;

    let txn = db.begin().await?;
    let items = load_items(&txn, existing.id).await?;
    detach_items(&txn, items).await?;
    existing.delete(&txn).await?;
    txn.commit().await?;

    Ok(())
}

/// Close out a visit — the recall→plan→visit→service-record loop-closer.
/// ONE atomic unit:
/// 1. the visit must exist (vehicle-scoped) and be open (not already
///    completed/canceled) — checked inside the transaction, so a
///    concurrent close can't slip between check and use;
/// 2. participating attached items (`planned`/`scheduled`) are marked
///    `done` — dropped/done items stay attached with their status as
///    history and contribute nothing below;
/// 3. the service record is created via [`service_record::create`]
///    (description defaults to the participating items' joined titles;
///    their non-null `schedule_item_id`s become schedule links, clearing
///    reminders; payer-aware) — nested as a savepoint, see the module docs;
/// 4. participating incident-sourced items link the record via
///    `incident_service_links`;
/// 5. participating research-finding-sourced items mark their finding
///    `completed` (closing the recall in the Research view) and back-link
///    it to the new record (`linked_entity_type: "service"`);
/// 6. the visit gets `service_record_id` + status `completed`.
///
/// Any failure (e.g. an invalid `paid_by`) rolls the whole unit back —
/// nothing is mutated.
pub async fn complete<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    visit_id: i32,
    input: CompleteVisit,
) -> DomainResult<CompletedVisit> {
    let txn = db.begin().await?;

    let existing = require_visit(&txn, vehicle_id, visit_id).await?;
    if !is_open(&existing.status) {
        return Err(DomainError::BadRequest(format!(
            "Visit {visit_id} is already {} and cannot be completed",
            existing.status
        )));
    }

    // Only participating items close out; dropped/done ones are history.
    let items: Vec<work_item::Model> = load_items(&txn, existing.id)
        .await?
        .into_iter()
        .filter(|i| work_item_svc::participates(&i.status))
        .collect();

    // Mark every participating item done (inside the txn, before the
    // record is built, so a later failure provably rolls this back).
    for item in &items {
        let mut active: work_item::ActiveModel = item.clone().into();
        active.status = Set("done".to_string());
        active.updated_at = Set(now_stamp());
        active.update(&txn).await?;
    }

    // Build the service record. Description defaults to the joined item
    // titles; shop comes from the visit unless the record is DIY.
    let joined_titles = items
        .iter()
        .map(|i| i.title.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let description = input
        .description
        .or_else(|| (!joined_titles.is_empty()).then_some(joined_titles));
    let schedule_item_ids: Vec<i32> = items
        .iter()
        .filter_map(|i| i.schedule_item_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    let record = service_record::create(
        &txn,
        vehicle_id,
        NewServiceRecord {
            service_date: input.service_date,
            mileage: input.mileage,
            description,
            parts_cost_cents: input.parts_cost_cents,
            parts_cost_currency: None,
            labor_cost_cents: input.labor_cost_cents,
            labor_cost_currency: None,
            total_cost_cents: input.total_cost_cents,
            total_cost_currency: None,
            shop_name: existing.shop_name.clone(),
            shop_id: existing.shop_id,
            notes: input.notes,
            build_id: None,
            paid_by: input.paid_by,
            payer_note: input.payer_note,
            schedule_item_ids: Some(schedule_item_ids),
            part_ids: None,
            line_items: None,
        },
    )
    .await?;

    // Incident-sourced items: link the record via incident_service_links
    // (deduplicated — two items from the same incident are one link).
    let incident_ids: BTreeSet<i32> = items.iter().filter_map(|i| i.incident_id).collect();
    for incident_id in incident_ids {
        incident_service_link::ActiveModel {
            incident_id: Set(incident_id),
            service_record_id: Set(record.record.id),
        }
        .insert(&txn)
        .await?;
    }

    // Research-finding-sourced items: the finding goes `completed`, which
    // closes the recall in the Research view, and back-links the service
    // record that resolved it.
    let finding_ids: BTreeSet<i32> = items.iter().filter_map(|i| i.research_finding_id).collect();
    for finding_id in finding_ids {
        let finding = research_finding::Entity::find_by_id(finding_id)
            .one(&txn)
            .await?
            .ok_or_else(|| {
                DomainError::NotFound(format!("Research finding {finding_id} not found"))
            })?;
        let mut active: research_finding::ActiveModel = finding.into();
        active.status = Set("completed".to_string());
        active.linked_entity_type = Set(Some("service".to_string()));
        active.linked_entity_id = Set(Some(record.record.id));
        active.updated_at = Set(now_stamp());
        active.update(&txn).await?;
    }

    // Stamp the visit itself.
    let mut active: visit::ActiveModel = existing.into();
    active.status = Set("completed".to_string());
    active.service_record_id = Set(Some(record.record.id));
    active.updated_at = Set(now_stamp());
    let completed = active.update(&txn).await?;

    let items = load_items(&txn, completed.id).await?;

    txn.commit().await?;

    Ok(CompletedVisit {
        visit: completed,
        service_record: record,
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::{mileage_log, research_report, service_record as service_record_entity},
        inputs::work_item::{NewWorkItem, UpdateWorkItem},
        services::reminders,
        test_support::test_db,
    };

    async fn seed_vehicle(db: &impl ConnectionTrait) -> i32 {
        use crate::entities::vehicle;
        vehicle::ActiveModel {
            name: Set("Car".into()),
            purchase_date: Set(Some("2020-01-01".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    /// A 12-month-interval item on a vehicle purchased 2020 — overdue until
    /// a linked service record clears it.
    async fn seed_overdue_schedule_item(db: &impl ConnectionTrait, vehicle_id: i32) -> i32 {
        use crate::entities::maintenance_schedule_item;
        maintenance_schedule_item::ActiveModel {
            vehicle_id: Set(Some(vehicle_id)),
            name: Set("Brake fluid flush".into()),
            interval_months: Set(Some(12)),
            enabled: Set(true),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn seed_finding(db: &impl ConnectionTrait, vehicle_id: i32) -> i32 {
        let report = research_report::ActiveModel {
            vehicle_id: Set(vehicle_id),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        research_finding::ActiveModel {
            report_id: Set(report.id),
            category: Set("recall".into()),
            title: Set("Recall: fuel pump".into()),
            status: Set("new".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn seed_incident(db: &impl ConnectionTrait, vehicle_id: i32) -> i32 {
        use crate::entities::incident;
        incident::ActiveModel {
            vehicle_id: Set(vehicle_id),
            category: Set("noise".into()),
            title: Set("Squeak".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    fn work(title: &str) -> NewWorkItem {
        NewWorkItem {
            title: title.into(),
            notes: None,
            schedule_item_id: None,
            research_finding_id: None,
            incident_id: None,
            build_id: None,
            est_cost_cents: None,
            visit_id: None,
        }
    }

    fn new_visit(work_item_ids: Option<Vec<i32>>) -> NewVisit {
        NewVisit {
            planned_date: Some("2026-08-01".into()),
            shop_name: Some("Joe's Garage".into()),
            shop_id: None,
            notes: None,
            work_item_ids,
        }
    }

    fn actuals() -> CompleteVisit {
        CompleteVisit {
            service_date: chrono::Utc::now()
                .date_naive()
                .format("%Y-%m-%d")
                .to_string(),
            mileage: Some(60_000),
            description: None,
            total_cost_cents: Some(45_000),
            parts_cost_cents: None,
            labor_cost_cents: None,
            paid_by: None,
            payer_note: None,
            notes: None,
        }
    }

    // --- create / attach / rollup -------------------------------------------

    #[tokio::test]
    async fn create_attaches_items_and_rolls_up_cost() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let a = work_item_svc::create(
            &db,
            vid,
            NewWorkItem {
                est_cost_cents: Some(10_000),
                ..work("Brakes")
            },
        )
        .await
        .unwrap();
        let b = work_item_svc::create(
            &db,
            vid,
            NewWorkItem {
                est_cost_cents: Some(2_500),
                ..work("Wipers")
            },
        )
        .await
        .unwrap();

        let v = create(&db, vid, new_visit(Some(vec![a.id, b.id])))
            .await
            .unwrap();
        assert_eq!(v.visit.status, "planned");
        assert_eq!(v.items.len(), 2);
        assert_eq!(v.est_total_cents, 12_500);
        assert!(v.items.iter().all(|i| i.status == "scheduled"));
        assert!(v.items.iter().all(|i| i.visit_id == Some(v.visit.id)));

        let fetched = get(&db, vid, v.visit.id).await.unwrap();
        assert_eq!(fetched.est_total_cents, 12_500);
    }

    #[tokio::test]
    async fn create_rejects_other_vehicles_items_and_creates_nothing() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign = work_item_svc::create(&db, other, work("theirs"))
            .await
            .unwrap();

        assert!(matches!(
            create(&db, owner, new_visit(Some(vec![foreign.id])))
                .await
                .unwrap_err(),
            DomainError::NotFound(_)
        ));
        // Rejected attach must not leave a visit row behind (txn rolled back).
        assert!(list(&db, owner, true).await.unwrap().is_empty());
        // The foreign item is untouched.
        let untouched = work_item_svc::get(&db, other, foreign.id).await.unwrap();
        assert_eq!(untouched.status, "planned");
        assert_eq!(untouched.visit_id, None);
    }

    // --- update: attach/detach + status whitelist ----------------------------

    #[tokio::test]
    async fn update_replaces_items_and_detached_go_back_to_planned() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let a = work_item_svc::create(&db, vid, work("A")).await.unwrap();
        let b = work_item_svc::create(&db, vid, work("B")).await.unwrap();
        let v = create(&db, vid, new_visit(Some(vec![a.id]))).await.unwrap();

        let updated = update(
            &db,
            vid,
            v.visit.id,
            UpdateVisit {
                status: Some("scheduled".into()),
                work_item_ids: Some(vec![b.id]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.visit.status, "scheduled");
        assert_eq!(
            updated.items.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![b.id]
        );

        // A was detached: back to the backlog as planned.
        let detached = work_item_svc::get(&db, vid, a.id).await.unwrap();
        assert_eq!(detached.visit_id, None);
        assert_eq!(detached.status, "planned");
        // B was attached: scheduled.
        let attached = work_item_svc::get(&db, vid, b.id).await.unwrap();
        assert_eq!(attached.visit_id, Some(v.visit.id));
        assert_eq!(attached.status, "scheduled");
    }

    #[tokio::test]
    async fn update_rejects_completed_status_and_bogus_status() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let v = create(&db, vid, new_visit(None)).await.unwrap();

        // `completed` must steer to complete().
        let err = update(
            &db,
            vid,
            v.visit.id,
            UpdateVisit {
                status: Some("completed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        match err {
            DomainError::BadRequest(msg) => {
                assert!(msg.contains("complete"), "must steer to complete(): {msg}");
            }
            other => panic!("expected BadRequest, got {other:?}"),
        }

        // Unknown status lists the whitelist.
        let err = update(
            &db,
            vid,
            v.visit.id,
            UpdateVisit {
                status: Some("bogus".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        match err {
            DomainError::BadRequest(msg) => {
                assert!(
                    msg.contains("Invalid status 'bogus'") && msg.contains("canceled"),
                    "house message shape: {msg}"
                );
            }
            other => panic!("expected BadRequest, got {other:?}"),
        }
        assert_eq!(
            get(&db, vid, v.visit.id).await.unwrap().visit.status,
            "planned"
        );
    }

    // --- delete detaches ------------------------------------------------------

    #[tokio::test]
    async fn delete_detaches_items_then_deletes() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let a = work_item_svc::create(&db, vid, work("A")).await.unwrap();
        let v = create(&db, vid, new_visit(Some(vec![a.id]))).await.unwrap();

        delete(&db, vid, v.visit.id).await.unwrap();

        assert!(matches!(
            get(&db, vid, v.visit.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        let survivor = work_item_svc::get(&db, vid, a.id).await.unwrap();
        assert_eq!(survivor.visit_id, None);
        assert_eq!(survivor.status, "planned");
    }

    // --- list filtering ---------------------------------------------------------

    #[tokio::test]
    async fn list_hides_closed_unless_included() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let open = create(&db, vid, new_visit(None)).await.unwrap();
        let canceled = create(&db, vid, new_visit(None)).await.unwrap();
        update(
            &db,
            vid,
            canceled.visit.id,
            UpdateVisit {
                status: Some("canceled".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let visible = list(&db, vid, false).await.unwrap();
        assert_eq!(
            visible.iter().map(|v| v.visit.id).collect::<Vec<_>>(),
            vec![open.visit.id]
        );
        assert_eq!(list(&db, vid, true).await.unwrap().len(), 2);
    }

    // --- complete: the loop-closer -----------------------------------------------

    #[tokio::test]
    async fn complete_closes_every_loop() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sched = seed_overdue_schedule_item(&db, vid).await;
        let finding = seed_finding(&db, vid).await;
        let inc = seed_incident(&db, vid).await;

        // The schedule item starts overdue (2020 purchase, 12-month interval,
        // never serviced).
        let before = reminders::calculate_reminders(&db, vid).await.unwrap();
        assert_eq!(before.reminders[0].status, "overdue");

        let items = [
            NewWorkItem {
                schedule_item_id: Some(sched),
                ..work("Brake fluid flush")
            },
            NewWorkItem {
                research_finding_id: Some(finding),
                ..work("Recall: fuel pump")
            },
            NewWorkItem {
                incident_id: Some(inc),
                ..work("Fix the squeak")
            },
        ];
        let mut ids = vec![];
        for i in items {
            ids.push(work_item_svc::create(&db, vid, i).await.unwrap().id);
        }
        let v = create(&db, vid, new_visit(Some(ids.clone())))
            .await
            .unwrap();

        let done = complete(
            &db,
            vid,
            v.visit.id,
            CompleteVisit {
                paid_by: Some("insurance".into()),
                payer_note: Some("claim #1".into()),
                ..actuals()
            },
        )
        .await
        .unwrap();

        // The visit is completed and linked to the record.
        assert_eq!(done.visit.status, "completed");
        assert_eq!(
            done.visit.service_record_id,
            Some(done.service_record.record.id)
        );

        // The record: payer-aware, description defaults to the joined titles,
        // schedule link wired.
        assert_eq!(done.service_record.record.paid_by, "insurance");
        assert_eq!(
            done.service_record.record.description.as_deref(),
            Some("Brake fluid flush, Recall: fuel pump, Fix the squeak")
        );
        assert_eq!(done.service_record.schedule_item_ids, vec![sched]);
        assert_eq!(
            done.service_record.record.shop_name.as_deref(),
            Some("Joe's Garage"),
            "shop comes from the visit"
        );

        // Every item is done.
        assert_eq!(done.items.len(), 3);
        assert!(done.items.iter().all(|i| i.status == "done"));

        // The reminder cleared (schedule link → recent service).
        let after = reminders::calculate_reminders(&db, vid).await.unwrap();
        assert_eq!(after.reminders[0].status, "ok");

        // The recall finding is completed and back-links the record.
        let f = research_finding::Entity::find_by_id(finding)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(f.status, "completed");
        assert_eq!(f.linked_entity_type.as_deref(), Some("service"));
        assert_eq!(f.linked_entity_id, Some(done.service_record.record.id));

        // The incident is linked to the record.
        let links = incident_service_link::Entity::find()
            .all(&db)
            .await
            .unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].incident_id, inc);
        assert_eq!(links[0].service_record_id, done.service_record.record.id);

        // The record's mileage auto-logged (service_record::create ran for
        // real inside the nested savepoint).
        let logs = mileage_log::Entity::find()
            .filter(mileage_log::Column::VehicleId.eq(vid))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].mileage, 60_000);
    }

    #[tokio::test]
    async fn complete_wrong_vehicle_is_not_found() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let v = create(&db, owner, new_visit(None)).await.unwrap();

        assert!(matches!(
            complete(&db, other, v.visit.id, actuals())
                .await
                .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert_eq!(
            get(&db, owner, v.visit.id).await.unwrap().visit.status,
            "planned"
        );
    }

    #[tokio::test]
    async fn complete_twice_is_rejected() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let v = create(&db, vid, new_visit(None)).await.unwrap();

        complete(&db, vid, v.visit.id, actuals()).await.unwrap();
        let err = complete(&db, vid, v.visit.id, actuals()).await.unwrap_err();
        match err {
            DomainError::BadRequest(msg) => {
                assert!(msg.contains("already completed"), "{msg}");
            }
            other => panic!("expected BadRequest, got {other:?}"),
        }
        // Canceled visits are equally uncompletable.
        let c = create(&db, vid, new_visit(None)).await.unwrap();
        update(
            &db,
            vid,
            c.visit.id,
            UpdateVisit {
                status: Some("canceled".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            complete(&db, vid, c.visit.id, actuals()).await.unwrap_err(),
            DomainError::BadRequest(_)
        ));
    }

    /// Fix 1: only participating (planned/scheduled) items close out with
    /// the visit. A dropped item keeps its status, stays attached as
    /// history, and contributes nothing — no done-marking, no description
    /// join, no schedule link, no incident link, no finding completion.
    #[tokio::test]
    async fn complete_skips_dropped_items() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sched = seed_overdue_schedule_item(&db, vid).await;
        let finding = seed_finding(&db, vid).await;
        let inc = seed_incident(&db, vid).await;

        let active = work_item_svc::create(&db, vid, work("Wipers"))
            .await
            .unwrap();
        let dropped = work_item_svc::create(
            &db,
            vid,
            NewWorkItem {
                schedule_item_id: Some(sched),
                research_finding_id: Some(finding),
                incident_id: Some(inc),
                ..work("Brake fluid flush")
            },
        )
        .await
        .unwrap();
        let v = create(&db, vid, new_visit(Some(vec![active.id, dropped.id])))
            .await
            .unwrap();
        // Drop one item while attached (status edits stay legal).
        work_item_svc::update(
            &db,
            vid,
            dropped.id,
            UpdateWorkItem {
                status: Some("dropped".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let done = complete(&db, vid, v.visit.id, actuals()).await.unwrap();

        // Only the participating item closed out.
        assert_eq!(
            done.service_record.record.description.as_deref(),
            Some("Wipers")
        );
        assert!(
            done.service_record.schedule_item_ids.is_empty(),
            "dropped item's schedule item must NOT be cleared"
        );
        assert_eq!(
            work_item_svc::get(&db, vid, active.id)
                .await
                .unwrap()
                .status,
            "done"
        );

        // The dropped item is untouched: status kept, still attached.
        let d = work_item_svc::get(&db, vid, dropped.id).await.unwrap();
        assert_eq!(d.status, "dropped");
        assert_eq!(d.visit_id, Some(v.visit.id));

        // Its finding is NOT completed, its incident NOT linked, its
        // reminder still overdue.
        let f = research_finding::Entity::find_by_id(finding)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(f.status, "new");
        assert_eq!(f.linked_entity_id, None);
        assert!(
            incident_service_link::Entity::find()
                .all(&db)
                .await
                .unwrap()
                .is_empty()
        );
        let rems = reminders::calculate_reminders(&db, vid).await.unwrap();
        assert_eq!(rems.reminders[0].status, "overdue");
    }

    /// Fix 2: a completed/canceled visit is immutable — no reopen, so no
    /// reopen→double-complete path.
    #[tokio::test]
    async fn update_rejected_once_closed() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let v = create(&db, vid, new_visit(None)).await.unwrap();
        complete(&db, vid, v.visit.id, actuals()).await.unwrap();

        // Reopen attempt: rejected with the completed-guard message shape.
        let err = update(
            &db,
            vid,
            v.visit.id,
            UpdateVisit {
                status: Some("planned".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        match err {
            DomainError::BadRequest(msg) => {
                assert!(msg.contains("already completed"), "{msg}");
            }
            other => panic!("expected BadRequest, got {other:?}"),
        }

        // A second complete is still rejected; exactly ONE record exists.
        assert!(matches!(
            complete(&db, vid, v.visit.id, actuals()).await.unwrap_err(),
            DomainError::BadRequest(_)
        ));
        assert_eq!(
            service_record_entity::Entity::find()
                .all(&db)
                .await
                .unwrap()
                .len(),
            1,
            "service_record_count stays 1"
        );

        // Canceled visits are equally frozen (even innocuous edits).
        let c = create(&db, vid, new_visit(None)).await.unwrap();
        update(
            &db,
            vid,
            c.visit.id,
            UpdateVisit {
                status: Some("canceled".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let err = update(
            &db,
            vid,
            c.visit.id,
            UpdateVisit {
                notes: Some(Some("late edit".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        match err {
            DomainError::BadRequest(msg) => {
                assert!(msg.contains("already canceled"), "{msg}");
            }
            other => panic!("expected BadRequest, got {other:?}"),
        }
    }

    /// Fix 3: a done/dropped item never attaches — via visit::create or
    /// visit::update replace-all.
    #[tokio::test]
    async fn attach_rejects_non_participating_items() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let done_item = work_item_svc::create(&db, vid, work("finished"))
            .await
            .unwrap();
        work_item_svc::update(
            &db,
            vid,
            done_item.id,
            UpdateWorkItem {
                status: Some("done".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // visit::create path.
        let err = create(&db, vid, new_visit(Some(vec![done_item.id])))
            .await
            .unwrap_err();
        match err {
            DomainError::BadRequest(msg) => {
                assert!(
                    msg.contains("done") && msg.contains("cannot be attached"),
                    "{msg}"
                );
            }
            other => panic!("expected BadRequest, got {other:?}"),
        }
        assert!(
            list(&db, vid, true).await.unwrap().is_empty(),
            "rolled back"
        );

        // visit::update replace-all path.
        let v = create(&db, vid, new_visit(None)).await.unwrap();
        assert!(matches!(
            update(
                &db,
                vid,
                v.visit.id,
                UpdateVisit {
                    work_item_ids: Some(vec![done_item.id]),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err(),
            DomainError::BadRequest(_)
        ));
        let untouched = work_item_svc::get(&db, vid, done_item.id).await.unwrap();
        assert_eq!(untouched.status, "done");
        assert_eq!(untouched.visit_id, None);
    }

    /// Fix 3 (provenance): an item sitting on a COMPLETED visit never
    /// moves to another visit — its attachment is the service history.
    #[tokio::test]
    async fn attach_rejects_items_from_completed_visit() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let item = work_item_svc::create(&db, vid, work("history"))
            .await
            .unwrap();
        let v1 = create(&db, vid, new_visit(Some(vec![item.id])))
            .await
            .unwrap();
        complete(&db, vid, v1.visit.id, actuals()).await.unwrap();

        let err = create(&db, vid, new_visit(Some(vec![item.id])))
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)));
        let kept = work_item_svc::get(&db, vid, item.id).await.unwrap();
        assert_eq!(kept.visit_id, Some(v1.visit.id), "provenance intact");
        assert_eq!(kept.status, "done");
    }

    /// Fix 5: canceling via update detaches exactly like delete — items
    /// return to the backlog (visit_id NULL, scheduled → planned).
    #[tokio::test]
    async fn cancel_detaches_items() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let a = work_item_svc::create(&db, vid, work("A")).await.unwrap();
        let v = create(&db, vid, new_visit(Some(vec![a.id]))).await.unwrap();

        let canceled = update(
            &db,
            vid,
            v.visit.id,
            UpdateVisit {
                status: Some("canceled".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(canceled.visit.status, "canceled");
        assert!(canceled.items.is_empty());
        assert_eq!(canceled.est_total_cents, 0);

        let freed = work_item_svc::get(&db, vid, a.id).await.unwrap();
        assert_eq!(freed.visit_id, None);
        assert_eq!(freed.status, "planned");
    }

    /// The rollback-on-failure guarantee (and the nested-savepoint decision's
    /// empirical proof): a bad `paid_by` fails inside `service_record::create`
    /// AFTER the items were already marked done in the outer transaction —
    /// the rejection must leave NOTHING mutated.
    #[tokio::test]
    async fn complete_with_bad_payer_rolls_back_everything() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sched = seed_overdue_schedule_item(&db, vid).await;
        let finding = seed_finding(&db, vid).await;
        let inc = seed_incident(&db, vid).await;

        let a = work_item_svc::create(
            &db,
            vid,
            NewWorkItem {
                schedule_item_id: Some(sched),
                research_finding_id: Some(finding),
                incident_id: Some(inc),
                ..work("Everything")
            },
        )
        .await
        .unwrap();
        let v = create(&db, vid, new_visit(Some(vec![a.id]))).await.unwrap();

        let err = complete(
            &db,
            vid,
            v.visit.id,
            CompleteVisit {
                paid_by: Some("my neighbor".into()),
                ..actuals()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)));

        // NOTHING mutated:
        let item = work_item_svc::get(&db, vid, a.id).await.unwrap();
        assert_eq!(item.status, "scheduled", "item-done mutation rolled back");
        assert_eq!(item.visit_id, Some(v.visit.id));

        let visit_after = get(&db, vid, v.visit.id).await.unwrap();
        assert_eq!(visit_after.visit.status, "planned");
        assert_eq!(visit_after.visit.service_record_id, None);

        assert!(
            service_record_entity::Entity::find()
                .all(&db)
                .await
                .unwrap()
                .is_empty(),
            "no service record survives the rollback"
        );
        assert!(
            mileage_log::Entity::find()
                .all(&db)
                .await
                .unwrap()
                .is_empty(),
            "no auto mileage log survives the rollback"
        );
        assert!(
            incident_service_link::Entity::find()
                .all(&db)
                .await
                .unwrap()
                .is_empty()
        );
        let f = research_finding::Entity::find_by_id(finding)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(f.status, "new", "the recall finding is untouched");

        // The reminder is still overdue — nothing cleared.
        let after = reminders::calculate_reminders(&db, vid).await.unwrap();
        assert_eq!(after.reminders[0].status, "overdue");
    }
}
