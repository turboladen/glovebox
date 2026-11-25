//! The checklist view: an appointment's (or the unscheduled jobs') needs
//! grouped into the next steps the owner takes, the same needs grouped by
//! job, and the decisions made before for a kind of part.

use std::collections::HashMap;

use sea_orm::{
    sea_query::{Expr, LikeExpr},
    *,
};
use serde::Serialize;

use super::shop::ShopRef;

use crate::{
    entities::{
        job_need,
        need_option::{self, Fitment},
        part::{self, Supplier},
        service_record, todo, vehicle,
        work_item::{self, JobStatus},
    },
    error::{DomainError, DomainResult},
    inputs::checklist::ChecklistScope,
    money::format_cents,
    services::{
        activity::{self, ActivityItem},
        appointment::{self, AppointmentSummary},
        budget::{self, BudgetForecast},
        incident::ServiceRef,
        need::{self, Stage, stage},
        reminders, todo as todo_svc, upcoming,
        waiting::{self, Waiting},
    },
};

/// Everything the owner reads to prepare a visit.
#[derive(Debug, Serialize)]
pub struct Checklist {
    /// The appointment, or `None` for the unscheduled view.
    pub appointment: Option<AppointmentSummary>,
    pub next_steps: NextSteps,
    pub jobs: Vec<JobView>,
    /// Every to-do of the appointment, done ones included.
    pub todos: Vec<todo::Model>,
    /// Each note about a need, with the need and job it belongs to. It is
    /// serialized as `warnings`, the lines [`Checklist::warnings`] writes.
    #[serde(rename = "warnings", serialize_with = "serialize_warnings")]
    pub checks: Vec<Check>,
}

impl Checklist {
    /// Each need's notes, prefixed with the need's name.
    #[must_use]
    pub fn warnings(&self) -> Vec<String> {
        warning_lines(&self.checks)
    }
}

fn warning_lines(checks: &[Check]) -> Vec<String> {
    checks
        .iter()
        .map(|c| format!("{}: {}", c.need_name, c.note))
        .collect()
}

fn serialize_warnings<S: serde::Serializer>(checks: &[Check], out: S) -> Result<S::Ok, S::Error> {
    out.collect_seq(warning_lines(checks))
}

/// The note on a need whose part is linked while no option is chosen. A page
/// may show it in a shorter form of its own.
pub const NO_DECISION_NOTE: &str = "A part is linked and no decision is recorded.";

/// One note about a need, for a page that shows it in a row of its own.
#[derive(Debug)]
pub struct Check {
    pub need_id: i32,
    pub need_name: String,
    pub job_title: String,
    pub note: String,
}

/// What to do next, in the order the owner does it. Rows are ordered by job,
/// then need id. Needs on a dropped job appear in no group.
#[derive(Debug, Serialize)]
pub struct NextSteps {
    pub decide: Vec<StepRow>,
    pub order: Vec<StepRow>,
    pub arriving: Vec<StepRow>,
    /// The open to-dos.
    pub todos: Vec<todo::Model>,
    /// Shop-supplied, received and installed needs, and every need of a done job.
    pub done: Vec<StepRow>,
}

/// One need in a next-steps group, with a one-sentence summary of what is
/// asked of the owner.
#[derive(Debug, Serialize)]
pub struct StepRow {
    pub need_id: i32,
    pub job_id: i32,
    pub job_title: String,
    pub name: String,
    /// The sentence "{name}: {detail}".
    pub summary: String,
    /// The summary without its leading "{name}: ", so a page can show the
    /// name in its own column.
    #[serde(skip)]
    pub detail: String,
    pub stage: Stage,
}

/// A job with its needs.
#[derive(Debug, Serialize)]
pub struct JobView {
    pub job: work_item::Model,
    pub needs: Vec<NeedView>,
    /// The service that completed the job, when one did.
    // A tool reply names that service by the job's `completed_service_id`,
    // so the key stays out of JSON.
    #[serde(skip)]
    pub completed_by: Option<ServiceRef>,
}

impl JobView {
    /// How many of the job's needs still ask something of the owner, which is
    /// none unless the job is planned.
    #[must_use]
    pub fn open_needs(&self) -> usize {
        self.needs
            .iter()
            .filter(|n| need::counts_as_open(self.job.status, n.stage))
            .count()
    }
}

