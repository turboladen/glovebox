//! What is waiting for a shop trip: the maintenance that is due, the planned
//! jobs on no appointment, and the open recalls and incidents.

use std::collections::HashMap;

use chrono::NaiveDate;
use sea_orm::*;
use serde::Serialize;

use crate::{
    dates::DayOrTime,
    entities::{incident::IncidentCategory, work_item},
    error::DomainResult,
    services::{checklist, reminders, upcoming, work_item::PlannedItem},
};

/// Everything on one vehicle that is not yet on an appointment.
///
/// A maintenance item, recall or incident that a planned job covers names
/// that job in `job_id`. One whose job is on an appointment is left out,
/// because the appointment already lists the job.
#[derive(Debug, Serialize)]
pub struct Waiting {
    /// The odometer reading the maintenance was judged against.
    pub estimated_mileage: i32,
    /// True when that reading is projected from older readings.
    pub mileage_is_estimate: bool,
    /// The date of the newest reading behind it.
    pub mileage_as_of: NaiveDate,
    /// Schedule items that are overdue or due soon, overdue first.
    pub maintenance: Vec<WaitingMaintenance>,
    /// Counts the due schedule items left out because their job is on an
    /// appointment.
    pub maintenance_booked: usize,
    /// Planned jobs on no appointment, most urgent revisit first.
    pub jobs: Vec<WaitingJob>,
    /// Recalls that are `open` or `planned`.
    pub recalls: Vec<WaitingRecall>,
    /// Counts the open recalls left out because their job is on an appointment.
    pub recalls_booked: usize,
    /// Unresolved incidents, newest first, excluding notes.
    pub incidents: Vec<WaitingIncident>,
    /// Counts the open incidents left out because their job is on an
    /// appointment.
    pub incidents_booked: usize,
}

/// Says how many items a list leaves out because their job is on an
/// appointment, such as `plus 2 on an appointment`, or `None` when it leaves
/// out none.
#[must_use]
pub fn booked_note(booked: usize) -> Option<String> {
    (booked > 0).then(|| format!("plus {booked} on an appointment"))
}

/// A schedule item that is overdue or due soon.
#[derive(Debug, Serialize)]
pub struct WaitingMaintenance {
    pub schedule_item_id: i32,
    pub name: String,
    /// `overdue` or `due_soon`.
    pub status: String,
    pub due_at_miles: Option<i32>,
    pub due_at_date: Option<NaiveDate>,
    /// How far past or short of due the item is, as
    /// [`reminders::how_far`] words it.
    pub how_far: String,
    /// The date of the last service that did this item, when one is recorded.
    pub last_done: Option<NaiveDate>,
    /// The planned job that covers this item.
    pub job_id: Option<i32>,
}

/// A planned job that is on no appointment.
#[derive(Debug, Serialize)]
pub struct WaitingJob {
    pub id: i32,
    pub title: String,
    /// `overdue`, `due_soon` or `ok` for a job with a revisit date or mileage.
    pub revisit_status: Option<&'static str>,
    pub est_cost_cents: Option<i32>,
    /// Counts the job's needs that are still open.
    pub needs_open: usize,
}

/// An open recall.
#[derive(Debug, Serialize)]
pub struct WaitingRecall {
    pub finding_id: i32,
    pub title: String,
    pub campaign_number: Option<String>,
    /// The planned job that covers this recall.
    pub job_id: Option<i32>,
}

/// An unresolved incident.
#[derive(Debug, Serialize)]
pub struct WaitingIncident {
    pub id: i32,
    pub title: String,
    pub category: IncidentCategory,
    pub occurred_at: DayOrTime,
    /// The planned job that covers this incident.
    pub job_id: Option<i32>,
}

// Whether a planned job covers something, and whether that job is booked.
enum Cover {
    None,
    Planned(i32),
    Booked,
}

