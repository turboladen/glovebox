//! Appointments: a visit that groups jobs and to-dos. Every function scopes
//! its ids to a vehicle, so another vehicle's id reads as a missing one.

use std::collections::HashMap;

use sea_orm::{sea_query::Expr, *};
use serde::Serialize;

use crate::{
    entities::{appointment, job_need, part, todo, work_item},
    error::{DomainError, DomainResult},
    inputs::appointment::{NewAppointment, StatusChange, UpdateAppointment},
    services::need::{counts_as_open, stage},
};

/// An appointment with the counts the owner reads at a glance. `needs_open`
/// counts the needs on its planned jobs that are undecided, waiting to be
/// ordered or ordered but not yet received; `todos_open` counts its
/// unfinished to-dos.
#[derive(Debug, Serialize)]
pub struct AppointmentSummary {
    #[serde(flatten)]
    pub appointment: appointment::Model,
    pub status: &'static str,
    /// True when the appointment has a planned date.
    pub booked: bool,
    pub jobs: usize,
    pub needs_open: usize,
    pub todos_open: usize,
}

/// What closing an appointment did.
#[derive(Debug, Serialize)]
pub struct ClosedAppointment {
    pub appointment: appointment::Model,
    /// The jobs that were still planned and are now off the appointment.
    pub unassigned_job_ids: Vec<i32>,
}

/// An appointment after edits and a status change, with the jobs that closing
/// or cancelling took off it.
#[derive(Debug, Serialize)]
pub struct RevisedAppointment {
    pub appointment: AppointmentSummary,
    pub unassigned_job_ids: Vec<i32>,
}

// The shop directory is global, so an id is checked for existence only. A
// missing shop would otherwise surface as an opaque foreign-key failure.
async fn validate_shop(db: &impl ConnectionTrait, shop_id: Option<i32>) -> DomainResult<()> {
    if let Some(id) = shop_id {
        super::shop::get(db, id).await?;
    }
    Ok(())
}

/// How an appointment was put away by hand, read from its dates. A cancelled
/// one counts as cancelled even when it also carries a closed date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    Closed,
    Cancelled,
}