/// A need with its stage, its decision and its purchase.
#[derive(Debug, Serialize)]
pub struct NeedView {
    pub need: job_need::Model,
    pub stage: Stage,
    pub chosen: Option<need_option::Model>,
    /// Every option other than the chosen one, rejected ones included.
    pub other_options: Vec<need_option::Model>,
    /// The linked part, with its kit contents.
    pub part: Option<super::part::PartView>,
    /// The linked part's seller, date, price and order number, as present.
    pub purchase_summary: Option<String>,
    pub notes: Vec<String>,
}

impl NeedView {
    /// The linked part's own record, without its kit contents.
    #[must_use]
    pub fn linked_part(&self) -> Option<&part::Model> {
        self.part.as_ref().map(|p| &p.part)
    }

    /// A sentence counting the need's options and how many are rejected,
    /// such as `3 options researched, 1 rejected`.
    #[must_use]
    pub fn options_note(&self) -> String {
        let all: Vec<&need_option::Model> = self.chosen.iter().chain(&self.other_options).collect();
        let n = all.len();
        let noun = if n == 1 { "option" } else { "options" };
        let rejected = all.iter().filter(|o| need::is_rejected(o)).count();
        if rejected > 0 {
            format!("{n} {noun} researched, {rejected} rejected")
        } else {
            format!("{n} {noun} researched")
        }
    }
}

/// A need matching a past-decisions search, with how it was decided.
#[derive(Debug, Serialize)]
pub struct PastDecision {
    pub vehicle_id: i32,
    pub vehicle_name: String,
    pub job_title: String,
    pub need: job_need::Model,
    pub chosen: Option<need_option::Model>,
    /// Holds the options that were ruled out: every rejected one and every
    /// one marked `does_not_fit`. One that had been chosen first carries the
    /// reason and date of that choice as `chosen_reason` and `chosen_on`.
    pub rejected: Vec<need_option::Model>,
    /// Parts bought for the need that went back.
    pub returned_parts: Vec<super::part::PartView>,
}

/// The checklist of one appointment, with its phase on `today`.
///
/// # Errors
/// `NotFound` for an appointment that is not the vehicle's.
pub async fn for_appointment(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    appointment_id: i32,
    today: chrono::NaiveDate,
) -> DomainResult<Checklist> {
    let found = appointment::require(db, vehicle_id, appointment_id).await?;
    let jobs = work_item::Entity::find()
        .filter(work_item::Column::AppointmentId.eq(appointment_id))
        .order_by_asc(work_item::Column::Id)
        .all(db)
        .await?;
    let todos =
        todo_svc::list_unchecked(db, &[vehicle_id], Some(Some(appointment_id)), true).await?;
    let views = job_views(db, jobs).await?;
    // The counts come from the rows the checklist shows, the same way
    // `appointment::summary` counts them.
    let job_statuses: Vec<JobStatus> = views.iter().map(|j| j.job.status).collect();
    let needs_open = views.iter().map(JobView::open_needs).sum();
    let todos_open = todos.iter().filter(|t| t.done_on.is_none()).count();
    let shop = super::shop::ref_of(db, found.shop_id).await?;
    let summary =
        appointment::summarize(found, &job_statuses, needs_open, todos_open, shop, today)?;
    Ok(assemble(Some(summary), views, todos))
}

