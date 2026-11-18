//! The `work_item` planning primitive: "the list of things I'm actually
//! gonna do." Items are ad-hoc or sourced from a schedule item, a research
//! finding (recall), or an incident. Every source link is vehicle-scoped: a
//! cross-vehicle reference is indistinguishable from a nonexistent one.

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{
        mileage_log, research_finding, research_report,
        work_item::{self, JobStatus},
    },
    error::{DomainError, DomainResult},
    inputs::work_item::{NewWorkItem, UpdateWorkItem},
    services::{
        reminders::DueStatus,
        schedule::{DEFAULT_WARNING_DAYS, DEFAULT_WARNING_MILES},
        service_record::CompletedWork,
    },
};

/// Days before a revisit date, and miles before a revisit odometer reading,
/// at which an item counts as due soon.
const DUE_SOON_DAYS: i32 = DEFAULT_WARNING_DAYS;
const DUE_SOON_MILES: i32 = DEFAULT_WARNING_MILES;

fn validate_revisit(miles: Option<i32>) -> DomainResult<()> {
    if miles.is_some_and(|m| m < 0) {
        return Err(DomainError::invalid(
            "revisit_by_miles",
            "must not be negative",
        ));
    }
    Ok(())
}

/// How close an item's revisit-by is. It is `None` when the item has no
/// revisit date or mileage. Otherwise a reading past its revisit point is
/// overdue, one within 30 days or 1000 miles of it is due soon, and any
/// other is ok, and the more urgent of the date and mileage readings wins.
/// With no estimated mileage only the date is judged, so an item with only a
/// revisit mileage reads ok.
#[must_use]
pub fn revisit_status(
    item: &work_item::Model,
    today: chrono::NaiveDate,
    estimated_mileage: Option<i32>,
) -> Option<DueStatus> {
    if item.revisit_by_date.is_none() && item.revisit_by_miles.is_none() {
        return None;
    }
    // Only a negative remainder is overdue here; reminders count zero as
    // overdue too.
    let judge = |remaining: i64, warning: i64| {
        if remaining < 0 {
            DueStatus::Overdue
        } else if remaining <= warning {
            DueStatus::DueSoon
        } else {
            DueStatus::NotDue
        }
    };
    let by_date = item
        .revisit_by_date
        .map(|due| judge((due - today).num_days(), i64::from(DUE_SOON_DAYS)));
    let by_miles = item
        .revisit_by_miles
        .zip(estimated_mileage)
        .map(|(due, est)| judge(i64::from(due - est), i64::from(DUE_SOON_MILES)));
    Some(DueStatus::most_urgent(by_date.into_iter().chain(by_miles)))
}

/// A to-do item with its revisit reading, as `list_with_status` returns it.
#[derive(Debug, Serialize)]
pub struct PlannedItem {
    #[serde(flatten)]
    pub item: work_item::Model,
    /// See [`revisit_status`]; omitted when the item has no revisit-by.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revisit_status: Option<DueStatus>,
}

/// Verify a referenced research finding belongs to the vehicle (via its
/// report). A cross-vehicle finding must be indistinguishable from a
/// nonexistent one.
async fn require_finding_owned(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    finding_id: i32,
) -> DomainResult<()> {
    let not_found = || DomainError::NotFound(format!("Research finding {finding_id} not found"));
    let finding = research_finding::Entity::find_by_id(finding_id)
        .one(db)
        .await?
        .ok_or_else(not_found)?;
    research_report::Entity::find_by_id(finding.report_id)
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(not_found)?;
    Ok(())
}

/// Guard every present source link on a work item: schedule item (vehicle
/// scope chain), research finding (via its report), and incident, each
/// vehicle-scoped. Returns the schedule item id to store, which is the
/// vehicle's own copy when the id names an inherited item the vehicle has
/// copied.
async fn require_links_owned(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    schedule_item_id: Option<i32>,
    research_finding_id: Option<i32>,
    incident_id: Option<i32>,
) -> DomainResult<Option<i32>> {
    let tracked = match schedule_item_id {
        Some(id) => crate::services::schedule::tracked_ids(db, vehicle_id, &[id])
            .await?
            .first()
            .copied(),
        None => None,
    };
    if let Some(id) = research_finding_id {
        require_finding_owned(db, vehicle_id, id).await?;
    }
    if let Some(id) = incident_id {
        crate::services::incident::require_incident(db, vehicle_id, id).await?;
    }
    Ok(tracked)
}

pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    include_done: bool,
) -> DomainResult<Vec<work_item::Model>> {
    crate::services::vehicle::require(db, vehicle_id).await?;
    let mut query = work_item::Entity::find().filter(work_item::Column::VehicleId.eq(vehicle_id));
    if !include_done {
        query = query
            .filter(work_item::Column::Status.is_not_in([JobStatus::Done, JobStatus::Dropped]));
    }
    Ok(query
        .order_by_desc(work_item::Column::CreatedAt)
        .order_by_desc(work_item::Column::Id)
        .all(db)
        .await?)
}

/// Lists a vehicle's to-do items, each with its revisit status judged against
/// `today` and the estimated odometer. A vehicle with no mileage data is
/// judged on dates alone.
pub async fn list_with_status(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    include_done: bool,
    today: chrono::NaiveDate,
) -> DomainResult<Vec<PlannedItem>> {
    let items = list(db, vehicle_id, include_done).await?;
    let estimated = if items.iter().any(|i| i.revisit_by_miles.is_some()) {
        let v = crate::services::vehicle::require(db, vehicle_id).await?;
        let has_readings = mileage_log::Entity::find()
            .filter(mileage_log::Column::VehicleId.eq(vehicle_id))
            .count(db)
            .await?
            > 0;
        if has_readings || v.purchase_mileage.is_some() {
            Some(
                crate::services::reminders::estimate_mileage(db, vehicle_id, &v, today)
                    .await?
                    .0,
            )
        } else {
            None
        }
    } else {
        None
    };
    Ok(items
        .into_iter()
        .map(|item| PlannedItem {
            revisit_status: revisit_status(&item, today, estimated),
            item,
        })
        .collect())
}

/// Marks the named open items of a vehicle as done by a service record, and
/// links each item's schedule item to that service so its reminder clears.
/// Runs inside the caller's transaction. An id from another vehicle reads as
/// nonexistent; an item that is already done or dropped is a `BadRequest`.
pub(crate) async fn complete_with_service(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    ids: &[i32],
    service_record_id: i32,
) -> DomainResult<Vec<work_item::Model>> {
    let mut completed = Vec::new();
    for id in super::dedup_ids(ids) {
        let existing = get(db, vehicle_id, id).await?;
        if existing.status != JobStatus::Planned {
            return Err(DomainError::BadRequest(format!(
                "Work item {id} is already {}",
                existing.status
            )));
        }
        let schedule_item_id = existing.schedule_item_id;
        let mut active: work_item::ActiveModel = existing.into();
        active.status = Set(JobStatus::Done);
        active.completed_service_id = Set(Some(service_record_id));
        active.updated_at = Set(super::now());
        completed.push(active.update(db).await?);

        // The stored schedule item id was resolved when the work item was
        // filed, so it is linked as is.
        if let Some(schedule_item_id) = schedule_item_id {
            super::schedule::insert_service_links(
                db,
                service_record_id,
                &[schedule_item_id],
                super::schedule::LinkMode::Add,
            )
            .await?;
        }
    }
    Ok(completed)
}

/// A to-do item as seen from the service that completed it.
#[derive(Clone, Debug, Serialize)]
pub struct WorkItemRef {
    pub id: i32,
    pub title: String,
}

/// The to-do items each of the given services completed, keyed by service id
/// and ordered by item id, in one query.
pub(crate) async fn load_completed_by(
    db: &impl ConnectionTrait,
    service_ids: &[i32],
) -> DomainResult<std::collections::HashMap<i32, Vec<WorkItemRef>>> {
    let mut out: std::collections::HashMap<i32, Vec<WorkItemRef>> =
        std::collections::HashMap::new();
    if service_ids.is_empty() {
        return Ok(out);
    }
    let items = work_item::Entity::find()
        .filter(work_item::Column::CompletedServiceId.is_in(service_ids.to_vec()))
        .order_by_asc(work_item::Column::Id)
        .all(db)
        .await?;
    for item in items {
        if let Some(service_id) = item.completed_service_id {
            out.entry(service_id).or_default().push(WorkItemRef {
                id: item.id,
                title: item.title,
            });
        }
    }
    Ok(out)
}

