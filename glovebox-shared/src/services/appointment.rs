//! Appointments: a visit that groups jobs and to-dos. Every function scopes
//! its ids to a vehicle, so another vehicle's id reads as a missing one.

use std::collections::{HashMap, HashSet};

use sea_orm::{sea_query::Expr, *};
use serde::Serialize;

use crate::{
    entities::{appointment, job_need, part, todo, work_item},
    error::{DomainError, DomainResult},
    inputs::appointment::{NewAppointment, UpdateAppointment},
    services::need::{Stage, stage},
};

/// An appointment with the counts the owner reads at a glance. `needs_open`
/// counts needs on its jobs (dropped jobs excluded) that are undecided, waiting to be ordered or
/// ordered but not yet received; `todos_open` counts its unfinished to-dos.
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

fn validate_purpose(purpose: &str) -> DomainResult<()> {
    if purpose.trim().is_empty() {
        return Err(DomainError::invalid("purpose", "must not be blank"));
    }
    Ok(())
}

// The shop directory is global, so an id is checked for existence only. A
// missing shop would otherwise surface as an opaque foreign-key failure.
async fn validate_shop(db: &impl ConnectionTrait, shop_id: Option<i32>) -> DomainResult<()> {
    if let Some(id) = shop_id {
        super::shop::get(db, id).await?;
    }
    Ok(())
}