/// The checklist of the vehicle's planned jobs that are on no appointment and
/// have needs, with the to-dos that are on no appointment.
///
/// # Errors
/// `NotFound` for a vehicle that does not exist.
pub async fn for_unscheduled(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<Checklist> {
    let todos = todo_svc::list(db, vehicle_id, Some(None), true).await?;
    let views = unscheduled_jobs(db, &[vehicle_id]).await?;
    Ok(assemble(None, views, todos))
}

// Loads the planned jobs of `vehicle_ids` that are on no appointment and have
// needs, in id order.
async fn unscheduled_jobs(
    db: &impl ConnectionTrait,
    vehicle_ids: &[i32],
) -> DomainResult<Vec<JobView>> {
    let jobs = work_item::Entity::find()
        .filter(work_item::Column::VehicleId.is_in(vehicle_ids.iter().copied()))
        .filter(work_item::Column::AppointmentId.is_null())
        .filter(work_item::Column::Status.eq(JobStatus::Planned))
        .order_by_asc(work_item::Column::Id)
        .all(db)
        .await?;
    let mut views = job_views(db, jobs).await?;
    views.retain(|j| !j.needs.is_empty());
    Ok(views)
}

/// Counts, for each of `jobs`, the needs that are still open.
pub(crate) async fn open_needs_by_job(
    db: &impl ConnectionTrait,
    jobs: Vec<work_item::Model>,
) -> DomainResult<HashMap<i32, usize>> {
    Ok(job_views(db, jobs)
        .await?
        .iter()
        .map(|view| (view.job.id, view.open_needs()))
        .collect())
}

/// One job with its needs.
///
/// # Errors
/// `NotFound` for a job that is not the vehicle's.
pub async fn for_job(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    work_item_id: i32,
) -> DomainResult<JobView> {
    let job = super::work_item::get(db, vehicle_id, work_item_id).await?;
    job_views(db, vec![job])
        .await?
        .pop()
        .ok_or_else(|| DomainError::NotFound(format!("Work item {work_item_id} not found")))
}

/// The scope that an appointment id, a job id or neither names: that
/// appointment, that job, or the unscheduled jobs.
///
/// # Errors
/// `Invalid` on `appointment_id` when both ids are given.
pub fn scope(
    appointment_id: Option<i32>,
    work_item_id: Option<i32>,
) -> DomainResult<ChecklistScope> {
    match (appointment_id, work_item_id) {
        (Some(_), Some(_)) => Err(DomainError::invalid(
            "appointment_id",
            "pass either appointment_id or work_item_id, not both",
        )),
        (Some(id), None) => Ok(ChecklistScope::Appointment(id)),
        (None, Some(id)) => Ok(ChecklistScope::Job(id)),
        (None, None) => Ok(ChecklistScope::Unscheduled),
    }
}

/// What [`get`] read: a checklist, or one job with its needs.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ChecklistRead {
    Checklist(Checklist),
    Job(JobView),
}

/// Reads the checklist `scope` names, through [`for_appointment`],
/// [`for_job`] or [`for_unscheduled`], with an appointment's phase on `today`.
///
/// # Errors
/// As for the function that reads the scope.
pub async fn get(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    scope: ChecklistScope,
    today: chrono::NaiveDate,
) -> DomainResult<ChecklistRead> {
    match scope {
        ChecklistScope::Appointment(id) => for_appointment(db, vehicle_id, id, today)
            .await
            .map(ChecklistRead::Checklist),
        ChecklistScope::Job(id) => for_job(db, vehicle_id, id).await.map(ChecklistRead::Job),
        ChecklistScope::Unscheduled => for_unscheduled(db, vehicle_id)
            .await
            .map(ChecklistRead::Checklist),
    }
}