impl Ending {
    fn of(a: &appointment::Model) -> Option<Self> {
        if a.cancelled_on.is_some() {
            Some(Self::Cancelled)
        } else if a.closed_on.is_some() {
            Some(Self::Closed)
        } else {
            None
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// The appointment's state from its dates and its jobs' statuses: `cancelled`
/// when cancelled, `done` when closed by hand or when it has jobs and every
/// one is done or dropped, otherwise `planning`.
#[must_use]
pub fn status(a: &appointment::Model, job_statuses: &[&str]) -> &'static str {
    match Ending::of(a) {
        Some(Ending::Cancelled) => "cancelled",
        Some(Ending::Closed) => "done",
        None if !job_statuses.is_empty()
            && job_statuses
                .iter()
                .all(|s| matches!(*s, "done" | "dropped")) =>
        {
            "done"
        }
        None => "planning",
    }
}

/// Creates an appointment for a vehicle.
///
/// # Errors
/// `NotFound` for a vehicle or shop that does not exist; `Invalid` for a
/// blank purpose or a malformed date.
pub async fn create(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    input: NewAppointment,
) -> DomainResult<appointment::Model> {
    super::validate_not_blank("purpose", &input.purpose)?;
    super::validate_optional_date("planned_date", input.planned_date.as_deref())?;
    super::vehicle::require(db, vehicle_id).await?;
    validate_shop(db, input.shop_id).await?;
    let model = appointment::ActiveModel {
        vehicle_id: Set(vehicle_id),
        purpose: Set(input.purpose),
        shop_name: Set(input.shop_name),
        shop_id: Set(input.shop_id),
        planned_date: Set(input.planned_date),
        notes: Set(input.notes),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

/// Changes an appointment's purpose, shop, date or notes.
///
/// # Errors
/// `NotFound` for an appointment that is not the vehicle's or a missing shop;
/// `Invalid` as for [`create`].
pub async fn update(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
    input: UpdateAppointment,
) -> DomainResult<appointment::Model> {
    let existing = require(db, vehicle_id, id).await?;
    if let Some(purpose) = &input.purpose {
        super::validate_not_blank("purpose", purpose)?;
    }
    super::validate_optional_date(
        "planned_date",
        input.planned_date.as_ref().and_then(|d| d.as_deref()),
    )?;
    validate_shop(db, input.shop_id.flatten()).await?;
    let mut active: appointment::ActiveModel = existing.into();
    if let Some(v) = input.purpose {
        active.purpose = Set(v);
    }
    if let Some(v) = input.shop_name {
        active.shop_name = Set(v);
    }
    if let Some(v) = input.shop_id {
        active.shop_id = Set(v);
    }
    if let Some(v) = input.planned_date {
        active.planned_date = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }
    active.updated_at = Set(super::now_stamp());
    Ok(active.update(db).await?)
}

/// Edits an appointment and, when `change` is present, closes, cancels or
/// reopens it, in one transaction. A refused status change leaves the edits
/// unapplied.
///
/// # Errors
/// As for [`update`], and as for [`close`] or [`reopen`] for the status change.
pub async fn update_with_status<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    input: UpdateAppointment,
    change: Option<StatusChange>,
) -> DomainResult<RevisedAppointment> {
    let txn = db.begin().await?;
    update(&txn, vehicle_id, id, input).await?;
    let unassigned_job_ids = match change {
        Some(StatusChange::Close) => {
            put_away(&txn, vehicle_id, id, Ending::Closed)
                .await?
                .unassigned_job_ids
        }
        Some(StatusChange::Cancel) => {
            put_away(&txn, vehicle_id, id, Ending::Cancelled)
                .await?
                .unassigned_job_ids
        }
        Some(StatusChange::Reopen) => {
            reopen(&txn, vehicle_id, id).await?;
            Vec::new()
        }
        None => Vec::new(),
    };
    let appointment = summary(&txn, vehicle_id, id).await?;
    txn.commit().await?;
    Ok(RevisedAppointment {
        appointment,
        unassigned_job_ids,
    })
}

/// Loads an appointment that belongs to `vehicle_id`.
///
/// # Errors
/// `NotFound` when it does not exist or belongs to another vehicle.
pub async fn require(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<appointment::Model> {
    appointment::Entity::find_by_id(id)
        .filter(appointment::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Appointment {id} not found")))
}

/// [`require`] for an appointment that is about to receive a job or an invoice:
/// a cancelled or hand-closed appointment is refused.
pub(crate) async fn require_assignable(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<appointment::Model> {
    let found = require(db, vehicle_id, id).await?;
    if let Some(ending) = Ending::of(&found) {
        return Err(DomainError::BadRequest(format!(
            "Appointment {id} is {}; reopen it first",
            ending.label()
        )));
    }
    Ok(found)
}

/// Returns `id` when it names an appointment that is neither closed by hand
/// nor cancelled, and `None` otherwise. A record filed for the owner to act
/// on goes on an appointment only while a list still shows that appointment.
pub(crate) async fn open_id(
    db: &impl ConnectionTrait,
    id: Option<i32>,
) -> DomainResult<Option<i32>> {
    let Some(id) = id else {
        return Ok(None);
    };
    let found = appointment::Entity::find_by_id(id).one(db).await?;
    Ok(found.filter(|a| Ending::of(a).is_none()).map(|a| a.id))
}

/// Lists a vehicle's appointments with their counts, soonest planned date
/// first and unbooked ones last. Without `include_closed`, appointments whose
/// status is `done` or `cancelled` are left out, except a `done` one that
/// still has an open to-do and was not closed by hand.
pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    include_closed: bool,
) -> DomainResult<Vec<AppointmentSummary>> {
    super::vehicle::require(db, vehicle_id).await?;
    let mut out = summaries(db, &[vehicle_id], None).await?;
    if !include_closed {
        out.retain(shows_as_open);
    }
    Ok(out)
}

/// Loads the appointments that [`list`] gives without closed ones, for every
/// vehicle in `vehicle_ids` at once, keyed by vehicle and in list order.
pub(crate) async fn open_by_vehicle(
    db: &impl ConnectionTrait,
    vehicle_ids: &[i32],
) -> DomainResult<HashMap<i32, Vec<AppointmentSummary>>> {
    let mut grouped: HashMap<i32, Vec<AppointmentSummary>> = HashMap::new();
    for summary in summaries(db, vehicle_ids, None).await? {
        if shows_as_open(&summary) {
            grouped
                .entry(summary.appointment.vehicle_id)
                .or_default()
                .push(summary);
        }
    }
    Ok(grouped)
}

// A derived `done` appointment with a to-do left still has work to show; a
// hand-closed or cancelled one has been put away on purpose.
fn shows_as_open(s: &AppointmentSummary) -> bool {
    s.status == "planning"
        || (s.status == "done" && s.appointment.closed_on.is_none() && s.todos_open > 0)
}

/// One appointment of the vehicle with its counts, whatever its status.
///
/// # Errors
/// `NotFound` when it does not exist or belongs to another vehicle.
pub async fn summary(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<AppointmentSummary> {
    require(db, vehicle_id, id).await?;
    summaries(db, &[vehicle_id], Some(id))
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| DomainError::NotFound(format!("Appointment {id} not found")))
}

/// The summary of `appointment` from the statuses of its jobs and its counts
/// of open needs and open to-dos.
pub(crate) fn summarize(
    appointment: appointment::Model,
    job_statuses: &[&str],
    needs_open: usize,
    todos_open: usize,
) -> AppointmentSummary {
    AppointmentSummary {
        status: status(&appointment, job_statuses),
        booked: appointment.planned_date.is_some(),
        jobs: job_statuses.len(),
        needs_open,
        todos_open,
        appointment,
    }
}

// Builds the summaries for every appointment of the vehicles, or for the one
// named by `only`, in list order, with every count loaded in batches.
async fn summaries(
    db: &impl ConnectionTrait,
    vehicle_ids: &[i32],
    only: Option<i32>,
) -> DomainResult<Vec<AppointmentSummary>> {
    let mut query = appointment::Entity::find()
        .filter(appointment::Column::VehicleId.is_in(vehicle_ids.iter().copied()));
    if let Some(id) = only {
        query = query.filter(appointment::Column::Id.eq(id));
    }
    let appointments = query.all(db).await?;
    let ids: Vec<i32> = appointments.iter().map(|a| a.id).collect();
    let jobs = work_item::Entity::find()
        .filter(work_item::Column::AppointmentId.is_in(ids.clone()))
        .all(db)
        .await?;
    let job_ids: Vec<i32> = jobs.iter().map(|j| j.id).collect();
    let needs = job_need::Entity::find()
        .filter(job_need::Column::WorkItemId.is_in(job_ids))
        .all(db)
        .await?;
    let part_ids: Vec<i32> = needs.iter().filter_map(|n| n.part_id).collect();
    let parts: HashMap<i32, part::Model> = part::Entity::find()
        .filter(part::Column::Id.is_in(part_ids))
        .all(db)
        .await?
        .into_iter()
        .map(|p| (p.id, p))
        .collect();
    let todos = todo::Entity::find()
        .filter(todo::Column::AppointmentId.is_in(ids))
        .filter(todo::Column::DoneOn.is_null())
        .all(db)
        .await?;

    let mut job_appointment: HashMap<i32, i32> = HashMap::new();
    let job_status: HashMap<i32, &str> = jobs.iter().map(|j| (j.id, j.status.as_str())).collect();
    let mut statuses: HashMap<i32, Vec<&str>> = HashMap::new();
    for job in &jobs {
        if let Some(appointment_id) = job.appointment_id {
            job_appointment.insert(job.id, appointment_id);
            statuses
                .entry(appointment_id)
                .or_default()
                .push(&job.status);
        }
    }
    let mut needs_open: HashMap<i32, usize> = HashMap::new();
    for need in &needs {
        let current = stage(need, need.part_id.and_then(|id| parts.get(&id)));
        let on_job = job_status.get(&need.work_item_id).copied().unwrap_or("");
        if counts_as_open(on_job, current)
            && let Some(appointment_id) = job_appointment.get(&need.work_item_id)
        {
            *needs_open.entry(*appointment_id).or_default() += 1;
        }
    }
    let mut todos_open: HashMap<i32, usize> = HashMap::new();
    for todo in &todos {
        if let Some(appointment_id) = todo.appointment_id {
            *todos_open.entry(appointment_id).or_default() += 1;
        }
    }

    let mut out: Vec<AppointmentSummary> = appointments
        .into_iter()
        .map(|a| {
            let job_statuses = statuses.get(&a.id).map_or(&[][..], Vec::as_slice);
            let needs = needs_open.get(&a.id).copied().unwrap_or(0);
            let todos = todos_open.get(&a.id).copied().unwrap_or(0);
            summarize(a, job_statuses, needs, todos)
        })
        .collect();
    out.sort_by(|a, b| {
        let key = |s: &AppointmentSummary| {
            (
                s.appointment.planned_date.is_none(),
                s.appointment.planned_date.clone(),
                s.appointment.id,
            )
        };
        key(a).cmp(&key(b))
    });
    Ok(out)
}

/// Closes an appointment by hand (`closed_on`) and takes every job still
/// `planned` off it, in one transaction. Done and dropped jobs stay, and no
/// need is touched.
///
/// # Errors
/// `NotFound` for an appointment that is not the vehicle's; `BadRequest` when
/// it is already cancelled or closed by hand.
pub async fn close<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<ClosedAppointment> {
    let txn = db.begin().await?;
    let closed = put_away(&txn, vehicle_id, id, Ending::Closed).await?;
    txn.commit().await?;
    Ok(closed)
}

/// [`close`], marking the appointment cancelled (`cancelled_on`) instead.
///
/// # Errors
/// As for [`close`].
pub async fn cancel<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<ClosedAppointment> {
    let txn = db.begin().await?;
    let cancelled = put_away(&txn, vehicle_id, id, Ending::Cancelled).await?;
    txn.commit().await?;
    Ok(cancelled)
}

// Closes or cancels on the caller's transaction.
async fn put_away(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
    ending: Ending,
) -> DomainResult<ClosedAppointment> {
    let existing = require(db, vehicle_id, id).await?;
    if let Some(current) = Ending::of(&existing) {
        return Err(DomainError::BadRequest(format!(
            "Appointment {id} is already {}",
            current.label()
        )));
    }
    let today = crate::services::today_label();
    let unassigned_job_ids: Vec<i32> = work_item::Entity::find()
        .filter(work_item::Column::AppointmentId.eq(id))
        .filter(work_item::Column::Status.eq("planned"))
        .order_by_asc(work_item::Column::Id)
        .all(db)
        .await?
        .into_iter()
        .map(|j| j.id)
        .collect();
    if !unassigned_job_ids.is_empty() {
        work_item::Entity::update_many()
            .col_expr(work_item::Column::AppointmentId, Expr::value(None::<i32>))
            .col_expr(
                work_item::Column::UpdatedAt,
                Expr::value(super::now_stamp()),
            )
            .filter(work_item::Column::Id.is_in(unassigned_job_ids.clone()))
            .exec(db)
            .await?;
    }
    let mut active: appointment::ActiveModel = existing.into();
    match ending {
        Ending::Cancelled => active.cancelled_on = Set(Some(today)),
        Ending::Closed => active.closed_on = Set(Some(today)),
    }
    active.updated_at = Set(super::now_stamp());
    let appointment = active.update(db).await?;
    Ok(ClosedAppointment {
        appointment,
        unassigned_job_ids,
    })
}

/// Brings the appointments of jobs that have just returned to `planned` back
/// in line with the rule that a closed or cancelled appointment holds no
/// planned job, on the caller's transaction. A hand-closed appointment opens
/// again, and a job on a cancelled one goes back to the unscheduled pool.
pub(crate) async fn settle_reopened_jobs(
    db: &impl ConnectionTrait,
    job_ids: &[i32],
) -> DomainResult<()> {
    if job_ids.is_empty() {
        return Ok(());
    }
    let appointment_ids: Vec<i32> = work_item::Entity::find()
        .filter(work_item::Column::Id.is_in(job_ids.to_vec()))
        .all(db)
        .await?
        .into_iter()
        .filter_map(|j| j.appointment_id)
        .collect();
    let (cancelled, closed): (Vec<appointment::Model>, Vec<appointment::Model>) =
        appointment::Entity::find()
            .filter(appointment::Column::Id.is_in(appointment_ids))
            .filter(
                Condition::any()
                    .add(appointment::Column::CancelledOn.is_not_null())
                    .add(appointment::Column::ClosedOn.is_not_null()),
            )
            .all(db)
            .await?
            .into_iter()
            .partition(|a| Ending::of(a) == Some(Ending::Cancelled));
    if !closed.is_empty() {
        appointment::Entity::update_many()
            .col_expr(appointment::Column::ClosedOn, Expr::value(None::<String>))
            .col_expr(
                appointment::Column::UpdatedAt,
                Expr::value(super::now_stamp()),
            )
            .filter(appointment::Column::Id.is_in(closed.iter().map(|a| a.id)))
            .exec(db)
            .await?;
    }
    if !cancelled.is_empty() {
        work_item::Entity::update_many()
            .col_expr(work_item::Column::AppointmentId, Expr::value(None::<i32>))
            .col_expr(
                work_item::Column::UpdatedAt,
                Expr::value(super::now_stamp()),
            )
            .filter(work_item::Column::Id.is_in(job_ids.to_vec()))
            .filter(work_item::Column::AppointmentId.is_in(cancelled.iter().map(|a| a.id)))
            .exec(db)
            .await?;
    }
    Ok(())
}

/// Clears both `closed_on` and `cancelled_on`. Jobs that closing took off the
/// appointment stay off it.
///
/// # Errors
/// `NotFound` for an appointment that is not the vehicle's.
pub async fn reopen(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<appointment::Model> {
    let existing = require(db, vehicle_id, id).await?;
    let mut active: appointment::ActiveModel = existing.into();
    active.cancelled_on = Set(None);
    active.closed_on = Set(None);
    active.updated_at = Set(super::now_stamp());
    Ok(active.update(db).await?)
}

/// Deletes an appointment. Its jobs, to-dos and services stay and lose the
/// appointment through the `ON DELETE SET NULL` foreign keys.
///
/// # Errors
/// `NotFound` for an appointment that is not the vehicle's.
pub async fn delete(db: &impl ConnectionTrait, vehicle_id: i32, id: i32) -> DomainResult<()> {
    require(db, vehicle_id, id).await?.delete(db).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::{service_record, todo, work_item},
        error::DomainError,
        inputs::{
            need::NewNeed,
            service_record::NewServiceRecord,
            todo::NewTodo,
            work_item::{NewWorkItem, UpdateWorkItem},
        },
        services::{need, todo as todo_svc, work_item as job},
        test_support::{VehicleFixture, add_job, set_job_status, test_db},
    };

    fn appt(purpose: &str) -> NewAppointment {
        NewAppointment {
            purpose: purpose.into(),
            ..Default::default()
        }
    }

    fn model(cancelled_on: Option<&str>, closed_on: Option<&str>) -> appointment::Model {
        appointment::Model {
            id: 1,
            vehicle_id: 1,
            purpose: "p".into(),
            shop_name: None,
            shop_id: None,
            planned_date: None,
            notes: None,
            cancelled_on: cancelled_on.map(Into::into),
            closed_on: closed_on.map(Into::into),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn status_table() {
        let open = model(None, None);
        assert_eq!(status(&open, &[]), "planning");
        assert_eq!(status(&open, &["done", "dropped"]), "done");
        assert_eq!(status(&open, &["done", "planned"]), "planning");
        assert_eq!(
            status(&model(Some("2026-11-01"), Some("2026-11-02")), &["done"]),
            "cancelled"
        );
        assert_eq!(
            status(&model(None, Some("2026-11-02")), &["planned"]),
            "done"
        );
    }

    #[tokio::test]
    async fn create_rejects_blank_purpose() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let err = create(&db, v, appt("   ")).await.unwrap_err();
        assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "purpose"));
    }

    #[tokio::test]
    async fn create_and_update_round_trip() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let made = create(
            &db,
            v,
            NewAppointment {
                purpose: "Brakes".into(),
                shop_name: Some("Example VW".into()),
                planned_date: Some("2026-12-01".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(made.shop_name.as_deref(), Some("Example VW"));
        let bad = create(
            &db,
            v,
            NewAppointment {
                purpose: "x".into(),
                planned_date: Some("2026-1-1".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(bad, DomainError::Invalid { ref field, .. } if field == "planned_date"));
        let missing_shop = create(
            &db,
            v,
            NewAppointment {
                purpose: "x".into(),
                shop_id: Some(9999),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(missing_shop, DomainError::NotFound(_)));
        let updated = update(
            &db,
            v,
            made.id,
            UpdateAppointment {
                purpose: Some("Brakes and rotors".into()),
                planned_date: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.purpose, "Brakes and rotors");
        assert_eq!(updated.planned_date, None);
        assert_eq!(updated.shop_name.as_deref(), Some("Example VW"));
        let blank = update(
            &db,
            v,
            made.id,
            UpdateAppointment {
                purpose: Some(" ".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(blank, DomainError::Invalid { .. }));
    }

    #[tokio::test]
    async fn assigning_a_job_from_another_vehicle_reads_as_missing() {
        let db = test_db().await;
        let v1 = VehicleFixture::new().insert_id(&db).await;
        let v2 = VehicleFixture::new().name("Other").insert_id(&db).await;
        let theirs = create(&db, v2, appt("Theirs")).await.unwrap();
        let nonexistent = job::create(
            &db,
            v1,
            NewWorkItem {
                title: "x".into(),
                appointment_id: Some(99_999),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        let wrong = job::create(
            &db,
            v1,
            NewWorkItem {
                title: "x".into(),
                appointment_id: Some(theirs.id),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(wrong, DomainError::NotFound(_)));
        assert_eq!(
            wrong.to_string().replace(&theirs.id.to_string(), "N"),
            nonexistent.to_string().replace("99999", "N")
        );
        let mine = add_job(&db, v1, "Brake pads", None).await;
        let err = job::update(
            &db,
            v1,
            mine,
            UpdateWorkItem {
                appointment_id: Some(Some(theirs.id)),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)));
    }

    #[tokio::test]
    async fn assigning_to_a_cancelled_appointment_is_refused() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = create(&db, v, appt("Brakes")).await.unwrap();
        cancel(&db, v, a.id).await.unwrap();
        let err = job::create(
            &db,
            v,
            NewWorkItem {
                title: "x".into(),
                appointment_id: Some(a.id),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)));
        assert_eq!(
            err.to_string(),
            format!("Appointment {} is cancelled; reopen it first", a.id)
        );
        let b = create(&db, v, appt("Other")).await.unwrap();
        close(&db, v, b.id).await.unwrap();
        let mine = add_job(&db, v, "Brake pads", None).await;
        let err = job::update(
            &db,
            v,
            mine,
            UpdateWorkItem {
                appointment_id: Some(Some(b.id)),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)));
        assert_eq!(
            err.to_string(),
            format!("Appointment {} is closed; reopen it first", b.id)
        );
    }

    #[tokio::test]
    async fn update_with_status_edits_and_closes_in_one_write() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = create(&db, v, appt("Brakes")).await.unwrap();
        let planned = add_job(&db, v, "Brake pads", Some(a.id)).await;

        let revised = update_with_status(
            &db,
            v,
            a.id,
            UpdateAppointment {
                purpose: Some("Brakes and plugs".into()),
                ..Default::default()
            },
            Some(StatusChange::Cancel),
        )
        .await
        .unwrap();
        assert_eq!(revised.appointment.appointment.purpose, "Brakes and plugs");
        assert_eq!(revised.appointment.status, "cancelled");
        assert_eq!(revised.unassigned_job_ids, vec![planned]);

        let reopened = update_with_status(
            &db,
            v,
            a.id,
            UpdateAppointment::default(),
            Some(StatusChange::Reopen),
        )
        .await
        .unwrap();
        assert_eq!(reopened.appointment.status, "planning");
        assert!(reopened.unassigned_job_ids.is_empty());

        let closed = update_with_status(
            &db,
            v,
            a.id,
            UpdateAppointment::default(),
            Some(StatusChange::Close),
        )
        .await
        .unwrap();
        assert_eq!(closed.appointment.status, "done");
        assert!(closed.appointment.appointment.closed_on.is_some());

        // No status change edits only.
        let edited = update_with_status(
            &db,
            v,
            a.id,
            UpdateAppointment {
                notes: Some(Some("bring pads".into())),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            edited.appointment.appointment.notes.as_deref(),
            Some("bring pads")
        );
    }

    #[tokio::test]
    async fn a_refused_status_change_leaves_the_edits_unapplied() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = create(&db, v, appt("Brakes")).await.unwrap();
        close(&db, v, a.id).await.unwrap();

        let err = update_with_status(
            &db,
            v,
            a.id,
            UpdateAppointment {
                purpose: Some("Renamed".into()),
                ..Default::default()
            },
            Some(StatusChange::Cancel),
        )
        .await
        .unwrap_err();

        assert!(matches!(err, DomainError::BadRequest(_)));
        assert_eq!(require(&db, v, a.id).await.unwrap().purpose, "Brakes");
    }

    #[tokio::test]
    async fn update_with_status_reads_a_wrong_vehicle_appointment_as_missing() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let other = VehicleFixture::new().name("Other").insert_id(&db).await;
        let a = create(&db, v, appt("Brakes")).await.unwrap();
        let missing = update_with_status(&db, other, 99_999, UpdateAppointment::default(), None)
            .await
            .unwrap_err();
        let wrong = update_with_status(&db, other, a.id, UpdateAppointment::default(), None)
            .await
            .unwrap_err();
        assert!(matches!(wrong, DomainError::NotFound(_)));
        assert_eq!(
            wrong.to_string().replace(&a.id.to_string(), "N"),
            missing.to_string().replace("99999", "N")
        );
    }

    #[tokio::test]
    async fn close_unassigns_planned_jobs_and_keeps_their_needs() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = create(&db, v, appt("Brakes")).await.unwrap();
        let planned = add_job(&db, v, "Brake pads", Some(a.id)).await;
        let finished = add_job(&db, v, "Brake pads", Some(a.id)).await;
        set_job_status(&db, v, finished, "done").await;
        need::add(
            &db,
            v,
            planned,
            NewNeed {
                name: "Pads".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let closed = close(&db, v, a.id).await.unwrap();
        assert_eq!(closed.unassigned_job_ids, vec![planned]);
        assert!(closed.appointment.closed_on.is_some());
        assert!(closed.appointment.cancelled_on.is_none());
        assert_eq!(
            job::get(&db, v, planned).await.unwrap().appointment_id,
            None
        );
        assert_eq!(
            job::get(&db, v, finished).await.unwrap().appointment_id,
            Some(a.id)
        );
        let needs = crate::entities::job_need::Entity::find()
            .filter(crate::entities::job_need::Column::WorkItemId.eq(planned))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(needs.len(), 1);

        let again = cancel(&db, v, a.id).await.unwrap_err();
        assert!(matches!(again, DomainError::BadRequest(_)));
        let reopened = reopen(&db, v, a.id).await.unwrap();
        assert!(reopened.closed_on.is_none() && reopened.cancelled_on.is_none());
        let cancelled = cancel(&db, v, a.id).await.unwrap();
        assert!(cancelled.appointment.cancelled_on.is_some());
        assert!(cancelled.appointment.closed_on.is_none());
        let again = close(&db, v, a.id).await.unwrap_err();
        assert!(matches!(again, DomainError::BadRequest(_)));
        let reopened = reopen(&db, v, a.id).await.unwrap();
        assert!(reopened.cancelled_on.is_none());
    }

    #[tokio::test]
    async fn delete_keeps_jobs() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = create(&db, v, appt("Brakes")).await.unwrap();
        let j = add_job(&db, v, "Brake pads", Some(a.id)).await;
        let t = todo_svc::add(
            &db,
            v,
            NewTodo {
                title: "Bring wheel locks".into(),
                appointment_id: Some(a.id),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let s = crate::services::service_record::create(
            &db,
            v,
            NewServiceRecord {
                service_date: "2026-11-01".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        service_record::Entity::update_many()
            .col_expr(
                service_record::Column::AppointmentId,
                sea_orm::sea_query::Expr::value(a.id),
            )
            .filter(service_record::Column::Id.eq(s.record.id))
            .exec(&db)
            .await
            .unwrap();

        delete(&db, v, a.id).await.unwrap();
        assert!(matches!(
            require(&db, v, a.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        let j = work_item::Entity::find_by_id(j)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(j.appointment_id, None);
        let t = todo::Entity::find_by_id(t.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t.appointment_id, None);
        let s = service_record::Entity::find_by_id(s.record.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(s.appointment_id, None);
    }

    #[tokio::test]
    async fn summary_counts_open_needs_and_todos() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = create(
            &db,
            v,
            NewAppointment {
                purpose: "Brakes".into(),
                planned_date: Some("2026-12-01".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let unbooked = create(&db, v, appt("Someday")).await.unwrap();
        let j1 = add_job(&db, v, "Brake pads", Some(a.id)).await;
        let j2 = add_job(&db, v, "Brake pads", Some(a.id)).await;
        for (job_id, name, supplied_by) in [
            (j1, "Pads", None),
            (j1, "Rotors", Some("shop")),
            (j2, "Fluid", None),
        ] {
            need::add(
                &db,
                v,
                job_id,
                NewNeed {
                    name: name.into(),
                    supplied_by: supplied_by.map(Into::into),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        let open_todo = todo_svc::add(
            &db,
            v,
            NewTodo {
                title: "Bring wheel locks".into(),
                appointment_id: Some(a.id),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let done_todo = todo_svc::add(
            &db,
            v,
            NewTodo {
                title: "Book loaner".into(),
                appointment_id: Some(a.id),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        todo_svc::complete(&db, v, done_todo.id, None)
            .await
            .unwrap();
        let _ = open_todo;

        let all = list(&db, v, true).await.unwrap();
        let s = all.iter().find(|s| s.appointment.id == a.id).unwrap();
        assert_eq!(
            (s.status, s.booked, s.jobs, s.needs_open, s.todos_open),
            ("planning", true, 2, 2, 1)
        );
        let u = all
            .iter()
            .find(|s| s.appointment.id == unbooked.id)
            .unwrap();
        assert_eq!(
            (u.booked, u.jobs, u.needs_open, u.todos_open),
            (false, 0, 0, 0)
        );

        cancel(&db, v, unbooked.id).await.unwrap();
        let open_only = list(&db, v, false).await.unwrap();
        assert_eq!(open_only.len(), 1);
        assert_eq!(open_only[0].appointment.id, a.id);
    }

    #[tokio::test]
    async fn wrong_vehicle_appointment_ids_read_as_missing() {
        let db = test_db().await;
        let v1 = VehicleFixture::new().insert_id(&db).await;
        let v2 = VehicleFixture::new().name("Other").insert_id(&db).await;
        let a = create(&db, v1, appt("Brakes")).await.unwrap();
        let missing = require(&db, v1, 99_999).await.unwrap_err().to_string();
        let wrong = require(&db, v2, a.id).await.unwrap_err();
        assert!(matches!(wrong, DomainError::NotFound(_)));
        assert_eq!(
            wrong.to_string().replace(&a.id.to_string(), "N"),
            missing.replace("99999", "N")
        );
        let same_as_missing = |err: DomainError| {
            assert!(matches!(err, DomainError::NotFound(_)));
            assert_eq!(
                err.to_string().replace(&a.id.to_string(), "N"),
                missing.replace("99999", "N")
            );
        };
        same_as_missing(
            update(&db, v2, a.id, UpdateAppointment::default())
                .await
                .unwrap_err(),
        );
        same_as_missing(close(&db, v2, a.id).await.unwrap_err());
        same_as_missing(reopen(&db, v2, a.id).await.unwrap_err());
        same_as_missing(delete(&db, v2, a.id).await.unwrap_err());
        assert!(require(&db, v1, a.id).await.is_ok());
    }

    #[tokio::test]
    async fn open_list_keeps_a_done_appointment_that_still_has_an_open_todo() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = create(&db, v, appt("Brakes")).await.unwrap();
        let job_id = add_job(&db, v, "Brake pads", Some(a.id)).await;
        set_job_status(&db, v, job_id, "done").await;
        let t = todo_svc::add(
            &db,
            v,
            NewTodo {
                title: "Return the old pads".into(),
                appointment_id: Some(a.id),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let open = list(&db, v, false).await.unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!((open[0].status, open[0].todos_open), ("done", 1));

        todo_svc::complete(&db, v, t.id, None).await.unwrap();
        assert!(list(&db, v, false).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_job_set_back_to_planned_reopens_a_hand_closed_appointment() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let closed = create(&db, v, appt("Brakes")).await.unwrap();
        let cancelled = create(&db, v, appt("Wipers")).await.unwrap();
        let on_closed = add_job(&db, v, "Brake pads", Some(closed.id)).await;
        let on_cancelled = add_job(&db, v, "Brake pads", Some(cancelled.id)).await;
        let set_status = |job_id, status: &str| {
            job::update(
                &db,
                v,
                job_id,
                UpdateWorkItem {
                    status: Some(status.into()),
                    ..Default::default()
                },
            )
        };
        set_status(on_closed, "done").await.unwrap();
        set_status(on_cancelled, "dropped").await.unwrap();
        close(&db, v, closed.id).await.unwrap();
        cancel(&db, v, cancelled.id).await.unwrap();

        set_status(on_closed, "planned").await.unwrap();
        // The returned job says where it ended up, not where it was.
        let returned = set_status(on_cancelled, "planned").await.unwrap();
        assert_eq!(returned.appointment_id, None);

        let reopened = summary(&db, v, closed.id).await.unwrap();
        assert_eq!(reopened.status, "planning");
        assert_eq!(reopened.appointment.closed_on, None);
        assert_eq!(
            summary(&db, v, cancelled.id).await.unwrap().status,
            "cancelled"
        );
        let unscheduled = job::get(&db, v, on_cancelled).await.unwrap();
        assert_eq!(unscheduled.appointment_id, None);
    }

    #[tokio::test]
    async fn a_job_update_may_name_the_closed_appointment_it_is_already_on() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let closed = create(&db, v, appt("Brakes")).await.unwrap();
        let other = create(&db, v, appt("Wipers")).await.unwrap();
        let on_closed = add_job(&db, v, "Brake pads", Some(closed.id)).await;
        let elsewhere = add_job(&db, v, "Wiper blades", Some(other.id)).await;
        set_job_status(&db, v, on_closed, "done").await;
        set_job_status(&db, v, elsewhere, "done").await;
        close(&db, v, closed.id).await.unwrap();

        let kept = job::update(
            &db,
            v,
            on_closed,
            UpdateWorkItem {
                notes: Some(Some("Pads only".into())),
                appointment_id: Some(Some(closed.id)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(kept.appointment_id, Some(closed.id));
        assert_eq!(kept.notes.as_deref(), Some("Pads only"));

        // Moving another job onto the closed appointment is a new assignment.
        let err = job::update(
            &db,
            v,
            elsewhere,
            UpdateWorkItem {
                appointment_id: Some(Some(closed.id)),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
    }

    #[tokio::test]
    async fn open_list_drops_a_hand_closed_appointment_even_with_an_open_todo() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = create(&db, v, appt("Brakes")).await.unwrap();
        todo_svc::add(
            &db,
            v,
            NewTodo {
                title: "Bring wheel locks".into(),
                appointment_id: Some(a.id),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        close(&db, v, a.id).await.unwrap();
        assert!(list(&db, v, false).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn needs_on_a_dropped_job_are_not_open() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = create(&db, v, appt("Brakes")).await.unwrap();
        let kept = add_job(&db, v, "Brake pads", Some(a.id)).await;
        let dropped = add_job(&db, v, "Brake pads", Some(a.id)).await;
        for job_id in [kept, dropped] {
            need::add(
                &db,
                v,
                job_id,
                NewNeed {
                    name: "Pads".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        set_job_status(&db, v, dropped, "dropped").await;
        let all = list(&db, v, true).await.unwrap();
        assert_eq!(all[0].needs_open, 1);
    }
}
