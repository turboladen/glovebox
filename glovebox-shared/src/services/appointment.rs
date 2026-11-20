//! Appointments: a visit that groups jobs and to-dos. Every function scopes
//! its ids to a vehicle, so another vehicle's id reads as a missing one.

use std::collections::HashMap;

use sea_orm::{sea_query::Expr, *};
use serde::Serialize;

use crate::{
    entities::{
        appointment, job_need, part, todo,
        work_item::{self, JobStatus},
    },
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
    /// The name of the shop `shop_id` names, read from the shop directory.
    pub shop_name: Option<String>,
    pub status: AppointmentStatus,
    /// Where the appointment stands, as [`phase`] reads it from the status,
    /// the planned date and today's date.
    pub phase: AppointmentPhase,
    pub jobs: usize,
    pub needs_open: usize,
    pub todos_open: usize,
    #[serde(skip)]
    ending: Ending,
}

impl AppointmentSummary {
    /// True while the appointment takes new jobs and to-dos: it is neither
    /// closed by hand nor cancelled. One whose jobs are all done still does.
    #[must_use]
    pub fn accepts_work(&self) -> bool {
        self.ending == Ending::Open
    }
}

/// What closing an appointment did.
#[derive(Debug, Serialize)]
pub struct ClosedAppointment {
    pub appointment: appointment::Model,
    /// The jobs that were still planned and are now off the appointment.
    /// Only cancelling takes any off; closing refuses while one is planned.
    pub unassigned_job_ids: Vec<i32>,
    /// The open to-dos that cancelling took off the appointment.
    pub unassigned_todo_ids: Vec<i32>,
}

/// An appointment after edits and a status change, with the jobs and to-dos
/// that cancelling took off it.
#[derive(Debug, Serialize)]
pub struct RevisedAppointment {
    pub appointment: AppointmentSummary,
    pub unassigned_job_ids: Vec<i32>,
    pub unassigned_todo_ids: Vec<i32>,
}

/// Whether an appointment was put away by hand, and when. It is read from
/// and written to `closed_on` and `cancelled_on`, which are never both set.
// `reopen_closed_among` clears `closed_on` in bulk, which leaves each row it
// touches open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ending {
    Open,
    Closed(chrono::NaiveDate),
    Cancelled(chrono::NaiveDate),
}

impl Ending {
    /// Reads a stored appointment.
    ///
    /// # Errors
    /// `Internal` for one that holds both dates.
    pub(crate) fn of(a: &appointment::Model) -> DomainResult<Self> {
        match (a.closed_on, a.cancelled_on) {
            (None, None) => Ok(Self::Open),
            (Some(on), None) => Ok(Self::Closed(on)),
            (None, Some(on)) => Ok(Self::Cancelled(on)),
            (Some(_), Some(_)) => Err(DomainError::Internal(format!(
                "appointment {} is both closed and cancelled",
                a.id
            ))),
        }
    }

    /// Sets `closed_on` and `cancelled_on`.
    pub(crate) fn write(self, active: &mut appointment::ActiveModel) {
        let (closed, cancelled) = match self {
            Self::Open => (None, None),
            Self::Closed(on) => (Some(on), None),
            Self::Cancelled(on) => (None, Some(on)),
        };
        active.closed_on = Set(closed);
        active.cancelled_on = Set(cancelled);
    }

    // How the appointment was put away, or `None` while it is open.
    fn put_away_as(self) -> Option<&'static str> {
        match self {
            Self::Open => None,
            Self::Closed(_) => Some("closed"),
            Self::Cancelled(_) => Some("cancelled"),
        }
    }
}

crate::vocab::vocabulary! {
    /// An appointment's status: `cancelled` when cancelled, `done` when closed by hand or when
    /// it has jobs and every one is done or dropped, otherwise `planning`.
    // No column stores it: the closed and cancelled dates and the jobs'
    // statuses decide it, and a `StatusChange` moves an appointment to one.
    pub enum AppointmentStatus {
        Planning => "planning",
        Done => "done",
        Cancelled => "cancelled",
    }
}

// Derives the status `AppointmentStatus` describes from the appointment's
// ending and its jobs' statuses.
fn status(ending: Ending, job_statuses: &[JobStatus]) -> AppointmentStatus {
    match ending {
        Ending::Cancelled(_) => AppointmentStatus::Cancelled,
        Ending::Closed(_) => AppointmentStatus::Done,
        Ending::Open
            if !job_statuses.is_empty() && job_statuses.iter().all(|s| s.is_finished()) =>
        {
            AppointmentStatus::Done
        }
        Ending::Open => AppointmentStatus::Planning,
    }
}

crate::vocab::vocabulary! {
    /// Where an appointment stands, as [`phase`] derives it.
    // No column stores it. `planning`, `booked` and `in_shop` split an open
    // appointment by its planned date: none, after today, or today or
    // earlier. No rule keys on it, and `AppointmentStatus` stays the value a
    // caller sets, so a phase is never a status change.
    pub enum AppointmentPhase {
        Planning => "planning",
        Booked => "booked",
        InShop => "in_shop",
        Done => "done",
        Cancelled => "cancelled",
    }
}