/// The needs, across all vehicles, whose name or any option's description,
/// manufacturer or part number contains `query` (case-insensitively), newest
/// decision first.
///
/// # Errors
/// `Invalid` for a blank query.
pub async fn past_decisions(
    db: &impl ConnectionTrait,
    query: &str,
) -> DomainResult<Vec<PastDecision>> {
    let query = query.trim();
    if query.is_empty() {
        return Err(DomainError::invalid("query", "must not be blank"));
    }
    // LIKE wildcards in the search text match themselves.
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let pattern = format!("%{escaped}%");
    let like = || LikeExpr::new(pattern.clone()).escape('\\');

    let option_matches = need_option::Entity::find()
        .filter(
            Condition::any()
                .add(Expr::col(need_option::Column::Description).like(like()))
                .add(Expr::col(need_option::Column::Manufacturer).like(like()))
                .add(Expr::col(need_option::Column::PartNumber).like(like())),
        )
        .all(db)
        .await?;
    let option_need_ids: Vec<i32> = option_matches.iter().map(|o| o.need_id).collect();
    let mut needs = job_need::Entity::find()
        .filter(
            Condition::any()
                .add(Expr::col(job_need::Column::Name).like(like()))
                .add(job_need::Column::Id.is_in(option_need_ids)),
        )
        .all(db)
        .await?;
    // Newest decision first; needs never decided come last, newest first.
    needs.sort_by(|a, b| {
        (b.decided_on.is_some(), &b.decided_on, b.id).cmp(&(
            a.decided_on.is_some(),
            &a.decided_on,
            a.id,
        ))
    });

    let need_ids: Vec<i32> = needs.iter().map(|n| n.id).collect();
    let job_ids: Vec<i32> = needs.iter().map(|n| n.work_item_id).collect();
    let jobs: HashMap<i32, work_item::Model> = work_item::Entity::find()
        .filter(work_item::Column::Id.is_in(job_ids))
        .all(db)
        .await?
        .into_iter()
        .map(|j| (j.id, j))
        .collect();
    let vehicle_ids: Vec<i32> = jobs.values().map(|j| j.vehicle_id).collect();
    let vehicles: HashMap<i32, vehicle::Model> = vehicle::Entity::find()
        .filter(vehicle::Column::Id.is_in(vehicle_ids))
        .all(db)
        .await?
        .into_iter()
        .map(|v| (v.id, v))
        .collect();
    let mut options = options_by_need(db, &need_ids).await?;
    let mut returned: HashMap<i32, Vec<super::part::PartView>> = HashMap::new();
    let returned_parts = part::Entity::find()
        .filter(part::Column::ReturnedFromNeedId.is_in(need_ids))
        .order_by_asc(part::Column::Id)
        .all(db)
        .await?;
    for p in super::part::views(db, returned_parts).await? {
        if let Some(need_id) = p.part.returned_from_need_id {
            returned.entry(need_id).or_default().push(p);
        }
    }

    let mut out = Vec::with_capacity(needs.len());
    for need in needs {
        let Some(job) = jobs.get(&need.work_item_id) else {
            continue;
        };
        let all = options.remove(&need.id).unwrap_or_default();
        let chosen = all
            .iter()
            .find(|o| Some(o.id) == need.chosen_option_id)
            .cloned();
        // An option marked `does_not_fit` is ruled out as surely as a rejected
        // one, and it keeps the reason of a choice the same way.
        let rejected = all.into_iter().filter(need::is_ruled_out).collect();
        out.push(PastDecision {
            vehicle_id: job.vehicle_id,
            vehicle_name: vehicles
                .get(&job.vehicle_id)
                .map_or_else(String::new, |v| v.name.clone()),
            job_title: job.title.clone(),
            returned_parts: returned.remove(&need.id).unwrap_or_default(),
            need,
            chosen,
            rejected,
        });
    }
    Ok(out)
}

async fn options_by_need(
    db: &impl ConnectionTrait,
    need_ids: &[i32],
) -> DomainResult<HashMap<i32, Vec<need_option::Model>>> {
    let mut grouped: HashMap<i32, Vec<need_option::Model>> = HashMap::new();
    for option in need_option::Entity::find()
        .filter(need_option::Column::NeedId.is_in(need_ids.iter().copied()))
        .order_by_asc(need_option::Column::Id)
        .all(db)
        .await?
    {
        grouped.entry(option.need_id).or_default().push(option);
    }
    Ok(grouped)
}