/// Releases jobs a service no longer completes, because it is about to be
/// deleted or a correction took them off it, on the caller's transaction. After `did_not_happen` they return to `planned`,
/// which reopens a hand-closed appointment or takes a job off a cancelled one.
/// After `done_another_way` they stay done with no completing service.
pub(crate) async fn release_completed(
    db: &impl ConnectionTrait,
    job_ids: &[i32],
    completed: CompletedWork,
) -> DomainResult<super::appointment::SettledJobs> {
    if job_ids.is_empty() {
        return Ok(super::appointment::SettledJobs::default());
    }
    let status = match completed {
        CompletedWork::DidNotHappen => Set(JobStatus::Planned),
        CompletedWork::DoneAnotherWay => NotSet,
    };
    work_item::Entity::update_many()
        .set(work_item::ActiveModel {
            status,
            completed_service_id: Set(None),
            updated_at: Set(super::now()),
            ..Default::default()
        })
        .filter(work_item::Column::Id.is_in(job_ids.to_vec()))
        .exec(db)
        .await?;
    if completed == CompletedWork::DoneAnotherWay {
        return Ok(super::appointment::SettledJobs::default());
    }
    super::appointment::settle_reopened_jobs(db, job_ids).await
}

pub async fn get(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<work_item::Model> {
    work_item::Entity::find_by_id(id)
        .filter(work_item::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Work item {id} not found")))
}

pub async fn create<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    input: NewWorkItem,
) -> DomainResult<work_item::Model> {
    if input.title.trim().is_empty() {
        return Err(DomainError::invalid("title", "must not be blank"));
    }
    validate_revisit(input.revisit_by_miles)?;
    let txn = super::begin_write(db).await?;
    let db = &txn;
    if let Some(id) = input.appointment_id {
        crate::services::appointment::require_assignable(db, vehicle_id, id).await?;
    }
    crate::services::vehicle::require(db, vehicle_id).await?;
    let schedule_item_id = require_links_owned(
        db,
        vehicle_id,
        input.schedule_item_id,
        input.research_finding_id,
        input.incident_id,
    )
    .await?;
    let model = work_item::ActiveModel {
        vehicle_id: Set(vehicle_id),
        title: Set(input.title),
        notes: Set(input.notes),
        schedule_item_id: Set(schedule_item_id),
        research_finding_id: Set(input.research_finding_id),
        incident_id: Set(input.incident_id),
        est_cost_cents: Set(input.est_cost_cents),
        revisit_by_date: Set(input.revisit_by_date),
        revisit_by_miles: Set(input.revisit_by_miles),
        appointment_id: Set(input.appointment_id),
        status: Set(JobStatus::Planned),
        ..Default::default()
    };
    let created = model.insert(db).await?;
    txn.commit().await?;
    Ok(created)
}

/// A work item after [`update`], with what its return to `planned` did to
/// its appointment. Each id is present only when the update did that.
#[derive(Debug, Serialize)]
pub struct UpdatedWorkItem {
    #[serde(flatten)]
    pub work_item: work_item::Model,
    /// The hand-closed appointment the item is on, which is open again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reopened_appointment_id: Option<i32>,
    /// The cancelled appointment the item came off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left_cancelled_appointment_id: Option<i32>,
}

/// Changes a work item. Setting a done or dropped item back to `planned`
/// settles its appointment as `appointment::settle_reopened_jobs` describes,
/// in the same transaction, and the result names the appointment it changed.
pub async fn update<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    input: UpdateWorkItem,
) -> DomainResult<UpdatedWorkItem> {
    let txn = super::begin_write(db).await?;
    let db = &txn;
    let existing = get(db, vehicle_id, id).await?;
    let reopens = existing.status != JobStatus::Planned && input.status == Some(JobStatus::Planned);

    if let Some(title) = &input.title
        && title.trim().is_empty()
    {
        return Err(DomainError::invalid("title", "must not be blank"));
    }
    validate_revisit(input.revisit_by_miles.flatten())?;
    // Same guards as create, on the links this update actually changes.
    let schedule_item_id = require_links_owned(
        db,
        vehicle_id,
        input.schedule_item_id.flatten(),
        input.research_finding_id.flatten(),
        input.incident_id.flatten(),
    )
    .await?;
    // Keeping the appointment a job is already on is not a new assignment, so
    // it stays allowed after the appointment is closed or cancelled.
    if let Some(Some(id)) = input.appointment_id
        && existing.appointment_id != Some(id)
    {
        crate::services::appointment::require_assignable(db, vehicle_id, id).await?;
    }

    let mut active: work_item::ActiveModel = existing.into();

    if let Some(v) = input.title {
        active.title = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }
    if let Some(v) = input.schedule_item_id {
        active.schedule_item_id = Set(v.and(schedule_item_id));
    }
    if let Some(v) = input.research_finding_id {
        active.research_finding_id = Set(v);
    }
    if let Some(v) = input.incident_id {
        active.incident_id = Set(v);
    }
    if let Some(v) = input.est_cost_cents {
        active.est_cost_cents = Set(v);
    }
    if let Some(v) = input.revisit_by_date {
        active.revisit_by_date = Set(v);
    }
    if let Some(v) = input.revisit_by_miles {
        active.revisit_by_miles = Set(v);
    }
    if let Some(v) = input.appointment_id {
        active.appointment_id = Set(v);
    }
    if let Some(v) = input.status {
        // A completing service link is kept only while the item is done.
        if v != JobStatus::Done {
            active.completed_service_id = Set(None);
        }
        active.status = Set(v);
    }

    active.updated_at = Set(super::now());
    let updated = active.update(db).await?;
    let mut out = UpdatedWorkItem {
        reopened_appointment_id: None,
        left_cancelled_appointment_id: None,
        work_item: updated,
    };
    if reopens {
        let settled = super::appointment::settle_reopened_jobs(db, &[id]).await?;
        if settled.unassigned_job_ids.contains(&id) {
            out.left_cancelled_appointment_id = out.work_item.appointment_id;
            // Settling took the job off the appointment, so the row is read
            // again and the caller sees where the job ended up.
            out.work_item = get(db, vehicle_id, id).await?;
        }
        out.reopened_appointment_id = settled.reopened_appointment_ids.first().copied();
    }
    txn.commit().await?;
    Ok(out)
}