/// The phase of an appointment with `status` and `planned_date` on `today`.
/// An open appointment is `planning` without a date, `booked` before its
/// date and `in_shop` from its date on. `done` and `cancelled` carry over.
#[must_use]
pub fn phase(
    status: AppointmentStatus,
    planned_date: Option<chrono::NaiveDate>,
    today: chrono::NaiveDate,
) -> AppointmentPhase {
    match (status, planned_date) {
        (AppointmentStatus::Done, _) => AppointmentPhase::Done,
        (AppointmentStatus::Cancelled, _) => AppointmentPhase::Cancelled,
        (AppointmentStatus::Planning, None) => AppointmentPhase::Planning,
        (AppointmentStatus::Planning, Some(date)) if date > today => AppointmentPhase::Booked,
        (AppointmentStatus::Planning, Some(_)) => AppointmentPhase::InShop,
    }
}

/// Creates an appointment for a vehicle.
///
/// # Errors
/// `NotFound` for a vehicle or shop id that does not exist; `BadRequest` for
/// a shop name that matches no shop or several; `Invalid` for a blank
/// purpose.
pub async fn create<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    input: NewAppointment,
) -> DomainResult<appointment::Model> {
    super::validate_not_blank("purpose", &input.purpose)?;
    let txn = super::begin_write(db).await?;
    let db = &txn;
    super::vehicle::require_in_garage(db, vehicle_id).await?;
    let shop_id = super::shop::resolve(db, input.shop.as_ref()).await?;
    let model = appointment::ActiveModel {
        vehicle_id: Set(vehicle_id),
        purpose: Set(input.purpose),
        shop_id: Set(shop_id),
        planned_date: Set(input.planned_date),
        notes: Set(input.notes),
        ..Default::default()
    };
    let out = model.insert(db).await?;
    txn.commit().await?;
    Ok(out)
}

/// Creates an appointment as [`create`] does and reads it back as every
/// appointment read summarizes it on `today`.
///
/// # Errors
/// As for [`create`].
pub async fn plan<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    input: NewAppointment,
    today: chrono::NaiveDate,
) -> DomainResult<AppointmentSummary> {
    let created = create(db, vehicle_id, input).await?;
    summary(db, vehicle_id, created.id, today).await
}

/// Changes an appointment's purpose, shop, date or notes.
///
/// # Errors
/// `NotFound` for an appointment that is not the vehicle's or a missing shop;
/// `BadRequest` and `Invalid` as for [`create`].
pub(crate) async fn update(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
    input: UpdateAppointment,
) -> DomainResult<appointment::Model> {
    let existing = require(db, vehicle_id, id).await?;
    if let Some(purpose) = &input.purpose {
        super::validate_not_blank("purpose", purpose)?;
    }
    let shop_id = super::shop::resolve_change(db, input.shop.as_ref()).await?;
    let mut active: appointment::ActiveModel = existing.into();
    if let Some(v) = input.purpose {
        active.purpose = Set(v);
    }
    if let Some(v) = shop_id {
        active.shop_id = Set(v);
    }
    if let Some(v) = input.planned_date {
        active.planned_date = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }
    active.updated_at = Set(super::now());
    Ok(active.update(db).await?)
}