// Loads the needs, options, parts and kit contents of every job in four
// batched queries and returns the views in the order of `jobs`.
async fn job_views(
    db: &impl ConnectionTrait,
    jobs: Vec<work_item::Model>,
) -> DomainResult<Vec<JobView>> {
    let job_ids: Vec<i32> = jobs.iter().map(|j| j.id).collect();
    let needs = job_need::Entity::find()
        .filter(job_need::Column::WorkItemId.is_in(job_ids))
        .order_by_asc(job_need::Column::Id)
        .all(db)
        .await?;
    let need_ids: Vec<i32> = needs.iter().map(|n| n.id).collect();
    let mut options = options_by_need(db, &need_ids).await?;
    let parts = part::Entity::find()
        .filter(part::Column::Id.is_in(needs.iter().filter_map(|n| n.part_id)))
        .all(db)
        .await?;
    let mut parts: HashMap<i32, super::part::PartView> = super::part::views(db, parts)
        .await?
        .into_iter()
        .map(|p| (p.part.id, p))
        .collect();
    let mut by_job: HashMap<i32, Vec<NeedView>> = HashMap::new();
    for need in needs {
        // A part links to at most one need, so each is moved out once.
        let linked = need.part_id.and_then(|id| parts.remove(&id));
        let job_id = need.work_item_id;
        let own_options = options.remove(&need.id).unwrap_or_default();
        by_job
            .entry(job_id)
            .or_default()
            .push(need_view(need, own_options, linked));
    }
    let completing: HashMap<i32, ServiceRef> = service_record::Entity::find()
        .filter(
            service_record::Column::Id.is_in(jobs.iter().filter_map(|j| j.completed_service_id)),
        )
        .all(db)
        .await?
        .into_iter()
        .map(|s| (s.id, ServiceRef::from(s)))
        .collect();
    Ok(jobs
        .into_iter()
        .map(|job| JobView {
            needs: by_job.remove(&job.id).unwrap_or_default(),
            completed_by: job
                .completed_service_id
                .and_then(|id| completing.get(&id).cloned()),
            job,
        })
        .collect())
}

fn need_view(
    need: job_need::Model,
    options: Vec<need_option::Model>,
    part: Option<super::part::PartView>,
) -> NeedView {
    let (chosen, other_options): (Vec<_>, Vec<_>) = options
        .into_iter()
        .partition(|o| Some(o.id) == need.chosen_option_id);
    let chosen = chosen.into_iter().next();
    let mut notes = Vec::new();
    if chosen
        .as_ref()
        .is_some_and(|c| c.fitment == Fitment::Unconfirmed)
    {
        notes.push("Fitment of the chosen option is not confirmed.".to_string());
    }
    if let (Some(part), Some(chosen)) = (&part, &chosen)
        && let (Some(have), Some(want)) = (&part.part.part_number, &chosen.part_number)
        && !have.trim().eq_ignore_ascii_case(want.trim())
    {
        notes.push(format!(
            "The linked part's number ({have}) differs from the chosen option's ({want})."
        ));
    }
    // A shop-supplied need asks no decision of the owner.
    if part.is_some() && chosen.is_none() && need.supplied_by != Some(Supplier::Shop) {
        notes.push(NO_DECISION_NOTE.to_string());
    }
    NeedView {
        stage: stage(&need, part.as_ref().map(|p| &p.part)),
        purchase_summary: part.as_ref().and_then(|p| purchase_summary(&p.part)),
        need,
        chosen,
        other_options,
        part,
        notes,
    }
}

// Writes the purchase as a sentence such as "Bought from FCP Euro on
// 2026-11-13 for $162.00, order FCP-10458", leaving out what is absent.
fn purchase_summary(part: &part::Model) -> Option<String> {
    let clauses: Vec<String> = [
        part.seller.as_ref().map(|s| format!("from {s}")),
        part.purchase_date.as_ref().map(|d| format!("on {d}")),
        part.cost_cents.map(|c| format!("for {}", format_cents(c))),
    ]
    .into_iter()
    .flatten()
    .collect();
    let order = part.order_ref.as_ref().map(|r| format!("order {r}"));
    match (clauses.is_empty(), order) {
        (true, None) => None,
        (true, Some(order)) => Some(format!("Bought, {order}")),
        (false, None) => Some(format!("Bought {}", clauses.join(" "))),
        (false, Some(order)) => Some(format!("Bought {}, {order}", clauses.join(" "))),
    }
}

fn assemble(
    appointment: Option<AppointmentSummary>,
    jobs: Vec<JobView>,
    todos: Vec<todo::Model>,
) -> Checklist {
    let next_steps = next_steps(&jobs, &todos);
    let checks: Vec<Check> = jobs
        .iter()
        .filter(|j| j.job.status != JobStatus::Dropped)
        .flat_map(|j| j.needs.iter().map(move |n| (j, n)))
        .flat_map(|(j, n)| {
            n.notes.iter().map(move |note| Check {
                need_id: n.need.id,
                need_name: n.need.name.clone(),
                job_title: j.job.title.clone(),
                note: note.clone(),
            })
        })
        .collect();
    Checklist {
        appointment,
        next_steps,
        jobs,
        todos,
        checks,
    }
}