/// The appointment's state from its dates and its jobs' statuses: `cancelled`
/// when cancelled, `done` when closed by hand or when it has jobs and every
/// one is done or dropped, otherwise `planning`.
#[must_use]
pub fn status(a: &appointment::Model, job_statuses: &[&str]) -> &'static str {
    if a.cancelled_on.is_some() {
        "cancelled"
    } else if a.closed_on.is_some()
        || (!job_statuses.is_empty()
            && job_statuses
                .iter()
                .all(|s| matches!(*s, "done" | "dropped")))
    {
        "done"
    } else {
        "planning"
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
    validate_purpose(&input.purpose)?;
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
        validate_purpose(purpose)?;
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

/// [`require`] for an appointment that is about to receive a job: a cancelled
/// or hand-closed appointment is refused.
pub(crate) async fn require_assignable(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<appointment::Model> {
    let found = require(db, vehicle_id, id).await?;
    if found.cancelled_on.is_some() || found.closed_on.is_some() {
        return Err(DomainError::BadRequest(format!(
            "Appointment {id} is {}; reopen it before adding jobs",
            if found.cancelled_on.is_some() {
                "cancelled"
            } else {
                "closed"
            }
        )));
    }
    Ok(found)
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
    let mut out = summaries(db, vehicle_id, None).await?;
    if !include_closed {
        // A derived `done` appointment with a to-do left still has work to
        // show; a hand-closed or cancelled one has been put away on purpose.
        out.retain(|s| {
            s.status == "planning"
                || (s.status == "done" && s.appointment.closed_on.is_none() && s.todos_open > 0)
        });
    }
    Ok(out)
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
    summaries(db, vehicle_id, Some(id))
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| DomainError::NotFound(format!("Appointment {id} not found")))
}

// Builds the summaries for every appointment of the vehicle, or for the one
// named by `only`, in list order, with every count loaded in batches.
async fn summaries(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    only: Option<i32>,
) -> DomainResult<Vec<AppointmentSummary>> {
    let mut query =
        appointment::Entity::find().filter(appointment::Column::VehicleId.eq(vehicle_id));
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
    let dropped: HashSet<i32> = jobs
        .iter()
        .filter(|j| j.status == "dropped")
        .map(|j| j.id)
        .collect();
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
        if matches!(current, Stage::Undecided | Stage::ToOrder | Stage::Ordered)
            && !dropped.contains(&need.work_item_id)
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
            AppointmentSummary {
                status: status(&a, job_statuses),
                booked: a.planned_date.is_some(),
                jobs: job_statuses.len(),
                needs_open: needs_open.get(&a.id).copied().unwrap_or(0),
                todos_open: todos_open.get(&a.id).copied().unwrap_or(0),
                appointment: a,
            }
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

/// Closes an appointment by hand (`closed_on`), or cancels it (`cancelled_on`)
/// when `cancel` is set, and takes every job still `planned` off it. Done and
/// dropped jobs stay, and no need is touched.
///
/// # Errors
/// `NotFound` for an appointment that is not the vehicle's; `BadRequest` when
/// it is already cancelled or closed by hand.
pub async fn close<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    cancel: bool,
) -> DomainResult<ClosedAppointment> {
    let existing = require(db, vehicle_id, id).await?;
    if existing.cancelled_on.is_some() || existing.closed_on.is_some() {
        return Err(DomainError::BadRequest(format!(
            "Appointment {id} is already {}",
            if existing.cancelled_on.is_some() {
                "cancelled"
            } else {
                "closed"
            }
        )));
    }
    let today = super::today().format("%Y-%m-%d").to_string();
    let txn = db.begin().await?;
    let unassigned_job_ids: Vec<i32> = work_item::Entity::find()
        .filter(work_item::Column::AppointmentId.eq(id))
        .filter(work_item::Column::Status.eq("planned"))
        .order_by_asc(work_item::Column::Id)
        .all(&txn)
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
            .exec(&txn)
            .await?;
    }
    let mut active: appointment::ActiveModel = existing.into();
    if cancel {
        active.cancelled_on = Set(Some(today));
    } else {
        active.closed_on = Set(Some(today));
    }
    active.updated_at = Set(super::now_stamp());
    let appointment = active.update(&txn).await?;
    txn.commit().await?;
    Ok(ClosedAppointment {
        appointment,
        unassigned_job_ids,
    })
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
        test_support::{VehicleFixture, test_db},
    };

    fn appt(purpose: &str) -> NewAppointment {
        NewAppointment {
            purpose: purpose.into(),
            ..Default::default()
        }
    }

    async fn add_job(db: &DatabaseConnection, vehicle_id: i32, appointment_id: Option<i32>) -> i32 {
        job::create(
            db,
            vehicle_id,
            NewWorkItem {
                title: "Brake pads".into(),
                appointment_id,
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
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
        let mine = add_job(&db, v1, None).await;
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
        close(&db, v, a.id, true).await.unwrap();
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
        let b = create(&db, v, appt("Other")).await.unwrap();
        close(&db, v, b.id, false).await.unwrap();
        let mine = add_job(&db, v, None).await;
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
    }

    #[tokio::test]
    async fn close_unassigns_planned_jobs_and_keeps_their_needs() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = create(&db, v, appt("Brakes")).await.unwrap();
        let planned = add_job(&db, v, Some(a.id)).await;
        let finished = add_job(&db, v, Some(a.id)).await;
        job::update(
            &db,
            v,
            finished,
            UpdateWorkItem {
                status: Some("done".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
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

        let closed = close(&db, v, a.id, false).await.unwrap();
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

        let again = close(&db, v, a.id, true).await.unwrap_err();
        assert!(matches!(again, DomainError::BadRequest(_)));
        let reopened = reopen(&db, v, a.id).await.unwrap();
        assert!(reopened.closed_on.is_none() && reopened.cancelled_on.is_none());
        let cancelled = close(&db, v, a.id, true).await.unwrap();
        assert!(cancelled.appointment.cancelled_on.is_some());
        assert!(cancelled.appointment.closed_on.is_none());
        let again = close(&db, v, a.id, false).await.unwrap_err();
        assert!(matches!(again, DomainError::BadRequest(_)));
        let reopened = reopen(&db, v, a.id).await.unwrap();
        assert!(reopened.cancelled_on.is_none());
    }

    #[tokio::test]
    async fn delete_keeps_jobs() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = create(&db, v, appt("Brakes")).await.unwrap();
        let j = add_job(&db, v, Some(a.id)).await;
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
        let j1 = add_job(&db, v, Some(a.id)).await;
        let j2 = add_job(&db, v, Some(a.id)).await;
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

        close(&db, v, unbooked.id, true).await.unwrap();
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
        same_as_missing(close(&db, v2, a.id, false).await.unwrap_err());
        same_as_missing(reopen(&db, v2, a.id).await.unwrap_err());
        same_as_missing(delete(&db, v2, a.id).await.unwrap_err());
        assert!(require(&db, v1, a.id).await.is_ok());
    }

    #[tokio::test]
    async fn open_list_keeps_a_done_appointment_that_still_has_an_open_todo() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = create(&db, v, appt("Brakes")).await.unwrap();
        let job_id = add_job(&db, v, Some(a.id)).await;
        job::update(
            &db,
            v,
            job_id,
            UpdateWorkItem {
                status: Some("done".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
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
        close(&db, v, a.id, false).await.unwrap();
        assert!(list(&db, v, false).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn needs_on_a_dropped_job_are_not_open() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = create(&db, v, appt("Brakes")).await.unwrap();
        let kept = add_job(&db, v, Some(a.id)).await;
        let dropped = add_job(&db, v, Some(a.id)).await;
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
        job::update(
            &db,
            v,
            dropped,
            UpdateWorkItem {
                status: Some("dropped".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let all = list(&db, v, true).await.unwrap();
        assert_eq!(all[0].needs_open, 1);
    }
}