/// Edits an appointment and, when `change` is present, closes, cancels or
/// reopens it, in one transaction, and returns its summary on `today`. A
/// refused status change leaves the edits unapplied.
///
/// # Errors
/// `NotFound` for an appointment that is not the vehicle's or a missing shop;
/// `Invalid` as for [`create`]; and as for [`close`] or [`reopen`] for the
/// status change.
pub async fn update_with_status<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    input: UpdateAppointment,
    change: Option<StatusChange>,
    today: chrono::NaiveDate,
) -> DomainResult<RevisedAppointment> {
    let txn = super::begin_write(db).await?;
    update(&txn, vehicle_id, id, input).await?;
    let (unassigned_job_ids, unassigned_todo_ids) = match change {
        Some(StatusChange::Close) => {
            put_away(&txn, vehicle_id, id, Ending::Closed, today).await?;
            (Vec::new(), Vec::new())
        }
        Some(StatusChange::Cancel) => {
            let cancelled = put_away(&txn, vehicle_id, id, Ending::Cancelled, today).await?;
            (cancelled.unassigned_job_ids, cancelled.unassigned_todo_ids)
        }
        Some(StatusChange::Reopen) => {
            reopen(&txn, vehicle_id, id).await?;
            (Vec::new(), Vec::new())
        }
        None => (Vec::new(), Vec::new()),
    };
    let appointment = summary(&txn, vehicle_id, id, today).await?;
    txn.commit().await?;
    Ok(RevisedAppointment {
        appointment,
        unassigned_job_ids,
        unassigned_todo_ids,
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

/// [`require`] for an appointment that is about to receive a job, a to-do or
/// an invoice: a cancelled or hand-closed appointment is refused.
pub(crate) async fn require_assignable(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<appointment::Model> {
    let found = require(db, vehicle_id, id).await?;
    if let Some(how) = Ending::of(&found)?.put_away_as() {
        return Err(DomainError::BadRequest(format!(
            "Appointment {id} is {how}, so it takes no new work"
        )));
    }
    Ok(found)
}

/// Returns `id` when it names an appointment of `vehicle_id` that is neither
/// closed by hand nor cancelled, and `None` otherwise. A record filed for the
/// owner to act on goes on an appointment only while a list still shows that
/// appointment.
pub(crate) async fn open_id(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: Option<i32>,
) -> DomainResult<Option<i32>> {
    let Some(id) = id else {
        return Ok(None);
    };
    let found = appointment::Entity::find_by_id(id)
        .filter(appointment::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?;
    match found {
        Some(a) if Ending::of(&a)? == Ending::Open => Ok(Some(a.id)),
        _ => Ok(None),
    }
}

/// Lists a vehicle's appointments with their counts, soonest planned date
/// first and unbooked ones last. Without `include_closed`, appointments whose
/// status is `done` or `cancelled` are left out, except a `done` one that
/// still has an open to-do and was not closed by hand.
pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    include_closed: bool,
    today: chrono::NaiveDate,
) -> DomainResult<Vec<AppointmentSummary>> {
    super::vehicle::require(db, vehicle_id).await?;
    let mut out = summaries(db, appointment::Column::VehicleId.eq(vehicle_id), today).await?;
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
    today: chrono::NaiveDate,
) -> DomainResult<HashMap<i32, Vec<AppointmentSummary>>> {
    let mut grouped: HashMap<i32, Vec<AppointmentSummary>> = HashMap::new();
    for summary in summaries(
        db,
        appointment::Column::VehicleId.is_in(vehicle_ids.iter().copied()),
        today,
    )
    .await?
    {
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
    s.status == AppointmentStatus::Planning
        || (s.status == AppointmentStatus::Done && s.accepts_work() && s.todos_open > 0)
}

/// Every appointment that names `shop_id`, across vehicles and whatever its
/// status, in [`list`] order.
pub(crate) async fn at_shop(
    db: &impl ConnectionTrait,
    shop_id: i32,
    today: chrono::NaiveDate,
) -> DomainResult<Vec<AppointmentSummary>> {
    summaries(db, appointment::Column::ShopId.eq(shop_id), today).await
}

/// One appointment of the vehicle with its counts, whatever its status.
///
/// # Errors
/// `NotFound` when it does not exist or belongs to another vehicle.
pub async fn summary(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
    today: chrono::NaiveDate,
) -> DomainResult<AppointmentSummary> {
    require(db, vehicle_id, id).await?;
    summaries(db, appointment::Column::Id.eq(id), today)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| DomainError::NotFound(format!("Appointment {id} not found")))
}

/// The summary of `appointment` on `today` from the statuses of its jobs, its
/// counts of open needs and open to-dos, and the name of its shop.
///
/// # Errors
/// `Internal` for an appointment that holds both a closed and a cancelled
/// date.
pub(crate) fn summarize(
    appointment: appointment::Model,
    job_statuses: &[JobStatus],
    needs_open: usize,
    todos_open: usize,
    shop_name: Option<String>,
    today: chrono::NaiveDate,
) -> DomainResult<AppointmentSummary> {
    let ending = Ending::of(&appointment)?;
    let status = status(ending, job_statuses);
    Ok(AppointmentSummary {
        shop_name,
        status,
        ending,
        phase: phase(status, appointment.planned_date, today),
        jobs: job_statuses.len(),
        needs_open,
        todos_open,
        appointment,
    })
}

// Builds the summaries for every appointment `filter` matches, in list order,
// with every count loaded in batches.
async fn summaries(
    db: &impl ConnectionTrait,
    filter: impl sea_orm::sea_query::IntoCondition,
    today: chrono::NaiveDate,
) -> DomainResult<Vec<AppointmentSummary>> {
    let appointments = appointment::Entity::find().filter(filter).all(db).await?;
    let shop_names = super::shop::names(db, appointments.iter().filter_map(|a| a.shop_id)).await?;
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
    let job_status: HashMap<i32, JobStatus> = jobs.iter().map(|j| (j.id, j.status)).collect();
    let mut statuses: HashMap<i32, Vec<JobStatus>> = HashMap::new();
    for job in &jobs {
        if let Some(appointment_id) = job.appointment_id {
            job_appointment.insert(job.id, appointment_id);
            statuses.entry(appointment_id).or_default().push(job.status);
        }
    }
    let mut needs_open: HashMap<i32, usize> = HashMap::new();
    for need in &needs {
        let current = stage(need, need.part_id.and_then(|id| parts.get(&id)));
        let on_job = job_status.get(&need.work_item_id);
        if on_job.is_some_and(|status| counts_as_open(*status, current))
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
            let shop_name = a.shop_id.and_then(|id| shop_names.get(&id).cloned());
            summarize(a, job_statuses, needs, todos, shop_name, today)
        })
        .collect::<DomainResult<_>>()?;
    out.sort_by(|a, b| {
        let key = |s: &AppointmentSummary| {
            (
                s.appointment.planned_date.is_none(),
                s.appointment.planned_date,
                s.appointment.id,
            )
        };
        key(a).cmp(&key(b))
    });
    Ok(out)
}

/// Closes an appointment by hand (`closed_on`). The appointment keeps its
/// jobs and to-dos.
///
/// # Errors
/// `NotFound` for an appointment that is not the vehicle's; `BadRequest` when
/// it is already cancelled or closed by hand, or while a job on it is still
/// planned or a to-do on it is open, naming each one.
pub async fn close<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<ClosedAppointment> {
    let txn = super::begin_write(db).await?;
    let closed = put_away(&txn, vehicle_id, id, Ending::Closed, super::today()).await?;
    txn.commit().await?;
    Ok(closed)
}

/// Cancels an appointment (`cancelled_on`) and takes every job still
/// `planned` and every open to-do off it, in one transaction. Done and
/// dropped jobs and done to-dos stay, and no need is touched.
///
/// # Errors
/// `NotFound` for an appointment that is not the vehicle's; `BadRequest` when
/// it is already cancelled or closed by hand.
pub async fn cancel<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<ClosedAppointment> {
    let txn = super::begin_write(db).await?;
    let cancelled = put_away(&txn, vehicle_id, id, Ending::Cancelled, super::today()).await?;
    txn.commit().await?;
    Ok(cancelled)
}

// Closes or cancels on the caller's transaction, with `ending` (one of the
// two variants) dated `today`.
async fn put_away(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
    ending: fn(chrono::NaiveDate) -> Ending,
    today: chrono::NaiveDate,
) -> DomainResult<ClosedAppointment> {
    let ending = ending(today);
    let existing = require(db, vehicle_id, id).await?;
    if let Some(how) = Ending::of(&existing)?.put_away_as() {
        return Err(DomainError::BadRequest(format!(
            "Appointment {id} is already {how}"
        )));
    }
    let planned_jobs = work_item::Entity::find()
        .filter(work_item::Column::AppointmentId.eq(id))
        .filter(work_item::Column::Status.eq(JobStatus::Planned))
        .order_by_asc(work_item::Column::Id)
        .all(db)
        .await?;
    let open_todos = todo::Entity::find()
        .filter(todo::Column::AppointmentId.eq(id))
        .filter(todo::Column::DoneOn.is_null())
        .order_by_asc(todo::Column::Id)
        .all(db)
        .await?;
    // A closed appointment records a trip that happened, so what is still open
    // on it needs the owner's word first. A cancelled trip did not happen, so
    // its open work goes back to the pool.
    if matches!(ending, Ending::Closed(_)) && !(planned_jobs.is_empty() && open_todos.is_empty()) {
        return Err(still_open(id, &planned_jobs, &open_todos));
    }
    let unassigned_job_ids: Vec<i32> = planned_jobs.iter().map(|j| j.id).collect();
    if !unassigned_job_ids.is_empty() {
        work_item::Entity::update_many()
            .col_expr(work_item::Column::AppointmentId, Expr::value(None::<i32>))
            .col_expr(work_item::Column::UpdatedAt, Expr::value(super::now()))
            .filter(work_item::Column::Id.is_in(unassigned_job_ids.clone()))
            .exec(db)
            .await?;
    }
    let unassigned_todo_ids: Vec<i32> = open_todos.iter().map(|t| t.id).collect();
    if !unassigned_todo_ids.is_empty() {
        todo::Entity::update_many()
            .col_expr(todo::Column::AppointmentId, Expr::value(None::<i32>))
            .col_expr(todo::Column::UpdatedAt, Expr::value(super::now()))
            .filter(todo::Column::Id.is_in(unassigned_todo_ids.clone()))
            .exec(db)
            .await?;
    }
    let mut active: appointment::ActiveModel = existing.into();
    ending.write(&mut active);
    active.updated_at = Set(super::now());
    let appointment = active.update(db).await?;
    Ok(ClosedAppointment {
        appointment,
        unassigned_job_ids,
        unassigned_todo_ids,
    })
}

// The refusal to close an appointment that still holds a planned job or an
// open to-do, naming each one.
fn still_open(id: i32, jobs: &[work_item::Model], todos: &[todo::Model]) -> DomainError {
    let items: Vec<String> = jobs
        .iter()
        .map(|j| format!("job {} \"{}\"", j.id, j.title))
        .chain(
            todos
                .iter()
                .map(|t| format!("to-do {} \"{}\"", t.id, t.title)),
        )
        .collect();
    DomainError::BadRequest(format!(
        "Appointment {id} still has open work: {}. Mark each one done or move it off the \
         appointment before closing it",
        items.join(", ")
    ))
}

/// What marking a to-do not done does to its appointment, on the caller's
/// transaction: a hand-closed appointment opens again, and a cancelled one
/// gives the to-do up, so a closed or cancelled appointment never holds an
/// open to-do. Returns the appointment the to-do keeps and the one that
/// reopened.
pub(crate) async fn settle_reopened_todo(
    db: &impl ConnectionTrait,
    appointment_id: Option<i32>,
) -> DomainResult<(Option<i32>, Option<i32>)> {
    let Some(id) = appointment_id else {
        return Ok((None, None));
    };
    let (reopened, cancelled) = reopen_closed_among(db, vec![id]).await?;
    if cancelled.contains(&id) {
        Ok((None, None))
    } else {
        Ok((Some(id), reopened.first().copied()))
    }
}

/// Lists what changed when jobs returned to `planned`: the hand-closed
/// appointments that opened again and the jobs that came off a cancelled one,
/// each list sorted by id.
#[derive(Debug, Default, Serialize)]
pub struct SettledJobs {
    /// The hand-closed appointments that are open again.
    pub reopened_appointment_ids: Vec<i32>,
    /// The jobs taken off a cancelled appointment.
    pub unassigned_job_ids: Vec<i32>,
}

/// Brings the appointments of jobs that have just returned to `planned` back
/// in line with the rule that a closed or cancelled appointment holds no
/// planned job, on the caller's transaction. A hand-closed appointment opens
/// again, and a job on a cancelled one goes back to the unscheduled pool.
pub(crate) async fn settle_reopened_jobs(
    db: &impl ConnectionTrait,
    job_ids: &[i32],
) -> DomainResult<SettledJobs> {
    if job_ids.is_empty() {
        return Ok(SettledJobs::default());
    }
    let jobs = work_item::Entity::find()
        .filter(work_item::Column::Id.is_in(job_ids.to_vec()))
        .all(db)
        .await?;
    let appointment_ids: Vec<i32> = jobs.iter().filter_map(|j| j.appointment_id).collect();
    let (reopened_appointment_ids, cancelled) = reopen_closed_among(db, appointment_ids).await?;
    let mut unassigned_job_ids: Vec<i32> = jobs
        .iter()
        .filter(|j| j.appointment_id.is_some_and(|id| cancelled.contains(&id)))
        .map(|j| j.id)
        .collect();
    unassigned_job_ids.sort_unstable();
    let settled = SettledJobs {
        reopened_appointment_ids,
        unassigned_job_ids,
    };
    if !settled.unassigned_job_ids.is_empty() {
        work_item::Entity::update_many()
            .col_expr(work_item::Column::AppointmentId, Expr::value(None::<i32>))
            .col_expr(work_item::Column::UpdatedAt, Expr::value(super::now()))
            .filter(work_item::Column::Id.is_in(settled.unassigned_job_ids.clone()))
            .exec(db)
            .await?;
    }
    Ok(settled)
}

// Opens again every hand-closed appointment among `ids` and returns their ids
// sorted, with the ids of the cancelled ones, which the caller takes its work
// off. One rule serves jobs and to-dos that have just become open again.
async fn reopen_closed_among(
    db: &impl ConnectionTrait,
    ids: Vec<i32>,
) -> DomainResult<(Vec<i32>, Vec<i32>)> {
    let mut cancelled = Vec::new();
    let mut reopened = Vec::new();
    for a in appointment::Entity::find()
        .filter(appointment::Column::Id.is_in(ids))
        .filter(
            Condition::any()
                .add(appointment::Column::CancelledOn.is_not_null())
                .add(appointment::Column::ClosedOn.is_not_null()),
        )
        .all(db)
        .await?
    {
        match Ending::of(&a)? {
            Ending::Cancelled(_) => cancelled.push(a.id),
            Ending::Closed(_) => reopened.push(a.id),
            Ending::Open => {}
        }
    }
    reopened.sort_unstable();
    if !reopened.is_empty() {
        appointment::Entity::update_many()
            .col_expr(
                appointment::Column::ClosedOn,
                Expr::value(None::<chrono::NaiveDate>),
            )
            .col_expr(appointment::Column::UpdatedAt, Expr::value(super::now()))
            .filter(appointment::Column::Id.is_in(reopened.clone()))
            .exec(db)
            .await?;
    }
    Ok((reopened, cancelled))
}

/// Clears both `closed_on` and `cancelled_on`. Jobs and to-dos that
/// cancelling took off the appointment stay off it.
///
/// # Errors
/// `NotFound` for an appointment that is not the vehicle's.
pub async fn reopen<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<appointment::Model> {
    let txn = super::begin_write(db).await?;
    let db = &txn;
    let existing = require(db, vehicle_id, id).await?;
    let mut active: appointment::ActiveModel = existing.into();
    Ending::Open.write(&mut active);
    active.updated_at = Set(super::now());
    let out = active.update(db).await?;
    txn.commit().await?;
    Ok(out)
}

/// Deletes an appointment. Its jobs, to-dos and services stay and lose the
/// appointment through the `ON DELETE SET NULL` foreign keys.
///
/// # Errors
/// `NotFound` for an appointment that is not the vehicle's.
pub async fn delete<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<()> {
    let txn = super::begin_write(db).await?;
    let db = &txn;
    require(db, vehicle_id, id).await?.delete(db).await?;
    txn.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::{part::Supplier, service_record, todo, work_item},
        error::DomainError,
        inputs::{
            need::NewNeed,
            service_record::NewServiceRecord,
            todo::NewTodo,
            work_item::{NewWorkItem, UpdateWorkItem},
        },
        services::{need, todo as todo_svc, work_item as job},
        test_support::{VehicleFixture, add_job, date, set_job_status, test_db},
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
            cancelled_on: cancelled_on.map(date),
            closed_on: closed_on.map(date),
            created_at: chrono::DateTime::default(),
            updated_at: chrono::DateTime::default(),
        }
    }

    #[test]
    fn status_table() {
        let closed = Ending::Closed(date("2026-11-02"));
        let cancelled = Ending::Cancelled(date("2026-11-01"));
        assert_eq!(status(Ending::Open, &[]), AppointmentStatus::Planning);
        assert_eq!(
            status(Ending::Open, &[JobStatus::Done, JobStatus::Dropped]),
            AppointmentStatus::Done
        );
        assert_eq!(
            status(Ending::Open, &[JobStatus::Done, JobStatus::Planned]),
            AppointmentStatus::Planning
        );
        assert_eq!(
            status(cancelled, &[JobStatus::Done]),
            AppointmentStatus::Cancelled
        );
        assert_eq!(
            status(closed, &[JobStatus::Planned]),
            AppointmentStatus::Done
        );
        let both = model(Some("2026-11-01"), Some("2026-11-02"));
        assert!(matches!(Ending::of(&both), Err(DomainError::Internal(_))));
    }

    #[tokio::test]
    async fn each_ending_reads_back_as_written() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = create(&db, v, appt("Brakes")).await.unwrap();
        for ending in [
            Ending::Closed(date("2026-11-01")),
            Ending::Cancelled(date("2026-11-02")),
            Ending::Open,
        ] {
            let mut active: appointment::ActiveModel = a.clone().into();
            ending.write(&mut active);
            let stored = active.update(&db).await.unwrap();
            assert_eq!(Ending::of(&stored).unwrap(), ending);
        }
    }

    #[test]
    fn phase_table() {
        use AppointmentPhase as P;
        use AppointmentStatus as S;
        let today = date("2026-11-18");
        let cases = [
            (S::Planning, None, P::Planning),
            (S::Planning, Some("2026-11-19"), P::Booked),
            (S::Planning, Some("2026-11-18"), P::InShop),
            (S::Planning, Some("2026-11-10"), P::InShop),
            (S::Done, Some("2026-12-01"), P::Done),
            (S::Done, None, P::Done),
            (S::Cancelled, Some("2026-11-10"), P::Cancelled),
        ];
        for (status, planned, expected) in cases {
            assert_eq!(
                phase(status, planned.map(date), today),
                expected,
                "{status:?} on {planned:?}"
            );
        }
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
        let shop_id = crate::test_support::add_shop(&db, "Example VW").await;
        let made = create(
            &db,
            v,
            NewAppointment {
                purpose: "Brakes".into(),
                shop: Some(crate::inputs::shop::ShopChoice::Name("example vw".into())),
                planned_date: Some(date("2026-12-01")),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(made.shop_id, Some(shop_id));
        let read = summary(&db, v, made.id, date("2026-11-13")).await.unwrap();
        assert_eq!(read.shop_name.as_deref(), Some("Example VW"));
        let missing_shop = create(
            &db,
            v,
            NewAppointment {
                purpose: "x".into(),
                shop: Some(crate::inputs::shop::ShopChoice::Id(9999)),
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
        assert_eq!(updated.shop_id, Some(shop_id));
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
            format!("Appointment {} is cancelled, so it takes no new work", a.id)
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
            format!("Appointment {} is closed, so it takes no new work", b.id)
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
            crate::services::today(),
        )
        .await
        .unwrap();
        assert_eq!(revised.appointment.appointment.purpose, "Brakes and plugs");
        assert_eq!(revised.appointment.status, AppointmentStatus::Cancelled);
        assert_eq!(revised.unassigned_job_ids, vec![planned]);

        let reopened = update_with_status(
            &db,
            v,
            a.id,
            UpdateAppointment::default(),
            Some(StatusChange::Reopen),
            crate::services::today(),
        )
        .await
        .unwrap();
        assert_eq!(reopened.appointment.status, AppointmentStatus::Planning);
        crate::test_support::assert_empty(&reopened.unassigned_job_ids);

        let closed = update_with_status(
            &db,
            v,
            a.id,
            UpdateAppointment::default(),
            Some(StatusChange::Close),
            crate::services::today(),
        )
        .await
        .unwrap();
        assert_eq!(closed.appointment.status, AppointmentStatus::Done);
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
            crate::services::today(),
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
            crate::services::today(),
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
        let missing = update_with_status(
            &db,
            other,
            99_999,
            UpdateAppointment::default(),
            None,
            crate::services::today(),
        )
        .await
        .unwrap_err();
        let wrong = update_with_status(
            &db,
            other,
            a.id,
            UpdateAppointment::default(),
            None,
            crate::services::today(),
        )
        .await
        .unwrap_err();
        assert!(matches!(wrong, DomainError::NotFound(_)));
        assert_eq!(
            wrong.to_string().replace(&a.id.to_string(), "N"),
            missing.to_string().replace("99999", "N")
        );
    }

    #[tokio::test]
    async fn close_refuses_open_jobs_and_cancel_takes_them_off_with_their_needs() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = create(&db, v, appt("Brakes")).await.unwrap();
        let planned = add_job(&db, v, "Brake pads", Some(a.id)).await;
        let finished = add_job(&db, v, "Brake pads", Some(a.id)).await;
        set_job_status(&db, v, finished, JobStatus::Done).await;
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

        let err = close(&db, v, a.id).await.unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "Appointment {} still has open work: job {planned} \"Brake pads\". Mark each one \
                 done or move it off the appointment before closing it",
                a.id
            )
        );
        assert!(require(&db, v, a.id).await.unwrap().closed_on.is_none());

        let cancelled = cancel(&db, v, a.id).await.unwrap();
        assert_eq!(cancelled.unassigned_job_ids, vec![planned]);
        assert!(cancelled.appointment.cancelled_on.is_some());
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

        let again = close(&db, v, a.id).await.unwrap_err();
        assert!(matches!(again, DomainError::BadRequest(_)));
        let reopened = reopen(&db, v, a.id).await.unwrap();
        assert!(reopened.cancelled_on.is_none());
        let closed = close(&db, v, a.id).await.unwrap();
        assert!(closed.appointment.closed_on.is_some());
        assert!(closed.unassigned_job_ids.is_empty() && closed.unassigned_todo_ids.is_empty());
        let again = cancel(&db, v, a.id).await.unwrap_err();
        assert!(matches!(again, DomainError::BadRequest(_)));
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
        let service = crate::services::service_record::create(
            &db,
            v,
            NewServiceRecord {
                service_date: date("2026-11-01"),
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
            .filter(service_record::Column::Id.eq(service.record.id))
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
        let service = service_record::Entity::find_by_id(service.record.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(service.appointment_id, None);
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
                planned_date: Some(date("2026-12-01")),
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
            (j1, "Rotors", Some(Supplier::Shop)),
            (j2, "Fluid", None),
        ] {
            need::add(
                &db,
                v,
                job_id,
                NewNeed {
                    name: name.into(),
                    supplied_by,
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

        let all = list(&db, v, true, date("2026-11-18")).await.unwrap();
        let s = all.iter().find(|s| s.appointment.id == a.id).unwrap();
        assert_eq!(
            (s.status, s.phase, s.jobs, s.needs_open, s.todos_open),
            (
                AppointmentStatus::Planning,
                AppointmentPhase::Booked,
                2,
                2,
                1
            )
        );
        let u = all
            .iter()
            .find(|s| s.appointment.id == unbooked.id)
            .unwrap();
        assert_eq!(
            (u.phase, u.jobs, u.needs_open, u.todos_open),
            (AppointmentPhase::Planning, 0, 0, 0)
        );

        cancel(&db, v, unbooked.id).await.unwrap();
        let open_only = list(&db, v, false, crate::services::today()).await.unwrap();
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
        set_job_status(&db, v, job_id, JobStatus::Done).await;
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

        let open = list(&db, v, false, crate::services::today()).await.unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(
            (open[0].status, open[0].todos_open),
            (AppointmentStatus::Done, 1)
        );

        todo_svc::complete(&db, v, t.id, None).await.unwrap();
        crate::test_support::assert_empty(
            &list(&db, v, false, crate::services::today()).await.unwrap(),
        );
    }

    #[tokio::test]
    async fn a_job_set_back_to_planned_reopens_a_hand_closed_appointment() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let closed = create(&db, v, appt("Brakes")).await.unwrap();
        let cancelled = create(&db, v, appt("Wipers")).await.unwrap();
        let on_closed = add_job(&db, v, "Brake pads", Some(closed.id)).await;
        let on_cancelled = add_job(&db, v, "Brake pads", Some(cancelled.id)).await;
        let set_status = |job_id, status: JobStatus| {
            job::update(
                &db,
                v,
                job_id,
                UpdateWorkItem {
                    status: Some(status),
                    ..Default::default()
                },
            )
        };
        set_status(on_closed, JobStatus::Done).await.unwrap();
        set_status(on_cancelled, JobStatus::Dropped).await.unwrap();
        close(&db, v, closed.id).await.unwrap();
        cancel(&db, v, cancelled.id).await.unwrap();

        let reopening = set_status(on_closed, JobStatus::Planned).await.unwrap();
        assert_eq!(reopening.reopened_appointment_id, Some(closed.id));
        assert_eq!(reopening.left_cancelled_appointment_id, None);
        assert_eq!(reopening.work_item.appointment_id, Some(closed.id));
        // The returned job says where it ended up, not where it was.
        let returned = set_status(on_cancelled, JobStatus::Planned).await.unwrap();
        assert_eq!(returned.work_item.appointment_id, None);
        assert_eq!(returned.left_cancelled_appointment_id, Some(cancelled.id));
        assert_eq!(returned.reopened_appointment_id, None);

        let reopened = summary(&db, v, closed.id, crate::services::today())
            .await
            .unwrap();
        assert_eq!(reopened.status, AppointmentStatus::Planning);
        assert_eq!(reopened.appointment.closed_on, None);
        assert_eq!(
            summary(&db, v, cancelled.id, crate::services::today())
                .await
                .unwrap()
                .status,
            AppointmentStatus::Cancelled
        );
        let unscheduled = job::get(&db, v, on_cancelled).await.unwrap();
        assert_eq!(unscheduled.appointment_id, None);
    }

    #[tokio::test]
    async fn a_job_set_back_to_planned_on_an_open_appointment_moves_nothing() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let open = create(&db, v, appt("Brakes")).await.unwrap();
        let job_id = add_job(&db, v, "Brake pads", Some(open.id)).await;
        set_job_status(&db, v, job_id, JobStatus::Done).await;

        let returned = job::update(
            &db,
            v,
            job_id,
            UpdateWorkItem {
                status: Some(JobStatus::Planned),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(returned.reopened_appointment_id, None);
        assert_eq!(returned.left_cancelled_appointment_id, None);
        assert_eq!(returned.work_item.appointment_id, Some(open.id));
    }

    #[tokio::test]
    async fn a_job_update_may_name_the_closed_appointment_it_is_already_on() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let closed = create(&db, v, appt("Brakes")).await.unwrap();
        let other = create(&db, v, appt("Wipers")).await.unwrap();
        let on_closed = add_job(&db, v, "Brake pads", Some(closed.id)).await;
        let elsewhere = add_job(&db, v, "Wiper blades", Some(other.id)).await;
        set_job_status(&db, v, on_closed, JobStatus::Done).await;
        set_job_status(&db, v, elsewhere, JobStatus::Done).await;
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
        assert_eq!(kept.work_item.appointment_id, Some(closed.id));
        assert_eq!(kept.work_item.notes.as_deref(), Some("Pads only"));

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
    async fn close_refuses_an_open_todo_and_cancel_takes_it_off() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = create(&db, v, appt("Brakes")).await.unwrap();
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
        .unwrap()
        .id;
        let err = close(&db, v, a.id).await.unwrap_err();
        assert!(
            err.to_string()
                .contains(&format!("to-do {t} \"Bring wheel locks\"")),
            "{err}"
        );

        let cancelled = cancel(&db, v, a.id).await.unwrap();
        assert_eq!(cancelled.unassigned_todo_ids, vec![t]);
        crate::test_support::assert_empty(
            &list(&db, v, false, crate::services::today()).await.unwrap(),
        );
        assert_eq!(
            todo_svc::require(&db, v, t).await.unwrap().appointment_id,
            None
        );
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
        set_job_status(&db, v, dropped, JobStatus::Dropped).await;
        let all = list(&db, v, true, crate::services::today()).await.unwrap();
        assert_eq!(all[0].needs_open, 1);
    }

    // The first connection is midway through putting a job on the
    // appointment. A close on a second connection waits for it and then takes
    // that job off, so the closed appointment holds no planned job.
    #[tokio::test]
    async fn close_waits_for_a_job_being_assigned_on_another_connection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("two.db");
        let first = crate::db::connect(path.to_str().unwrap()).await.unwrap();
        let second = crate::db::connect(path.to_str().unwrap()).await.unwrap();
        let v = VehicleFixture::new().insert(&first).await.id;
        let a = create(
            &first,
            v,
            NewAppointment {
                purpose: "Brakes".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;

        let txn = crate::services::begin_write(&first).await.unwrap();
        require_assignable(&txn, v, a).await.unwrap();
        let job = work_item::ActiveModel {
            vehicle_id: Set(v),
            title: Set("Pads".into()),
            appointment_id: Set(Some(a)),
            status: Set(JobStatus::Planned),
            ..Default::default()
        }
        .insert(&txn)
        .await
        .unwrap()
        .id;

        let closing = tokio::spawn(async move { close(&second, v, a).await });
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(
            !closing.is_finished(),
            "the close ran ahead of the open write"
        );
        txn.commit().await.unwrap();

        // The close read the job the open write committed, so it refuses.
        let err = closing.await.unwrap().unwrap_err();
        assert!(err.to_string().contains(&format!("job {job} ")), "{err}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn racing_creates_reopens_and_deletes_all_succeed() {
        let (_dir, pools) = crate::test_support::race_pools().await;
        let first = &pools[0];
        let v = VehicleFixture::new().insert_id(first).await;
        let mut ids = Vec::new();
        for i in 0..pools.len() * 2 {
            ids.push(
                create(first, v, appt(&format!("Visit {i}")))
                    .await
                    .unwrap()
                    .id,
            );
        }
        let results = crate::test_support::race(&pools, |i, db| {
            let id = ids[i];
            async move {
                match i % 3 {
                    0 => create(&db, v, appt("New visit")).await.map(|_| ()),
                    1 => reopen(&db, v, id).await.map(|_| ()),
                    _ => delete(&db, v, id).await,
                }
            }
        })
        .await;
        assert!(results.iter().all(Result::is_ok), "{results:?}");
    }
}