fn next_steps(jobs: &[JobView], todos: &[todo::Model]) -> NextSteps {
    let mut steps = NextSteps {
        decide: Vec::new(),
        order: Vec::new(),
        arriving: Vec::new(),
        todos: todos
            .iter()
            .filter(|t| t.done_on.is_none())
            .cloned()
            .collect(),
        done: Vec::new(),
    };
    for job in jobs {
        let job_done = match job.job.status {
            JobStatus::Dropped => continue,
            JobStatus::Done => true,
            JobStatus::Planned => false,
        };
        for view in &job.needs {
            let detail = need_detail(view, job.job.status);
            let group = match view.stage {
                _ if job_done => &mut steps.done,
                Stage::Undecided => &mut steps.decide,
                Stage::ToOrder => &mut steps.order,
                Stage::Ordered => &mut steps.arriving,
                Stage::ShopSupplies | Stage::Received | Stage::Installed => &mut steps.done,
            };
            group.push(StepRow {
                need_id: view.need.id,
                job_id: job.job.id,
                job_title: job.job.title.clone(),
                name: view.need.name.clone(),
                summary: format!("{}: {detail}", view.need.name),
                detail,
                stage: view.stage,
            });
        }
    }
    steps
}

// Names an option in a sentence: by manufacturer and part number when it has
// a part number, otherwise by its description.
fn option_label(option: &need_option::Model) -> String {
    super::part::label(
        option.manufacturer.as_deref(),
        option.part_number.as_deref(),
    )
    .unwrap_or_else(|| option.description.clone())
}

/// Whether a need is still open on a job that is done or dropped, so it asks
/// nothing of the owner whatever its stage says.
#[must_use]
pub fn open_on_closed_job(view: &NeedView, job_status: JobStatus) -> bool {
    job_status != JobStatus::Planned && view.stage.is_open()
}

/// The sentence a need's row shows, without the need's name: what is asked of
/// the owner, or what settled it. A need still open on a done job reads "the
/// job is done", and one on a dropped job "the job is dropped".
pub fn need_detail(view: &NeedView, job_status: JobStatus) -> String {
    if !open_on_closed_job(view, job_status) {
        return step_detail(view);
    }
    match job_status {
        JobStatus::Dropped => "the job is dropped".to_owned(),
        JobStatus::Done | JobStatus::Planned => "the job is done".to_owned(),
    }
}

// Returns what is asked of the owner for a need of a job that is not done,
// or what settled it, as a sentence without the need's name.
fn step_detail(view: &NeedView) -> String {
    match view.stage {
        Stage::Undecided if view.chosen.is_some() => "who supplies it is not decided".to_owned(),
        Stage::Undecided => "no part chosen".to_owned(),
        Stage::ToOrder => {
            let what = view.chosen.as_ref().map_or_else(String::new, |c| {
                let from = c
                    .seller
                    .as_ref()
                    .map_or_else(String::new, |s| format!(" from {s}"));
                let count = if view.need.quantity > 1 {
                    format!("{} \u{d7} ", view.need.quantity)
                } else {
                    String::new()
                };
                format!(" {count}{}{from}", option_label(c))
            });
            format!("order{what}")
        }
        Stage::Ordered => {
            let seller = view.linked_part().and_then(|p| p.seller.as_ref());
            let date = view.linked_part().and_then(|p| p.purchase_date.as_ref());
            format!(
                "ordered{}{}",
                seller.map_or_else(String::new, |s| format!(" from {s}")),
                date.map_or_else(String::new, |d| format!(" on {d}"))
            )
        }
        Stage::ShopSupplies => "the shop supplies it".to_owned(),
        Stage::Received => match view.linked_part().and_then(|p| p.received_on.as_ref()) {
            Some(date) => format!("received on {date}"),
            None => "received".to_owned(),
        },
        Stage::Installed => "installed".to_owned(),
    }
}