pub async fn delete<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<()> {
    let txn = super::begin_write(db).await?;
    let db = &txn;
    let existing = get(db, vehicle_id, id).await?;
    existing.delete(db).await?;
    txn.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::{
            incident, incident::IncidentCategory, maintenance_schedule_item, mileage_log, part,
            service_record, service_record_line_item, service_record_line_item::WorkKind,
            service_schedule_link,
        },
        inputs::{
            part::NewPart,
            service_record::{NewLineItem, NewServiceRecord},
        },
        services::{reminders, service_record as service_svc},
        test_support::{VehicleFixture, date, row_count, test_db},
    };
    use chrono::NaiveDate;

    async fn seed_vehicle(db: &impl ConnectionTrait) -> i32 {
        VehicleFixture::new().insert_id(db).await
    }

    // Job edits racing on one file from several pools all succeed: each opens
    // with the write lock, which a write to an FTS-indexed table needs.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_edits_of_one_job_all_succeed() {
        let (_dir, pools) = crate::test_support::race_pools().await;
        let first = &pools[0];
        let v = seed_vehicle(first).await;
        let job = create(
            first,
            v,
            NewWorkItem {
                title: "Brakes".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = crate::test_support::race(&pools, |i, db| async move {
            let input = UpdateWorkItem {
                notes: Some(Some(format!("note {i}"))),
                ..Default::default()
            };
            update(&db, v, job.id, input).await.map(|_| ())
        })
        .await;

        assert!(results.iter().all(Result::is_ok), "{results:?}");
    }

    async fn seed_schedule_item(db: &impl ConnectionTrait, vehicle_id: i32) -> i32 {
        use crate::entities::maintenance_schedule_item;
        maintenance_schedule_item::ActiveModel {
            vehicle_id: Set(Some(vehicle_id)),
            name: Set("Oil change".into()),
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
            title: Set("Recall".into()),
            status: Set("new".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn seed_incident(db: &impl ConnectionTrait, vehicle_id: i32) -> i32 {
        incident::ActiveModel {
            vehicle_id: Set(vehicle_id),
            occurred_at: Set(crate::dates::local_now()),
            category: Set(IncidentCategory::Noise),
            title: Set("Squeak".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    fn item(title: &str) -> NewWorkItem {
        NewWorkItem {
            title: title.into(),
            notes: None,
            schedule_item_id: None,
            research_finding_id: None,
            incident_id: None,
            est_cost_cents: None,
            revisit_by_date: None,
            revisit_by_miles: None,
            appointment_id: None,
        }
    }

    // Each source id is a foreign key that clears when its row is deleted, so
    // a job never names a schedule item, finding or incident that is gone.
    #[tokio::test]
    async fn deleting_a_jobs_source_clears_the_link_and_keeps_the_job() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let (sched, finding, inc) = (
            seed_schedule_item(&db, vid).await,
            seed_finding(&db, vid).await,
            seed_incident(&db, vid).await,
        );
        let job = create(
            &db,
            vid,
            NewWorkItem {
                schedule_item_id: Some(sched),
                research_finding_id: Some(finding),
                incident_id: Some(inc),
                ..item("Everything at once")
            },
        )
        .await
        .unwrap();
        let sources = async || {
            let job = get(&db, vid, job.id).await.unwrap();
            (
                job.schedule_item_id,
                job.research_finding_id,
                job.incident_id,
            )
        };

        crate::services::schedule::delete(&db, sched).await.unwrap();
        assert_eq!(sources().await, (None, Some(finding), Some(inc)));

        research_finding::Entity::delete_by_id(finding)
            .exec(&db)
            .await
            .unwrap();
        assert_eq!(sources().await, (None, None, Some(inc)));

        crate::services::incident::delete(&db, vid, inc)
            .await
            .unwrap();
        assert_eq!(sources().await, (None, None, None));
        assert_eq!(
            get(&db, vid, job.id).await.unwrap().title,
            "Everything at once"
        );
    }

    fn with_revisit(date: Option<&str>, miles: Option<i32>) -> work_item::Model {
        work_item::Model {
            id: 1,
            vehicle_id: 1,
            title: "t".into(),
            notes: None,
            schedule_item_id: None,
            research_finding_id: None,
            incident_id: None,
            est_cost_cents: None,
            status: JobStatus::Planned,
            created_at: chrono::DateTime::default(),
            updated_at: chrono::DateTime::default(),
            revisit_by_date: date.map(crate::test_support::date),
            revisit_by_miles: miles,
            completed_service_id: None,
            appointment_id: None,
        }
    }

    fn service_on(date: &str, work_item_ids: Vec<i32>) -> NewServiceRecord {
        NewServiceRecord {
            service_date: crate::test_support::date(date),
            work_item_ids: Some(work_item_ids),
            ..Default::default()
        }
    }

    async fn reminder_status(db: &DatabaseConnection, vehicle_id: i32, name: &str) -> DueStatus {
        reminders::calculate_reminders(db, vehicle_id)
            .await
            .unwrap()
            .reminders
            .into_iter()
            .find(|r| r.schedule_item.name == name)
            .expect("reminder present")
            .status
    }

    #[test]
    fn revisit_status_takes_the_worse_of_date_and_mileage() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        assert_eq!(
            revisit_status(&with_revisit(None, None), today, Some(64000)),
            None
        );
        assert_eq!(
            revisit_status(&with_revisit(Some("2026-10-01"), None), today, None),
            Some(DueStatus::Overdue)
        );
        assert_eq!(
            revisit_status(&with_revisit(Some("2026-12-01"), None), today, None),
            Some(DueStatus::DueSoon)
        );
        assert_eq!(
            revisit_status(&with_revisit(Some("2027-06-01"), None), today, None),
            Some(DueStatus::NotDue)
        );
        assert_eq!(
            revisit_status(
                &with_revisit(Some("2027-06-01"), Some(64500)),
                today,
                Some(64000)
            ),
            Some(DueStatus::DueSoon)
        );
        assert_eq!(
            revisit_status(&with_revisit(None, Some(60000)), today, Some(64000)),
            Some(DueStatus::Overdue)
        );
        assert_eq!(
            revisit_status(&with_revisit(None, Some(60000)), today, None),
            Some(DueStatus::NotDue)
        );
    }

    #[test]
    fn revisit_status_boundaries() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        let by_date = |d: &str| revisit_status(&with_revisit(Some(d), None), today, None);
        assert_eq!(by_date("2026-11-12"), Some(DueStatus::DueSoon));
        assert_eq!(by_date("2026-12-12"), Some(DueStatus::DueSoon));
        assert_eq!(by_date("2026-12-13"), Some(DueStatus::NotDue));
        assert_eq!(by_date("2026-11-11"), Some(DueStatus::Overdue));

        let by_miles =
            |est: i32| revisit_status(&with_revisit(None, Some(64000)), today, Some(est));
        assert_eq!(by_miles(64000), Some(DueStatus::DueSoon));
        assert_eq!(by_miles(64001), Some(DueStatus::Overdue));
        assert_eq!(by_miles(63000), Some(DueStatus::DueSoon));
        assert_eq!(by_miles(62999), Some(DueStatus::NotDue));
    }

    #[tokio::test]
    async fn a_service_completes_the_work_items_it_names_and_clears_their_reminders() {
        let db = test_db().await;
        let v = VehicleFixture::new()
            .purchase_date("2020-01-01")
            .insert_id(&db)
            .await;
        let sched = maintenance_schedule_item::ActiveModel {
            vehicle_id: Set(Some(v)),
            name: Set("Brake fluid flush".into()),
            interval_months: Set(Some(12)),
            enabled: Set(true),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        assert_eq!(
            reminder_status(&db, v, "Brake fluid flush").await,
            DueStatus::Overdue
        );
        let planned = create(
            &db,
            v,
            NewWorkItem {
                schedule_item_id: Some(sched),
                ..item("Flush brake fluid")
            },
        )
        .await
        .unwrap();

        let today = crate::services::today().format("%Y-%m-%d").to_string();
        let svc = service_svc::create(&db, v, service_on(&today, vec![planned.id]))
            .await
            .unwrap();

        let item_after = get(&db, v, planned.id).await.unwrap();
        assert_eq!(item_after.status, JobStatus::Done);
        assert_eq!(item_after.completed_service_id, Some(svc.record.id));
        assert_eq!(
            reminder_status(&db, v, "Brake fluid flush").await,
            DueStatus::NotDue
        );
    }

    #[tokio::test]
    async fn a_work_item_from_another_vehicle_is_not_found_and_nothing_commits() {
        let db = test_db().await;
        let v1 = seed_vehicle(&db).await;
        let v2 = seed_vehicle(&db).await;
        let foreign = create(&db, v2, item("theirs")).await.unwrap();

        let sched = seed_schedule_item(&db, v1).await;
        let full = NewServiceRecord {
            mileage: Some(50_000),
            line_items: Some(vec![NewLineItem {
                paid_by: None,
                description: "Oil".into(),
                work_kind: WorkKind::Maintenance,
                quantity: None,
                unit_cost_cents: None,
                cost_cents: None,
            }]),
            parts: Some(vec![NewPart {
                name: "Filter".into(),
                ..Default::default()
            }]),
            schedule_item_ids: Some(vec![sched]),
            ..service_on("2026-11-12", vec![foreign.id])
        };
        let err = service_svc::create(&db, v1, full).await.unwrap_err();
        let missing = service_svc::create(&db, v1, service_on("2026-11-12", vec![9999]))
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)));
        assert_eq!(
            err.to_string().replace(&foreign.id.to_string(), "N"),
            missing.to_string().replace("9999", "N")
        );
        assert_eq!(row_count::<service_record::Entity>(&db).await, 0);
        assert_eq!(row_count::<service_record_line_item::Entity>(&db).await, 0);
        assert_eq!(row_count::<part::Entity>(&db).await, 0);
        assert_eq!(row_count::<mileage_log::Entity>(&db).await, 0);
        assert_eq!(row_count::<service_schedule_link::Entity>(&db).await, 0);
        assert_eq!(
            get(&db, v2, foreign.id).await.unwrap().status,
            JobStatus::Planned
        );
    }

    #[tokio::test]
    async fn a_finished_work_item_cannot_be_completed_again() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let planned = create(&db, v, item("once")).await.unwrap();
        service_svc::create(&db, v, service_on("2026-11-12", vec![planned.id]))
            .await
            .unwrap();

        let err = service_svc::create(&db, v, service_on("2026-11-13", vec![planned.id]))
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "got {err:?}");
        assert_eq!(row_count::<service_record::Entity>(&db).await, 1);
    }

    #[tokio::test]
    async fn revisit_fields_round_trip_validate_and_clear() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let created = create(
            &db,
            v,
            NewWorkItem {
                revisit_by_date: Some(date("2027-05-01")),
                revisit_by_miles: Some(70_000),
                ..item("watch the seep")
            },
        )
        .await
        .unwrap();
        assert_eq!(created.revisit_by_date, Some(date("2027-05-01")));
        assert_eq!(created.revisit_by_miles, Some(70_000));

        let cleared = update(
            &db,
            v,
            created.id,
            UpdateWorkItem {
                revisit_by_date: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.work_item.revisit_by_date, None);
        assert_eq!(cleared.work_item.revisit_by_miles, Some(70_000));
    }

    #[tokio::test]
    async fn planned_items_get_a_revisit_status_even_without_mileage_data() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        create(
            &db,
            v,
            NewWorkItem {
                revisit_by_date: Some(date("2000-01-01")),
                revisit_by_miles: Some(10),
                ..item("late")
            },
        )
        .await
        .unwrap();
        create(&db, v, item("plain")).await.unwrap();
        create(
            &db,
            v,
            NewWorkItem {
                revisit_by_miles: Some(10),
                ..item("miles only")
            },
        )
        .await
        .unwrap();

        let listed = list_with_status(&db, v, false, crate::services::today())
            .await
            .unwrap();
        let by_title = |t: &str| listed.iter().find(|p| p.item.title == t).unwrap();
        assert_eq!(by_title("late").revisit_status, Some(DueStatus::Overdue));
        assert_eq!(by_title("plain").revisit_status, None);
        // With no odometer reading and no purchase mileage, a miles-only
        // revisit cannot be judged yet and reads `ok`.
        assert_eq!(
            by_title("miles only").revisit_status,
            Some(DueStatus::NotDue)
        );
    }

    #[tokio::test]
    async fn create_defaults_planned_and_round_trips() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = create(
            &db,
            vid,
            NewWorkItem {
                notes: Some("front axle only".into()),
                est_cost_cents: Some(12_500),
                ..item("Replace front brakes")
            },
        )
        .await
        .unwrap();
        assert_eq!(created.status, JobStatus::Planned);
        assert_eq!(created.est_cost_cents, Some(12_500));

        let fetched = get(&db, vid, created.id).await.unwrap();
        assert_eq!(fetched.title, "Replace front brakes");
        assert_eq!(fetched.notes.as_deref(), Some("front axle only"));

        let listed = list(&db, vid, false).await.unwrap();
        assert_eq!(listed.len(), 1);
    }

    #[tokio::test]
    async fn create_rejects_blank_title() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        assert!(matches!(
            create(&db, vid, item("   ")).await.unwrap_err(),
            DomainError::Invalid { field, .. } if field == "title"
        ));
        crate::test_support::assert_empty(&list(&db, vid, true).await.unwrap());
    }

    #[tokio::test]
    async fn create_missing_vehicle_is_not_found() {
        let db = test_db().await;
        assert!(matches!(
            create(&db, 999, item("x")).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    // --- the source-link wrong-vehicle probes -----------------------------

    #[tokio::test]
    async fn create_rejects_other_vehicles_schedule_item() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign = seed_schedule_item(&db, other).await;
        assert!(matches!(
            create(
                &db,
                owner,
                NewWorkItem {
                    schedule_item_id: Some(foreign),
                    ..item("t")
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        crate::test_support::assert_empty(&list(&db, owner, true).await.unwrap());
    }

    #[tokio::test]
    async fn create_rejects_other_vehicles_research_finding() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign = seed_finding(&db, other).await;
        assert!(matches!(
            create(
                &db,
                owner,
                NewWorkItem {
                    research_finding_id: Some(foreign),
                    ..item("t")
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        crate::test_support::assert_empty(&list(&db, owner, true).await.unwrap());
    }

    #[tokio::test]
    async fn create_rejects_other_vehicles_incident() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign = seed_incident(&db, other).await;
        assert!(matches!(
            create(
                &db,
                owner,
                NewWorkItem {
                    incident_id: Some(foreign),
                    ..item("t")
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        crate::test_support::assert_empty(&list(&db, owner, true).await.unwrap());
    }

    #[tokio::test]
    async fn update_rejects_other_vehicles_links_and_mutates_nothing() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign_incident = seed_incident(&db, other).await;
        let created = create(&db, owner, item("t")).await.unwrap();

        assert!(matches!(
            update(
                &db,
                owner,
                created.id,
                UpdateWorkItem {
                    incident_id: Some(Some(foreign_incident)),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert_eq!(get(&db, owner, created.id).await.unwrap().incident_id, None);
    }

    #[tokio::test]
    async fn happy_links_round_trip() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sched = seed_schedule_item(&db, vid).await;
        let finding = seed_finding(&db, vid).await;
        let inc = seed_incident(&db, vid).await;

        let created = create(
            &db,
            vid,
            NewWorkItem {
                schedule_item_id: Some(sched),
                research_finding_id: Some(finding),
                incident_id: Some(inc),
                ..item("Everything sourced")
            },
        )
        .await
        .unwrap();
        assert_eq!(created.schedule_item_id, Some(sched));
        assert_eq!(created.research_finding_id, Some(finding));
        assert_eq!(created.incident_id, Some(inc));
    }

    #[tokio::test]
    async fn update_sets_a_status_and_clears_a_double_option() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = create(
            &db,
            vid,
            NewWorkItem {
                est_cost_cents: Some(5_000),
                ..item("t")
            },
        )
        .await
        .unwrap();

        // An explicit null clears est_cost_cents.
        let updated = update(
            &db,
            vid,
            created.id,
            UpdateWorkItem {
                status: Some(JobStatus::Done),
                est_cost_cents: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.work_item.status, JobStatus::Done);
        assert_eq!(updated.work_item.est_cost_cents, None);
    }

    #[tokio::test]
    async fn list_hides_done_and_dropped_unless_included() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let a = create(&db, vid, item("open")).await.unwrap();
        let b = create(&db, vid, item("finished")).await.unwrap();
        let c = create(&db, vid, item("abandoned")).await.unwrap();
        for (id, status) in [(b.id, JobStatus::Done), (c.id, JobStatus::Dropped)] {
            update(
                &db,
                vid,
                id,
                UpdateWorkItem {
                    status: Some(status),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }

        let open = list(&db, vid, false).await.unwrap();
        assert_eq!(
            open.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![a.id],
            "done/dropped hidden by default"
        );
        assert_eq!(list(&db, vid, true).await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn get_and_delete_are_vehicle_scoped() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let created = create(&db, owner, item("mine")).await.unwrap();

        assert!(matches!(
            get(&db, other, created.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(matches!(
            delete(&db, other, created.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));

        delete(&db, owner, created.id).await.unwrap();
        assert!(matches!(
            get(&db, owner, created.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn moving_a_completed_item_off_done_clears_its_completing_service() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let created = create(&db, v, item("Do it")).await.unwrap();
        let svc = crate::services::service_record::create(
            &db,
            v,
            NewServiceRecord {
                service_date: date("2024-01-01"),
                work_item_ids: Some(vec![created.id]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            get(&db, v, created.id).await.unwrap().completed_service_id,
            Some(svc.record.id)
        );

        let reopened = update(
            &db,
            v,
            created.id,
            UpdateWorkItem {
                status: Some(JobStatus::Planned),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(reopened.work_item.status, JobStatus::Planned);
        assert_eq!(reopened.work_item.completed_service_id, None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn racing_deletes_all_succeed() {
        let (_dir, pools) = crate::test_support::race_pools().await;
        let first = &pools[0];
        let v = seed_vehicle(first).await;
        let mut ids = Vec::new();
        for i in 0..pools.len() * 2 {
            ids.push(
                create(first, v, item(&format!("Job {i}")))
                    .await
                    .unwrap()
                    .id,
            );
        }
        let results = crate::test_support::race(&pools, |i, db| {
            let id = ids[i];
            async move { delete(&db, v, id).await }
        })
        .await;
        assert!(results.iter().all(Result::is_ok), "{results:?}");
    }
}
