//! The checklist view: an appointment's (or the unscheduled jobs') needs
//! grouped into the next steps the owner takes, the same needs grouped by
//! job, and the decisions made before for a kind of part.

use std::collections::HashMap;

use sea_orm::{
    sea_query::{Expr, LikeExpr},
    *,
};
use serde::Serialize;

use crate::{
    entities::{job_need, need_option, part, todo, vehicle, work_item},
    error::{DomainError, DomainResult},
    money::format_cents,
    services::{
        appointment::{self, AppointmentSummary},
        need::{Stage, stage},
        todo as todo_svc,
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
    /// Each need's notes, prefixed with the need's name.
    pub warnings: Vec<String>,
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
    pub summary: String,
    pub stage: Stage,
}

/// A job with its needs.
#[derive(Debug, Serialize)]
pub struct JobView {
    pub job: work_item::Model,
    pub needs: Vec<NeedView>,
}

/// A need with its stage, its decision and its purchase.
#[derive(Debug, Serialize)]
pub struct NeedView {
    pub need: job_need::Model,
    pub stage: Stage,
    pub chosen: Option<need_option::Model>,
    /// Every option other than the chosen one, rejected ones included.
    pub other_options: Vec<need_option::Model>,
    pub part: Option<part::Model>,
    /// The linked part's seller, date, price and order number, as present.
    pub purchase_summary: Option<String>,
    pub notes: Vec<String>,
}

/// A need matching a past-decisions search, with how it was decided.
#[derive(Debug, Serialize)]
pub struct PastDecision {
    pub vehicle_id: i32,
    pub vehicle_name: String,
    pub job_title: String,
    pub need: job_need::Model,
    pub chosen: Option<need_option::Model>,
    pub rejected: Vec<need_option::Model>,
    /// Parts bought for the need that went back.
    pub returned_parts: Vec<part::Model>,
}

/// The checklist of one appointment.
///
/// # Errors
/// `NotFound` for an appointment that is not the vehicle's.
pub async fn for_appointment(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    appointment_id: i32,
) -> DomainResult<Checklist> {
    let summary = appointment::summary(db, vehicle_id, appointment_id).await?;
    let jobs = work_item::Entity::find()
        .filter(work_item::Column::AppointmentId.eq(appointment_id))
        .order_by_asc(work_item::Column::Id)
        .all(db)
        .await?;
    let todos = todo_svc::list(db, vehicle_id, Some(Some(appointment_id)), true).await?;
    Ok(assemble(Some(summary), job_views(db, jobs).await?, todos))
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
    let jobs = work_item::Entity::find()
        .filter(work_item::Column::VehicleId.eq(vehicle_id))
        .filter(work_item::Column::AppointmentId.is_null())
        .filter(work_item::Column::Status.eq("planned"))
        .order_by_asc(work_item::Column::Id)
        .all(db)
        .await?;
    let mut views = job_views(db, jobs).await?;
    views.retain(|j| !j.needs.is_empty());
    Ok(assemble(None, views, todos))
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
    let mut returned: HashMap<i32, Vec<part::Model>> = HashMap::new();
    for p in part::Entity::find()
        .filter(part::Column::ReturnedFromNeedId.is_in(need_ids))
        .order_by_asc(part::Column::Id)
        .all(db)
        .await?
    {
        if let Some(need_id) = p.returned_from_need_id {
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
        let rejected = all
            .into_iter()
            .filter(|o| o.rejected_reason.is_some() || o.rejected_on.is_some())
            .collect();
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

// Loads the needs, options and parts of every job in three batched queries
// and returns the views in the order of `jobs`.
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
    let parts: HashMap<i32, part::Model> = part::Entity::find()
        .filter(part::Column::Id.is_in(needs.iter().filter_map(|n| n.part_id)))
        .all(db)
        .await?
        .into_iter()
        .map(|p| (p.id, p))
        .collect();
    let mut by_job: HashMap<i32, Vec<NeedView>> = HashMap::new();
    for need in needs {
        let linked = need.part_id.and_then(|id| parts.get(&id).cloned());
        let job_id = need.work_item_id;
        let own_options = options.remove(&need.id).unwrap_or_default();
        by_job
            .entry(job_id)
            .or_default()
            .push(need_view(need, own_options, linked));
    }
    Ok(jobs
        .into_iter()
        .map(|job| JobView {
            needs: by_job.remove(&job.id).unwrap_or_default(),
            job,
        })
        .collect())
}

fn need_view(
    need: job_need::Model,
    options: Vec<need_option::Model>,
    part: Option<part::Model>,
) -> NeedView {
    let (chosen, other_options): (Vec<_>, Vec<_>) = options
        .into_iter()
        .partition(|o| Some(o.id) == need.chosen_option_id);
    let chosen = chosen.into_iter().next();
    let mut notes = Vec::new();
    if chosen.as_ref().is_some_and(|c| c.fitment == "unconfirmed") {
        notes.push("Fitment of the chosen option is not confirmed.".to_string());
    }
    if let (Some(part), Some(chosen)) = (&part, &chosen)
        && let (Some(have), Some(want)) = (&part.part_number, &chosen.part_number)
        && !have.trim().eq_ignore_ascii_case(want.trim())
    {
        notes.push(format!(
            "The linked part's number ({have}) differs from the chosen option's ({want})."
        ));
    }
    if part.is_some() && chosen.is_none() {
        notes.push("A part is linked and no decision is recorded.".to_string());
    }
    NeedView {
        stage: stage(&need, part.as_ref()),
        purchase_summary: part.as_ref().and_then(purchase_summary),
        need,
        chosen,
        other_options,
        part,
        notes,
    }
}

fn purchase_summary(part: &part::Model) -> Option<String> {
    let pieces: Vec<String> = [
        part.seller.clone(),
        part.purchase_date.clone(),
        part.cost_cents.map(format_cents),
        part.order_ref.as_ref().map(|r| format!("order {r}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!pieces.is_empty()).then(|| pieces.join(" \u{b7} "))
}

fn assemble(
    appointment: Option<AppointmentSummary>,
    jobs: Vec<JobView>,
    todos: Vec<todo::Model>,
) -> Checklist {
    let next_steps = next_steps(&jobs, &todos);
    let warnings = jobs
        .iter()
        .filter(|j| j.job.status != "dropped")
        .flat_map(|j| &j.needs)
        .flat_map(|n| {
            n.notes
                .iter()
                .map(|note| format!("{}: {note}", n.need.name))
        })
        .collect();
    Checklist {
        appointment,
        next_steps,
        jobs,
        todos,
        warnings,
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
        let job_done = match job.job.status.as_str() {
            "dropped" => continue,
            status => status == "done",
        };
        for view in &job.needs {
            let row = StepRow {
                need_id: view.need.id,
                job_id: job.job.id,
                job_title: job.job.title.clone(),
                name: view.need.name.clone(),
                summary: step_summary(view, job_done),
                stage: view.stage,
            };
            let group = match view.stage {
                _ if job_done => &mut steps.done,
                Stage::Undecided => &mut steps.decide,
                Stage::ToOrder => &mut steps.order,
                Stage::Ordered => &mut steps.arriving,
                Stage::ShopSupplies | Stage::Received | Stage::Installed => &mut steps.done,
            };
            group.push(row);
        }
    }
    steps
}

// Names an option in a sentence: by manufacturer and part number when it has
// a part number, otherwise by its description.
fn option_label(option: &need_option::Model) -> String {
    if option.part_number.is_none() {
        return option.description.clone();
    }
    [
        option.manufacturer.as_deref(),
        option.part_number.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
}

fn step_summary(view: &NeedView, job_done: bool) -> String {
    let name = &view.need.name;
    match view.stage {
        Stage::Undecided | Stage::ToOrder | Stage::Ordered if job_done => {
            format!("{name}: the job is done")
        }
        Stage::Undecided if view.chosen.is_some() => {
            format!("{name}: who supplies it is not decided")
        }
        Stage::Undecided => format!("{name}: no part chosen"),
        Stage::ToOrder => {
            let what = view.chosen.as_ref().map_or_else(String::new, |c| {
                let from = c
                    .seller
                    .as_ref()
                    .map_or_else(String::new, |s| format!(" from {s}"));
                format!(" {}{from}", option_label(c))
            });
            format!("{name}: order{what}")
        }
        Stage::Ordered => {
            let seller = view.part.as_ref().and_then(|p| p.seller.as_ref());
            let date = view.part.as_ref().and_then(|p| p.purchase_date.as_ref());
            format!(
                "{name}: ordered{}{}",
                seller.map_or_else(String::new, |s| format!(" from {s}")),
                date.map_or_else(String::new, |d| format!(" on {d}"))
            )
        }
        Stage::ShopSupplies => format!("{name}: the shop supplies it"),
        Stage::Received => match view.part.as_ref().and_then(|p| p.received_on.as_ref()) {
            Some(date) => format!("{name}: received on {date}"),
            None => format!("{name}: received"),
        },
        Stage::Installed => format!("{name}: installed"),
    }
}

/// One vehicle's open work for the home page.
#[derive(Debug, Serialize)]
pub struct VehicleHome {
    pub vehicle: vehicle::Model,
    /// The open appointments, soonest planned date first, unbooked ones last.
    pub appointments: Vec<HomeAppointment>,
    /// The planned jobs that are on no appointment and have needs.
    pub unscheduled_jobs: Vec<HomeJob>,
    /// The open to-dos that are on no appointment.
    pub todos: Vec<todo::Model>,
}

/// An open appointment with its counts written out as statements.
#[derive(Debug, Serialize)]
pub struct HomeAppointment {
    pub id: i32,
    pub purpose: String,
    pub shop_name: Option<String>,
    /// The planned date, or `not booked`.
    pub when: String,
    /// For example `3 needs open · 2 to-dos open`.
    pub counts: String,
}

/// An unscheduled job with its open needs written out as a statement.
#[derive(Debug, Serialize)]
pub struct HomeJob {
    pub id: i32,
    pub title: String,
    /// For example `2 needs open`.
    pub counts: String,
}

/// `1 need`, `2 needs`.
fn count_of(n: usize, singular: &str, plural: &str) -> String {
    format!("{n} {}", if n == 1 { singular } else { plural })
}

/// Whether a need still asks something of the owner before the visit.
fn is_open(stage: Stage) -> bool {
    matches!(stage, Stage::Undecided | Stage::ToOrder | Stage::Ordered)
}

/// What the home page shows for every vehicle that is not archived: its open
/// appointments, its unscheduled jobs and its to-dos that are on no
/// appointment. An appointment whose jobs are all done stays listed while it
/// has an open to-do.
///
/// # Errors
/// `Db` when a query fails.
pub async fn home(db: &impl ConnectionTrait) -> DomainResult<Vec<VehicleHome>> {
    let mut out = Vec::new();
    for vehicle in super::vehicle::list(db)
        .await?
        .into_iter()
        .filter(|v| v.archived_at.is_none())
    {
        let appointments = appointment::list(db, vehicle.id, false)
            .await?
            .into_iter()
            .map(|s| HomeAppointment {
                id: s.appointment.id,
                purpose: s.appointment.purpose,
                shop_name: s.appointment.shop_name,
                when: s
                    .appointment
                    .planned_date
                    .unwrap_or_else(|| "not booked".to_owned()),
                counts: format!(
                    "{} open \u{b7} {} open",
                    count_of(s.needs_open, "need", "needs"),
                    count_of(s.todos_open, "to-do", "to-dos")
                ),
            })
            .collect();
        let unscheduled = for_unscheduled(db, vehicle.id).await?;
        let unscheduled_jobs = unscheduled
            .jobs
            .iter()
            .map(|j| HomeJob {
                id: j.job.id,
                title: j.job.title.clone(),
                counts: format!(
                    "{} open",
                    count_of(
                        j.needs.iter().filter(|n| is_open(n.stage)).count(),
                        "need",
                        "needs"
                    )
                ),
            })
            .collect();
        out.push(VehicleHome {
            vehicle,
            appointments,
            unscheduled_jobs,
            todos: unscheduled.next_steps.todos,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inputs::{
            appointment::NewAppointment,
            need::{NewNeed, NewOption, OrderDetails, ReturnInput, UpdateNeed, UpdateOption},
            part::NewPart,
            todo::NewTodo,
            work_item::{NewWorkItem, UpdateWorkItem},
        },
        services::{need, part as part_svc, work_item as job_svc},
        test_support::{VehicleFixture, test_db},
    };

    async fn new_job(
        db: &DatabaseConnection,
        vehicle_id: i32,
        title: &str,
        appointment_id: Option<i32>,
    ) -> i32 {
        job_svc::create(
            db,
            vehicle_id,
            NewWorkItem {
                title: title.into(),
                appointment_id,
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    async fn set_job_status(db: &DatabaseConnection, vehicle_id: i32, job_id: i32, status: &str) {
        job_svc::update(
            db,
            vehicle_id,
            job_id,
            UpdateWorkItem {
                status: Some(status.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    async fn new_need(db: &DatabaseConnection, vehicle_id: i32, job_id: i32, name: &str) -> i32 {
        need::add(
            db,
            vehicle_id,
            job_id,
            NewNeed {
                name: name.into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    async fn new_option(
        db: &DatabaseConnection,
        vehicle_id: i32,
        need_id: i32,
        description: &str,
        part_number: Option<&str>,
    ) -> i32 {
        need::add_option(
            db,
            vehicle_id,
            need_id,
            NewOption {
                description: description.into(),
                part_number: part_number.map(Into::into),
                seller: Some("FCP Euro".into()),
                price_cents: Some(8_499),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    // A need the owner will buy: supplied by the owner with an option chosen.
    async fn to_order_need(
        db: &DatabaseConnection,
        vehicle_id: i32,
        job_id: i32,
        name: &str,
        description: &str,
    ) -> i32 {
        let id = new_need(db, vehicle_id, job_id, name).await;
        let option_id = new_option(db, vehicle_id, id, description, None).await;
        set_supplier(db, vehicle_id, id, "self").await;
        need::decide(db, vehicle_id, id, Some(option_id), Some("fits".into()))
            .await
            .unwrap();
        id
    }

    // A to-order need whose only option is `option`.
    async fn to_order_need_with(
        db: &DatabaseConnection,
        vehicle_id: i32,
        job_id: i32,
        name: &str,
        option: NewOption,
    ) -> i32 {
        let id = new_need(db, vehicle_id, job_id, name).await;
        let option_id = need::add_option(db, vehicle_id, id, option)
            .await
            .unwrap()
            .id;
        set_supplier(db, vehicle_id, id, "self").await;
        need::decide(db, vehicle_id, id, Some(option_id), None)
            .await
            .unwrap();
        id
    }

    async fn set_supplier(db: &DatabaseConnection, vehicle_id: i32, need_id: i32, who: &str) {
        need::update(
            db,
            vehicle_id,
            need_id,
            UpdateNeed {
                supplied_by: Some(Some(who.into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    async fn order(db: &DatabaseConnection, vehicle_id: i32, need_id: i32, on: &str) {
        need::mark_ordered(
            db,
            vehicle_id,
            need_id,
            OrderDetails {
                purchase_date: Some(on.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    async fn appointment_id(db: &DatabaseConnection, vehicle_id: i32) -> i32 {
        appointment::create(
            db,
            vehicle_id,
            NewAppointment {
                purpose: "60k service".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    fn rows(group: &[StepRow]) -> Vec<(&str, &str)> {
        group
            .iter()
            .map(|r| (r.name.as_str(), r.summary.as_str()))
            .collect()
    }

    #[tokio::test]
    async fn next_steps_group_by_stage_in_order() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_id(&db, v).await;
        let job = new_job(&db, v, "60k service", Some(a)).await;
        new_need(&db, v, job, "Rear pads").await;
        to_order_need_with(
            &db,
            v,
            job,
            "Rear rotors",
            NewOption {
                description: "Rear rotor, solid disc".into(),
                manufacturer: Some("OE".into()),
                part_number: Some("5Q0615601E".into()),
                seller: Some("FCP Euro".into()),
                ..Default::default()
            },
        )
        .await;
        let front_pads = to_order_need(&db, v, job, "Front pads", "Textar pads").await;
        order(&db, v, front_pads, "2026-11-14").await;
        let front_rotors = to_order_need(&db, v, job, "Front rotors", "Zimmermann rotors").await;
        order(&db, v, front_rotors, "2026-11-06").await;
        need::mark_received(&db, v, front_rotors, Some("2026-11-09".into()))
            .await
            .unwrap();
        crate::services::todo::add(
            &db,
            v,
            NewTodo {
                title: "Bring wheel locks".into(),
                appointment_id: Some(a),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let list = for_appointment(&db, v, a).await.unwrap();
        assert_eq!(list.appointment.as_ref().unwrap().appointment.id, a);
        let steps = &list.next_steps;
        assert_eq!(
            rows(&steps.decide),
            [("Rear pads", "Rear pads: no part chosen")]
        );
        assert_eq!(
            rows(&steps.order),
            [(
                "Rear rotors",
                "Rear rotors: order OE 5Q0615601E from FCP Euro"
            )]
        );
        assert_eq!(
            rows(&steps.arriving),
            [(
                "Front pads",
                "Front pads: ordered from FCP Euro on 2026-11-14"
            )]
        );
        assert_eq!(steps.done.len(), 1);
        assert_eq!(steps.done[0].name, "Front rotors");
        assert_eq!(steps.done[0].stage, Stage::Received);
        assert_eq!(steps.todos.len(), 1);
        assert_eq!(steps.decide[0].job_title, "60k service");
        assert_eq!(list.jobs.len(), 1);
        assert_eq!(list.jobs[0].needs.len(), 4);
    }

    #[tokio::test]
    async fn undecided_summary_says_what_is_missing() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_id(&db, v).await;
        let job = new_job(&db, v, "Brakes", Some(a)).await;
        let n = new_need(&db, v, job, "Rear pads").await;
        let option_id = new_option(&db, v, n, "Textar pads", None).await;
        need::decide(&db, v, n, Some(option_id), None)
            .await
            .unwrap();
        let list = for_appointment(&db, v, a).await.unwrap();
        assert_eq!(
            rows(&list.next_steps.decide),
            [("Rear pads", "Rear pads: who supplies it is not decided")]
        );
    }

    #[tokio::test]
    async fn shop_supplied_need_is_done_not_open() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_id(&db, v).await;
        let job = new_job(&db, v, "Brakes", Some(a)).await;
        let n = new_need(&db, v, job, "Rear pads").await;
        set_supplier(&db, v, n, "shop").await;
        let list = for_appointment(&db, v, a).await.unwrap();
        let steps = &list.next_steps;
        assert!(steps.decide.is_empty() && steps.order.is_empty() && steps.arriving.is_empty());
        assert_eq!(steps.done.len(), 1);
        assert_eq!(steps.done[0].stage, Stage::ShopSupplies);
        assert_eq!(list.appointment.unwrap().needs_open, 0);
    }

    #[tokio::test]
    async fn dropped_job_needs_appear_under_the_job_only() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_id(&db, v).await;
        let job = new_job(&db, v, "Brakes", Some(a)).await;
        new_need(&db, v, job, "Rear pads").await;
        set_job_status(&db, v, job, "dropped").await;
        let list = for_appointment(&db, v, a).await.unwrap();
        let steps = &list.next_steps;
        assert!(
            steps.decide.is_empty()
                && steps.order.is_empty()
                && steps.arriving.is_empty()
                && steps.done.is_empty()
        );
        assert_eq!(list.jobs[0].needs.len(), 1);
        assert_eq!(list.appointment.unwrap().needs_open, 0);
    }

    #[tokio::test]
    async fn done_job_needs_go_to_done_even_when_undecided() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_id(&db, v).await;
        let job = new_job(&db, v, "Brakes", Some(a)).await;
        new_need(&db, v, job, "Rear pads").await;
        to_order_need(&db, v, job, "Rear rotors", "OE rotors").await;
        set_job_status(&db, v, job, "done").await;
        let list = for_appointment(&db, v, a).await.unwrap();
        let steps = &list.next_steps;
        assert!(steps.decide.is_empty() && steps.order.is_empty() && steps.arriving.is_empty());
        assert_eq!(steps.done.len(), 2);
        assert_eq!(steps.done[0].stage, Stage::Undecided);
        assert_eq!(list.appointment.unwrap().needs_open, 0);
    }

    #[tokio::test]
    async fn order_summary_names_the_option_by_part_number_or_description() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_id(&db, v).await;
        let job = new_job(&db, v, "Brakes", Some(a)).await;
        let option =
            |description: &str, maker: Option<&str>, number: Option<&str>, seller| NewOption {
                description: description.into(),
                manufacturer: maker.map(Into::into),
                part_number: number.map(Into::into),
                seller: seller,
                ..Default::default()
            };
        let fcp = || Some("FCP Euro".to_string());
        to_order_need_with(
            &db,
            v,
            job,
            "Rear rotors",
            option("Rear rotor", Some("Zimmermann"), Some("600.3241.20"), fcp()),
        )
        .await;
        to_order_need_with(
            &db,
            v,
            job,
            "Wiper blades",
            option("Aero blades", None, Some("A123"), fcp()),
        )
        .await;
        to_order_need_with(
            &db,
            v,
            job,
            "Cabin filter",
            option("Mann cabin filter", Some("Mann"), None, fcp()),
        )
        .await;
        to_order_need_with(
            &db,
            v,
            job,
            "Oil",
            option("5W-40 oil", Some("Liqui Moly"), Some("2332"), None),
        )
        .await;
        let list = for_appointment(&db, v, a).await.unwrap();
        assert_eq!(
            rows(&list.next_steps.order),
            [
                (
                    "Rear rotors",
                    "Rear rotors: order Zimmermann 600.3241.20 from FCP Euro"
                ),
                ("Wiper blades", "Wiper blades: order A123 from FCP Euro"),
                (
                    "Cabin filter",
                    "Cabin filter: order Mann cabin filter from FCP Euro"
                ),
                ("Oil", "Oil: order Liqui Moly 2332"),
            ]
        );
    }

    #[tokio::test]
    async fn a_whitespace_manufacturer_leaves_no_leading_space_in_the_label() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_id(&db, v).await;
        let job = new_job(&db, v, "Brakes", Some(a)).await;
        to_order_need_with(
            &db,
            v,
            job,
            "Wiper blades",
            NewOption {
                description: "Aero blades".into(),
                manufacturer: Some("   ".into()),
                part_number: Some("A123".into()),
                ..Default::default()
            },
        )
        .await;
        let list = for_appointment(&db, v, a).await.unwrap();
        assert_eq!(list.next_steps.order[0].summary, "Wiper blades: order A123");
    }

    #[tokio::test]
    async fn done_group_sentences_are_exact() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_id(&db, v).await;
        let job = new_job(&db, v, "Brakes", Some(a)).await;
        let shop = new_need(&db, v, job, "Rear pads").await;
        set_supplier(&db, v, shop, "shop").await;
        let received = to_order_need(&db, v, job, "Front pads", "Textar pads").await;
        order(&db, v, received, "2026-11-06").await;
        need::mark_received(&db, v, received, Some("2026-11-09".into()))
            .await
            .unwrap();
        let installed = to_order_need(&db, v, job, "Front rotors", "Zimmermann rotors").await;
        order(&db, v, installed, "2026-11-06").await;
        let linked = need::require(&db, v, installed).await.unwrap();
        let mut part: part::ActiveModel = part::Entity::find_by_id(linked.part_id.unwrap())
            .one(&db)
            .await
            .unwrap()
            .unwrap()
            .into();
        part.status = Set("installed".into());
        part.update(&db).await.unwrap();
        let list = for_appointment(&db, v, a).await.unwrap();
        assert_eq!(
            rows(&list.next_steps.done),
            [
                ("Rear pads", "Rear pads: the shop supplies it"),
                ("Front pads", "Front pads: received on 2026-11-09"),
                ("Front rotors", "Front rotors: installed"),
            ]
        );

        let open = new_need(&db, v, job, "Wipers").await;
        let _ = open;
        set_job_status(&db, v, job, "done").await;
        let list = for_appointment(&db, v, a).await.unwrap();
        let wipers = list.next_steps.done.last().unwrap();
        assert_eq!(wipers.summary, "Wipers: the job is done");
    }

    const UNCONFIRMED: &str = "Fitment of the chosen option is not confirmed.";
    const NO_DECISION: &str = "A part is linked and no decision is recorded.";

    #[tokio::test]
    async fn unconfirmed_fitment_and_mismatched_part_number_warn() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_id(&db, v).await;
        let job = new_job(&db, v, "Brakes", Some(a)).await;
        let n = new_need(&db, v, job, "Rear rotors").await;
        let option_id = new_option(&db, v, n, "OE rotors", Some("5Q0615601E")).await;
        set_supplier(&db, v, n, "self").await;
        need::decide(&db, v, n, Some(option_id), None)
            .await
            .unwrap();
        let bought = part_svc::create(
            &db,
            v,
            NewPart {
                name: "Rotors".into(),
                part_number: Some("5Q0615601F".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        need::link_part(&db, v, n, bought.id, None).await.unwrap();

        let list = for_appointment(&db, v, a).await.unwrap();
        let mismatch =
            "The linked part's number (5Q0615601F) differs from the chosen option's (5Q0615601E).";
        assert_eq!(list.jobs[0].needs[0].notes, [UNCONFIRMED, mismatch]);
        assert_eq!(
            list.warnings,
            [
                format!("Rear rotors: {UNCONFIRMED}"),
                format!("Rear rotors: {mismatch}")
            ]
        );
    }

    #[tokio::test]
    async fn confirmed_fitment_and_matching_numbers_do_not_warn() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_id(&db, v).await;
        let job = new_job(&db, v, "Brakes", Some(a)).await;
        let n = new_need(&db, v, job, "Rear rotors").await;
        let option_id = new_option(&db, v, n, "OE rotors", Some("5q0615601e")).await;
        need::update_option(
            &db,
            v,
            option_id,
            UpdateOption {
                fitment: Some("confirmed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        set_supplier(&db, v, n, "self").await;
        need::decide(&db, v, n, Some(option_id), None)
            .await
            .unwrap();
        let bought = part_svc::create(
            &db,
            v,
            NewPart {
                name: "Rotors".into(),
                part_number: Some("5Q0615601E".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        need::link_part(&db, v, n, bought.id, None).await.unwrap();
        let list = for_appointment(&db, v, a).await.unwrap();
        assert!(list.warnings.is_empty(), "{:?}", list.warnings);
    }

    #[tokio::test]
    async fn linked_part_without_decision_is_noted() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_id(&db, v).await;
        let job = new_job(&db, v, "Brakes", Some(a)).await;
        let n = new_need(&db, v, job, "Rear pads").await;
        let bought = part_svc::create(
            &db,
            v,
            NewPart {
                name: "Pads".into(),
                seller: Some("FCP Euro".into()),
                purchase_date: Some("2026-11-06".into()),
                cost_cents: Some(8_499),
                order_ref: Some("12345".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        need::link_part(&db, v, n, bought.id, None).await.unwrap();
        let view = for_job(&db, v, job).await.unwrap();
        let need_view = &view.needs[0];
        assert_eq!(need_view.notes, [NO_DECISION]);
        assert_eq!(need_view.stage, Stage::Ordered);
        assert_eq!(
            need_view.purchase_summary.as_deref(),
            Some("FCP Euro · 2026-11-06 · $84.99 · order 12345")
        );
    }

    #[tokio::test]
    async fn purchase_summary_uses_only_the_parts_present() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let job = new_job(&db, v, "Brakes", None).await;
        let n = new_need(&db, v, job, "Rear pads").await;
        let bought = part_svc::create(
            &db,
            v,
            NewPart {
                name: "Pads".into(),
                cost_cents: Some(5),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        need::link_part(&db, v, n, bought.id, None).await.unwrap();
        let view = for_job(&db, v, job).await.unwrap();
        assert_eq!(view.needs[0].purchase_summary.as_deref(), Some("$0.05"));
    }

    #[tokio::test]
    async fn unscheduled_lists_only_planned_jobs_with_needs() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_id(&db, v).await;
        let with_needs = new_job(&db, v, "Wipers", None).await;
        new_need(&db, v, with_needs, "Blades").await;
        new_job(&db, v, "Wash", None).await;
        let scheduled = new_job(&db, v, "Brakes", Some(a)).await;
        new_need(&db, v, scheduled, "Pads").await;
        let done = new_job(&db, v, "Oil", None).await;
        new_need(&db, v, done, "Filter").await;
        set_job_status(&db, v, done, "done").await;
        crate::services::todo::add(
            &db,
            v,
            NewTodo {
                title: "Buy wiper fluid".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let list = for_unscheduled(&db, v).await.unwrap();
        assert!(list.appointment.is_none());
        let titles: Vec<&str> = list.jobs.iter().map(|j| j.job.title.as_str()).collect();
        assert_eq!(titles, ["Wipers"]);
        assert_eq!(
            rows(&list.next_steps.decide),
            [("Blades", "Blades: no part chosen")]
        );
        assert_eq!(list.todos.len(), 1);
        assert_eq!(list.next_steps.todos.len(), 1);
    }

    #[tokio::test]
    async fn wrong_vehicle_ids_read_as_missing() {
        let db = test_db().await;
        let v1 = VehicleFixture::new().insert_id(&db).await;
        let v2 = VehicleFixture::new().name("Other").insert_id(&db).await;
        let a = appointment_id(&db, v1).await;
        let job = new_job(&db, v1, "Brakes", Some(a)).await;
        let missing_appointment = for_appointment(&db, v1, 99_999).await.unwrap_err();
        let wrong_appointment = for_appointment(&db, v2, a).await.unwrap_err();
        assert!(matches!(wrong_appointment, DomainError::NotFound(_)));
        assert_eq!(
            wrong_appointment.to_string().replace(&a.to_string(), "N"),
            missing_appointment.to_string().replace("99999", "N")
        );
        let missing_job = for_job(&db, v1, 99_999).await.unwrap_err();
        let wrong_job = for_job(&db, v2, job).await.unwrap_err();
        assert!(matches!(wrong_job, DomainError::NotFound(_)));
        assert_eq!(
            wrong_job.to_string().replace(&job.to_string(), "N"),
            missing_job.to_string().replace("99999", "N")
        );
    }

    async fn set_decided_on(db: &DatabaseConnection, need_id: i32, on: &str) {
        let found = job_need::Entity::find_by_id(need_id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        let mut active: job_need::ActiveModel = found.into();
        active.decided_on = Set(Some(on.into()));
        active.update(db).await.unwrap();
    }

    #[tokio::test]
    async fn past_decisions_finds_by_need_name_and_part_number_across_vehicles() {
        let db = test_db().await;
        let gti = VehicleFixture::new().name("GTI").insert_id(&db).await;
        let alltrack = VehicleFixture::new().name("Alltrack").insert_id(&db).await;
        let gti_job = new_job(&db, gti, "Brakes", None).await;
        let older = to_order_need(&db, gti, gti_job, "Brake pads", "Textar pads").await;
        set_decided_on(&db, older, "2026-10-01").await;
        let alltrack_job = new_job(&db, alltrack, "Rotors", None).await;
        let newer = new_need(&db, alltrack, alltrack_job, "Rear rotors").await;
        let option_id = new_option(&db, alltrack, newer, "OE", Some("5Q0615601E")).await;
        need::decide(
            &db,
            alltrack,
            newer,
            Some(option_id),
            Some("OE fits".into()),
        )
        .await
        .unwrap();
        set_decided_on(&db, newer, "2026-11-01").await;

        let by_name = past_decisions(&db, "BRAKE").await.unwrap();
        assert_eq!(by_name.len(), 1);
        assert_eq!(by_name[0].vehicle_name, "GTI");
        assert_eq!(by_name[0].job_title, "Brakes");
        assert_eq!(
            by_name[0].chosen.as_ref().unwrap().description,
            "Textar pads"
        );

        let by_number = past_decisions(&db, "5q0615601e").await.unwrap();
        assert_eq!(by_number.len(), 1);
        assert_eq!(by_number[0].vehicle_name, "Alltrack");
        assert_eq!(
            by_number[0].need.decision_reason.as_deref(),
            Some("OE fits")
        );

        let both = past_decisions(&db, "r").await.unwrap();
        let ids: Vec<i32> = both.iter().map(|d| d.need.id).collect();
        assert_eq!(ids, [newer, older]);

        // Wildcard characters in the query match themselves only.
        assert!(past_decisions(&db, "%").await.unwrap().is_empty());
        assert!(past_decisions(&db, "_").await.unwrap().is_empty());
        assert!(
            past_decisions(&db, "no such part")
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn past_decisions_includes_rejected_options_and_returned_parts() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let job = new_job(&db, v, "Brakes", None).await;
        let n = new_need(&db, v, job, "Rear pads").await;
        let wrong = new_option(&db, v, n, "Cheap pads", None).await;
        let right = new_option(&db, v, n, "Textar pads", None).await;
        need::update_option(
            &db,
            v,
            wrong,
            UpdateOption {
                rejected_reason: Some(Some("glazes quickly".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        set_supplier(&db, v, n, "self").await;
        need::decide(&db, v, n, Some(right), Some("OE-grade".into()))
            .await
            .unwrap();
        order(&db, v, n, "2026-11-06").await;
        need::return_part(
            &db,
            v,
            n,
            ReturnInput {
                reason: "wrong box".into(),
                option_was_wrong: false,
            },
        )
        .await
        .unwrap();

        let found = past_decisions(&db, "rear pads").await.unwrap();
        assert_eq!(found.len(), 1);
        let d = &found[0];
        assert_eq!(d.rejected.len(), 1);
        assert_eq!(d.rejected[0].id, wrong);
        assert_eq!(
            d.rejected[0].rejected_reason.as_deref(),
            Some("glazes quickly")
        );
        assert_eq!(d.returned_parts.len(), 1);
        assert_eq!(
            d.returned_parts[0].return_reason.as_deref(),
            Some("wrong box")
        );
        assert_eq!(d.chosen.as_ref().unwrap().id, right);
    }

    #[tokio::test]
    async fn blank_query_is_invalid() {
        let db = test_db().await;
        let err = past_decisions(&db, "   ").await.unwrap_err();
        assert!(matches!(err, DomainError::Invalid { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn home_view_groups_by_vehicle() {
        let db = test_db().await;
        let golf = VehicleFixture::new().name("Golf").insert_id(&db).await;
        let alltrack = VehicleFixture::new().name("Alltrack").insert_id(&db).await;
        let old = VehicleFixture::new().name("Old").insert_id(&db).await;
        crate::services::vehicle::archive(&db, old).await.unwrap();

        let booked = appointment::create(
            &db,
            golf,
            NewAppointment {
                purpose: "60k service".into(),
                planned_date: Some("2026-12-01".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        let unbooked = appointment::create(
            &db,
            golf,
            NewAppointment {
                purpose: "Tire swap".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        let job = new_job(&db, golf, "Brakes", Some(booked)).await;
        new_need(&db, golf, job, "Rear pads").await;
        new_need(&db, golf, job, "Rear rotors").await;
        let todo_input = |title: &str, appointment_id| NewTodo {
            title: title.into(),
            appointment_id,
            ..Default::default()
        };
        crate::services::todo::add(&db, golf, todo_input("Bring wheel locks", Some(booked)))
            .await
            .unwrap();
        crate::services::todo::add(&db, golf, todo_input("Wash the car", None))
            .await
            .unwrap();
        let done = crate::services::todo::add(&db, golf, todo_input("Buy wipers", None))
            .await
            .unwrap();
        crate::services::todo::complete(&db, golf, done.id, None)
            .await
            .unwrap();
        let loose = new_job(&db, golf, "Wiper blades", None).await;
        new_need(&db, golf, loose, "Blades").await;
        new_job(&db, golf, "Needless job", None).await;
        crate::services::todo::add(&db, alltrack, todo_input("Check tire pressure", None))
            .await
            .unwrap();

        let home = home(&db).await.unwrap();

        let names: Vec<&str> = home.iter().map(|h| h.vehicle.name.as_str()).collect();
        assert_eq!(
            names,
            ["Golf", "Alltrack"],
            "archived vehicles are left out"
        );
        let g = &home[0];
        let shown: Vec<(i32, &str, &str, &str)> = g
            .appointments
            .iter()
            .map(|a| (a.id, a.purpose.as_str(), a.when.as_str(), a.counts.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                (
                    booked,
                    "60k service",
                    "2026-12-01",
                    "2 needs open \u{b7} 1 to-do open"
                ),
                (
                    unbooked,
                    "Tire swap",
                    "not booked",
                    "0 needs open \u{b7} 0 to-dos open"
                ),
            ]
        );
        let jobs: Vec<(&str, &str)> = g
            .unscheduled_jobs
            .iter()
            .map(|j| (j.title.as_str(), j.counts.as_str()))
            .collect();
        assert_eq!(jobs, [("Wiper blades", "1 need open")]);
        let todos: Vec<&str> = g.todos.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            todos,
            ["Wash the car"],
            "done and appointment to-dos are left out"
        );
        let a = &home[1];
        assert!(a.appointments.is_empty() && a.unscheduled_jobs.is_empty());
        assert_eq!(a.todos.len(), 1);
    }
}