/// Holds one vehicle's open work for the home page.
#[derive(Debug, Serialize)]
pub struct VehicleHome {
    pub vehicle: vehicle::Model,
    /// Lists the open appointments, soonest planned date first, unbooked ones last.
    pub appointments: Vec<HomeAppointment>,
    /// Holds what is not yet on an appointment: due maintenance, planned
    /// jobs, open recalls and open incidents.
    pub waiting: Waiting,
    /// Lists the open to-dos that are on no appointment.
    pub todos: Vec<todo::Model>,
    /// The 12-month cost forecast, judged from the same reminders and planned
    /// jobs as `waiting`.
    pub budget: BudgetForecast,
    /// The latest [`HOME_ACTIVITY_LIMIT`] services, incidents and odometer
    /// readings, newest first.
    pub recent_activity: Vec<ActivityItem>,
}

/// How many recent records [`home`] lists for each vehicle.
pub const HOME_ACTIVITY_LIMIT: usize = 3;

/// Describes an open appointment and what is still open on it.
#[derive(Debug, Serialize)]
pub struct HomeAppointment {
    pub id: i32,
    pub purpose: String,
    /// Names the appointment's shop. It prints as `shop_id` and `shop_name`,
    /// both null when the appointment names no shop.
    #[serde(flatten, serialize_with = "ShopRef::id_and_shop_name")]
    pub shop: Option<ShopRef>,
    /// Holds the planned date, or `not booked` when there is none.
    pub when: String,
    /// True when the appointment has a planned date.
    pub dated: bool,
    /// Holds the appointment's phase on the day the home page was read.
    pub phase: super::appointment::AppointmentPhase,
    /// Counts the jobs on the appointment.
    pub jobs: usize,
    /// Counts the needs that are still open.
    pub needs_open: usize,
    /// Counts the to-dos that are still open.
    pub todos_open: usize,
}

/// What the home page shows for every vehicle that is not archived: what is
/// waiting for an appointment, its open appointments, its to-dos that are
/// on no appointment, its 12-month forecast and its latest records. An
/// appointment whose jobs are all done stays listed while it has an open
/// to-do.
///
/// # Errors
/// `Db` when a query fails.
pub async fn home(
    db: &impl ConnectionTrait,
    today: chrono::NaiveDate,
) -> DomainResult<Vec<VehicleHome>> {
    // The appointments and the to-dos of every vehicle come from one batched
    // query each. The rest is read per vehicle, through the functions the
    // tools use, so the page and the tools agree: the reminders and the
    // planned jobs are read once, and both the waiting lists and the forecast
    // are judged from them.
    let vehicles = super::vehicle::list(db, false).await?;
    let ids: Vec<i32> = vehicles.iter().map(|v| v.id).collect();
    let mut appointments = appointment::open_by_vehicle(db, &ids, today).await?;
    let mut todos: HashMap<i32, Vec<todo::Model>> = HashMap::new();
    for open in todo_svc::list_unchecked(db, &ids, Some(None), false).await? {
        todos.entry(open.vehicle_id).or_default().push(open);
    }
    let mut homes = Vec::with_capacity(vehicles.len());
    for vehicle in vehicles {
        let reminders = reminders::calculate_reminders_on(db, vehicle.id, today).await?;
        let planned = upcoming::planned_work(db, vehicle.id, today).await?;
        homes.push(VehicleHome {
            waiting: waiting::from_parts(db, vehicle.id, &reminders, &planned).await?,
            budget: budget::forecast_from(db, &reminders, &planned, today).await?,
            recent_activity: activity::recent_of(db, &vehicle, HOME_ACTIVITY_LIMIT).await?,
            appointments: appointments
                .remove(&vehicle.id)
                .unwrap_or_default()
                .into_iter()
                .map(home_appointment)
                .collect(),
            todos: todos.remove(&vehicle.id).unwrap_or_default(),
            vehicle,
        });
    }
    Ok(homes)
}

fn home_appointment(s: AppointmentSummary) -> HomeAppointment {
    HomeAppointment {
        id: s.appointment.id,
        purpose: s.appointment.purpose,
        shop: s.shop,
        when: super::appointment::when(s.appointment.planned_date),
        dated: s.appointment.planned_date.is_some(),
        phase: s.phase,
        jobs: s.jobs,
        needs_open: s.needs_open,
        todos_open: s.todos_open,
    }
}

#[cfg(test)]
mod tests;