impl Cover {
    // Judges the planned jobs that point at one thing. A job on an
    // appointment wins, because a planned job is only ever on an open one.
    fn of<'a>(jobs: impl Iterator<Item = &'a work_item::Model>) -> Self {
        let mut cover = Self::None;
        for job in jobs {
            if job.appointment_id.is_some() {
                return Self::Booked;
            }
            if matches!(cover, Self::None) {
                cover = Self::Planned(job.id);
            }
        }
        cover
    }

    // The row's `job_id`, or `None` for a row that is left out.
    fn job_id(self) -> Option<Option<i32>> {
        match self {
            Self::None => Some(None),
            Self::Planned(id) => Some(Some(id)),
            Self::Booked => None,
        }
    }
}

/// What is waiting on one vehicle.
///
/// # Errors
/// `NotFound` for a vehicle that does not exist.
pub async fn waiting(db: &impl ConnectionTrait, vehicle_id: i32) -> DomainResult<Waiting> {
    let reminders = reminders::calculate_reminders(db, vehicle_id).await?;
    let planned = upcoming::planned_work(db, vehicle_id).await?;
    let covering = |link: fn(&work_item::Model) -> Option<i32>, id: i32| {
        Cover::of(
            planned
                .iter()
                .map(|p| &p.item)
                .filter(move |job| link(job) == Some(id)),
        )
        .job_id()
    };

    let due: Vec<_> = reminders
        .reminders
        .iter()
        .filter(|r| r.status == "overdue" || r.status == "due_soon")
        .collect();
    let maintenance: Vec<WaitingMaintenance> = due
        .iter()
        .filter_map(|r| {
            let job_id = covering(|job| job.schedule_item_id, r.schedule_item.id)?;
            Some(WaitingMaintenance {
                schedule_item_id: r.schedule_item.id,
                name: r.schedule_item.name.clone(),
                status: r.status.clone(),
                due_at_miles: r.due_at_miles,
                due_at_date: r.due_at_date,
                how_far: reminders::how_far(r.miles_remaining, r.days_remaining),
                last_done: r.last_service.as_ref().map(|s| s.date),
                job_id,
            })
        })
        .collect();

    let open_recalls = upcoming::open_recalls(db, vehicle_id).await?;
    let recalls_open = open_recalls.len();
    let recalls: Vec<WaitingRecall> = open_recalls
        .into_iter()
        .filter_map(|r| {
            let job_id = covering(|job| job.research_finding_id, r.finding_id)?;
            Some(WaitingRecall {
                finding_id: r.finding_id,
                title: r.title,
                campaign_number: r.campaign_number,
                job_id,
            })
        })
        .collect();

    let open_incidents = upcoming::open_incidents(db, vehicle_id).await?;
    let incidents_open = open_incidents.len();
    let incidents: Vec<WaitingIncident> = open_incidents
        .into_iter()
        .filter_map(|i| {
            let job_id = covering(|job| job.incident_id, i.id)?;
            Some(WaitingIncident {
                id: i.id,
                title: i.title,
                category: i.category,
                occurred_at: i.occurred_at,
                job_id,
            })
        })
        .collect();

    let unbooked: Vec<PlannedItem> = planned
        .into_iter()
        .filter(|p| p.item.appointment_id.is_none())
        .collect();
    let needs_open: HashMap<i32, usize> =
        checklist::open_needs_by_job(db, unbooked.iter().map(|p| p.item.clone()).collect()).await?;
    let jobs = unbooked
        .into_iter()
        .map(|p| WaitingJob {
            needs_open: needs_open.get(&p.item.id).copied().unwrap_or(0),
            id: p.item.id,
            title: p.item.title,
            revisit_status: p.revisit_status,
            est_cost_cents: p.item.est_cost_cents,
        })
        .collect();

    Ok(Waiting {
        estimated_mileage: reminders.estimated_mileage,
        mileage_is_estimate: reminders.mileage_is_estimate,
        mileage_as_of: reminders.mileage_as_of,
        maintenance_booked: due.len() - maintenance.len(),
        maintenance,
        jobs,
        recalls_booked: recalls_open - recalls.len(),
        recalls,
        incidents_booked: incidents_open - incidents.len(),
        incidents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::model_template,
        error::DomainError,
        inputs::{
            appointment::NewAppointment,
            incident::NewIncident,
            mileage::NewMileageEntry,
            need::NewNeed,
            work_item::{NewWorkItem, UpdateWorkItem},
        },
        services::{
            appointment, incident as incident_svc, mileage, need, nhtsa::RecallInfo, recall,
            research, work_item as work_item_svc,
        },
        test_support::{VehicleFixture, test_db},
    };

    fn work(title: &str) -> NewWorkItem {
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

    fn recall_info(campaign: &str) -> RecallInfo {
        RecallInfo {
            campaign_number: campaign.into(),
            manufacturer: None,
            title: format!("Recall {campaign}"),
            summary: None,
            consequence: None,
            remedy: None,
            report_date: None,
            component: None,
            action_number: None,
        }
    }

    // A GTI on the seeded schedule at 64,000 miles, where the 60k spark plugs
    // are overdue.
    async fn gti_at_64k(db: &DatabaseConnection) -> i32 {
        let template_id = model_template::Entity::find()
            .filter(model_template::Column::Model.eq("Golf GTI"))
            .one(db)
            .await
            .unwrap()
            .expect("seeded GTI template")
            .id;
        let v = VehicleFixture::new()
            .model_template_id(template_id)
            .purchase_date("2017-03-05")
            .purchase_mileage(40)
            .insert_id(db)
            .await;
        mileage::create(
            db,
            v,
            NewMileageEntry {
                mileage: 64000,
                recorded_at: Some(crate::services::today().into()),
                source: None,
                notes: None,
            },
        )
        .await
        .unwrap();
        v
    }

    async fn job(db: &DatabaseConnection, v: i32, input: NewWorkItem) -> i32 {
        work_item_svc::create(db, v, input).await.unwrap().id
    }

    async fn open_appointment(db: &DatabaseConnection, v: i32) -> i32 {
        appointment::create(
            db,
            v,
            NewAppointment {
                purpose: "Shop trip".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    async fn incident(
        db: &DatabaseConnection,
        v: i32,
        category: IncidentCategory,
        title: &str,
    ) -> i32 {
        incident_svc::create(
            db,
            v,
            NewIncident {
                category,
                title: title.into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .incident
        .id
    }

    fn plugs(w: &Waiting) -> Option<&WaitingMaintenance> {
        w.maintenance.iter().find(|m| m.name == "Spark Plugs")
    }

    #[tokio::test]
    async fn maintenance_lists_only_items_that_are_overdue_or_due_soon() {
        let db = test_db().await;
        let v = gti_at_64k(&db).await;

        let w = waiting(&db, v).await.unwrap();

        assert!(!w.maintenance.is_empty());
        assert!(
            w.maintenance
                .iter()
                .all(|m| m.status == "overdue" || m.status == "due_soon")
        );
        let all = reminders::calculate_reminders(&db, v).await.unwrap();
        let due = all.reminders.iter().filter(|r| r.status != "ok").count();
        assert!(
            due < all.reminders.len(),
            "the fixture has items not yet due"
        );
        assert_eq!(w.maintenance.len(), due);

        let plugs = plugs(&w).expect("spark plugs are overdue");
        assert_eq!(plugs.status, "overdue");
        assert_eq!(plugs.due_at_miles, Some(60040));
        assert!(
            plugs.how_far.starts_with("3,960 mi over"),
            "{}",
            plugs.how_far
        );
        assert_eq!((plugs.last_done, plugs.job_id), (None, None));
        assert_eq!((w.estimated_mileage, w.mileage_is_estimate), (64000, false));
        assert_eq!(w.mileage_as_of, crate::services::today());
    }

    #[tokio::test]
    async fn jobs_lists_every_planned_job_on_no_appointment_most_urgent_first() {
        let db = test_db().await;
        let v = gti_at_64k(&db).await;
        let appt = open_appointment(&db, v).await;
        let wiper = job(
            &db,
            v,
            NewWorkItem {
                est_cost_cents: Some(2500),
                ..work("Rear wiper")
            },
        )
        .await;
        job(
            &db,
            v,
            NewWorkItem {
                revisit_by_miles: Some(63000),
                ..work("Replace N80 valve")
            },
        )
        .await;
        job(
            &db,
            v,
            NewWorkItem {
                appointment_id: Some(appt),
                ..work("Booked brakes")
            },
        )
        .await;
        let finished = job(&db, v, work("Already done")).await;
        work_item_svc::update(
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
            wiper,
            NewNeed {
                name: "Blade".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let w = waiting(&db, v).await.unwrap();

        let rows: Vec<_> = w
            .jobs
            .iter()
            .map(|j| (j.title.as_str(), j.revisit_status, j.needs_open))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("Replace N80 valve", Some("overdue"), 0),
                ("Rear wiper", None, 1),
            ]
        );
        assert_eq!(w.jobs[1].est_cost_cents, Some(2500));
    }

    #[tokio::test]
    async fn a_covered_item_names_its_job_and_a_booked_one_is_left_out() {
        let db = test_db().await;
        let v = gti_at_64k(&db).await;
        let plugs_id = plugs(&waiting(&db, v).await.unwrap())
            .unwrap()
            .schedule_item_id;
        let recalls = research::record_recalls(&db, v, &[recall_info("A1"), recall_info("B2")])
            .await
            .unwrap()
            .recalls;
        recall::set_status(&db, v, recalls[1].finding_id, "done")
            .await
            .unwrap();
        let leak = incident(
            &db,
            v,
            IncidentCategory::ShopObservation,
            "Seeping valve cover",
        )
        .await;
        incident(&db, v, IncidentCategory::Note, "Fact").await;

        let plug_job = job(
            &db,
            v,
            NewWorkItem {
                schedule_item_id: Some(plugs_id),
                ..work("Do the plugs")
            },
        )
        .await;
        let recall_job = job(
            &db,
            v,
            NewWorkItem {
                research_finding_id: Some(recalls[0].finding_id),
                ..work("Recall A1")
            },
        )
        .await;
        let leak_job = job(
            &db,
            v,
            NewWorkItem {
                incident_id: Some(leak),
                ..work("Fix the leak")
            },
        )
        .await;

        let w = waiting(&db, v).await.unwrap();
        assert_eq!(plugs(&w).unwrap().job_id, Some(plug_job));
        assert_eq!(w.recalls.len(), 1, "the done recall is not waiting");
        assert_eq!(
            (w.recalls[0].title.as_str(), w.recalls[0].job_id),
            ("Recall A1", Some(recall_job))
        );
        assert_eq!(w.recalls[0].campaign_number.as_deref(), Some("A1"));
        assert_eq!(w.incidents.len(), 1, "a note is not waiting");
        assert_eq!(
            (w.incidents[0].title.as_str(), w.incidents[0].job_id),
            ("Seeping valve cover", Some(leak_job))
        );
        assert_eq!(w.jobs.len(), 3);
        assert_eq!(
            (w.maintenance_booked, w.recalls_booked, w.incidents_booked),
            (0, 0, 0)
        );

        let appt = open_appointment(&db, v).await;
        for id in [plug_job, recall_job, leak_job] {
            work_item_svc::update(
                &db,
                v,
                id,
                UpdateWorkItem {
                    appointment_id: Some(Some(appt)),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }

        let w = waiting(&db, v).await.unwrap();
        assert!(plugs(&w).is_none());
        assert!(!w.maintenance.is_empty(), "other due items still wait");
        assert!(w.recalls.is_empty() && w.incidents.is_empty() && w.jobs.is_empty());
        assert_eq!(
            (w.maintenance_booked, w.recalls_booked, w.incidents_booked),
            (1, 1, 1)
        );
    }

    #[test]
    fn booked_note_counts_what_a_list_leaves_out() {
        assert_eq!(booked_note(0), None);
        assert_eq!(booked_note(2).as_deref(), Some("plus 2 on an appointment"));
    }

    #[tokio::test]
    async fn a_vehicle_with_no_schedule_and_no_records_has_nothing_waiting() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;

        let w = waiting(&db, v).await.unwrap();

        assert!(w.maintenance.is_empty() && w.jobs.is_empty());
        assert!(w.recalls.is_empty() && w.incidents.is_empty());
    }

    #[tokio::test]
    async fn a_missing_vehicle_is_not_found() {
        let db = test_db().await;
        assert!(matches!(
            waiting(&db, 999).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }
}
