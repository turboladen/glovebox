//! The one-call answer to "what is coming for this car": scheduled
//! maintenance, the to-do list, open recalls and unresolved incidents.

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::incident,
    error::DomainResult,
    services::{
        appointment::{self, AppointmentSummary},
        budget::{self, BudgetForecast},
        incident as incident_svc,
        recall::{self, RecallWithStatus},
        reminders::{self, RemindersResponse},
        work_item::{self, PlannedItem},
    },
};

/// Everything the owner should know is coming for one vehicle.
#[derive(Debug, Serialize)]
pub struct Upcoming {
    /// Scheduled maintenance with each item's status, the estimated mileage
    /// and the warranty status.
    #[serde(flatten)]
    pub reminders: RemindersResponse,
    /// The 12-month cost forecast.
    pub budget: BudgetForecast,
    /// Open to-do items, most urgent revisit first. Items sharing a revisit
    /// status keep creation order, and items with no revisit status come last.
    pub planned_work: Vec<PlannedItem>,
    /// The sum of the planned items' estimates; an item without one counts as zero.
    pub planned_work_est_cost_cents: i32,
    /// Recalls that are still `open` or `planned`.
    pub recalls: Vec<RecallWithStatus>,
    /// Unresolved incidents, excluding notes.
    pub open_incidents: Vec<incident::Model>,
    /// Appointments still being prepared, soonest first, each with its open
    /// counts.
    pub appointments: Vec<AppointmentSummary>,
}

fn urgency(status: Option<&str>) -> u8 {
    match status {
        Some("overdue") => 0,
        Some("due_soon") => 1,
        Some("ok") => 2,
        _ => 3,
    }
}

/// Answers "what is coming" for a vehicle in one read.
///
/// A vehicle with thin data still gets an answer: with no mileage readings
/// the mileage estimate falls back to the purchase mileage, with no schedule
/// template the reminders are empty, and a work item is judged on whichever
/// of its revisit date and revisit mileage it can be judged on.
///
/// Returns `NotFound` when the vehicle does not exist.
pub async fn upcoming(db: &impl ConnectionTrait, vehicle_id: i32) -> DomainResult<Upcoming> {
    let reminders = reminders::calculate_reminders(db, vehicle_id).await?;
    let budget = budget::forecast_from(db, vehicle_id, &reminders).await?;

    let mut planned_work: Vec<PlannedItem> = work_item::list_with_status(db, vehicle_id, false)
        .await?
        .into_iter()
        .filter(|p| p.item.status == "planned")
        .collect();
    planned_work.sort_by_key(|p| (urgency(p.revisit_status), p.item.id));
    let planned_work_est_cost_cents = planned_work
        .iter()
        .map(|p| p.item.est_cost_cents.unwrap_or(0))
        .sum();

    let recalls = recall::list(db, vehicle_id)
        .await?
        .into_iter()
        .filter(|r| r.status == "open" || r.status == "planned")
        .collect();
    let open_incidents = incident_svc::list_rows(db, vehicle_id, Some(false))
        .await?
        .into_iter()
        .filter(|i| i.category != "note")
        .collect();

    let appointments = appointment::list(db, vehicle_id, false).await?;

    Ok(Upcoming {
        reminders,
        budget,
        planned_work,
        planned_work_est_cost_cents,
        recalls,
        open_incidents,
        appointments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::model_template,
        error::DomainError,
        inputs::{
            appointment::NewAppointment, incident::NewIncident, mileage::NewMileageEntry,
            need::NewNeed, todo::NewTodo, work_item::NewWorkItem,
        },
        services::{
            appointment, incident as incident_svc, mileage, need, nhtsa::RecallInfo, recall,
            research, todo, work_item as work_item_svc,
        },
        test_support::{VehicleFixture, test_db},
    };

    fn titles(items: &[PlannedItem]) -> Vec<&str> {
        items.iter().map(|p| p.item.title.as_str()).collect()
    }

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

    async fn gti_template_id(db: &sea_orm::DatabaseConnection) -> i32 {
        model_template::Entity::find()
            .filter(model_template::Column::Model.eq("Golf GTI"))
            .one(db)
            .await
            .unwrap()
            .expect("seeded GTI template")
            .id
    }

    #[tokio::test]
    async fn whats_coming_lists_open_appointments_with_counts() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let open = appointment::create(
            &db,
            v,
            NewAppointment {
                purpose: "60k service".into(),
                planned_date: Some("2026-12-01".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let cancelled = appointment::create(
            &db,
            v,
            NewAppointment {
                purpose: "Detailing".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        appointment::close(&db, v, cancelled.id, true)
            .await
            .unwrap();
        let job = work_item_svc::create(
            &db,
            v,
            NewWorkItem {
                appointment_id: Some(open.id),
                ..work("Brakes")
            },
        )
        .await
        .unwrap();
        need::add(
            &db,
            v,
            job.id,
            NewNeed {
                name: "Rear pads".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        todo::add(
            &db,
            v,
            NewTodo {
                title: "Bring wheel locks".into(),
                appointment_id: Some(open.id),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let got = upcoming(&db, v).await.unwrap();
        assert_eq!(got.appointments.len(), 1);
        let a = &got.appointments[0];
        assert_eq!(a.appointment.id, open.id);
        assert_eq!(a.appointment.purpose, "60k service");
        assert_eq!(a.appointment.planned_date.as_deref(), Some("2026-12-01"));
        assert_eq!((a.jobs, a.needs_open, a.todos_open), (1, 1, 1));
        assert_eq!(
            serde_json::to_value(&got).unwrap()["appointments"][0]["needs_open"],
            1
        );
    }

    #[tokio::test]
    async fn upcoming_merges_schedule_planned_work_recalls_and_open_incidents() {
        let db = test_db().await;
        let template_id = gti_template_id(&db).await;
        let v = VehicleFixture::new()
            .model_template_id(template_id)
            .purchase_date("2017-03-05")
            .purchase_mileage(40)
            .insert_id(&db)
            .await;
        let today = crate::services::today().format("%Y-%m-%d").to_string();
        mileage::create(
            &db,
            v,
            NewMileageEntry {
                mileage: 64000,
                recorded_at: Some(today),
                source: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        // Created in this order so creation order is the tiebreak within a group.
        work_item_svc::create(
            &db,
            v,
            NewWorkItem {
                est_cost_cents: Some(2500),
                ..work("Rear wiper")
            },
        )
        .await
        .unwrap();
        work_item_svc::create(
            &db,
            v,
            NewWorkItem {
                est_cost_cents: Some(18000),
                revisit_by_miles: Some(63000),
                ..work("Replace N80 valve")
            },
        )
        .await
        .unwrap();
        let finished = work_item_svc::create(&db, v, work("Already done"))
            .await
            .unwrap();
        work_item_svc::update(
            &db,
            v,
            finished.id,
            crate::inputs::work_item::UpdateWorkItem {
                status: Some("done".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let recalls = research::record_recalls(&db, v, &[recall_info("A1"), recall_info("B2")])
            .await
            .unwrap()
            .recalls;
        recall::set_status(&db, v, recalls[1].finding_id, "done")
            .await
            .unwrap();

        for (category, title) in [
            ("shop_observation", "Seeping valve cover"),
            ("note", "Fact"),
        ] {
            incident_svc::create(
                &db,
                v,
                NewIncident {
                    category: category.into(),
                    title: title.into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }

        let u = upcoming(&db, v).await.unwrap();
        assert!(
            u.reminders
                .reminders
                .iter()
                .any(|r| r.schedule_item.name == "Spark Plugs" && r.status == "overdue")
        );
        assert_eq!(
            titles(&u.planned_work),
            vec!["Replace N80 valve", "Rear wiper"]
        );
        assert_eq!(u.planned_work[0].revisit_status, Some("overdue"));
        assert_eq!(u.planned_work[1].revisit_status, None);
        assert_eq!(u.planned_work_est_cost_cents, 20500);
        assert_eq!(u.recalls.len(), 1);
        assert_eq!(u.recalls[0].status, "open");
        assert_eq!(u.open_incidents.len(), 1);
        assert_eq!(u.open_incidents[0].title, "Seeping valve cover");
    }

    #[tokio::test]
    async fn planned_work_sorts_by_urgency_then_creation_order() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let far = (crate::services::today() + chrono::Duration::days(400))
            .format("%Y-%m-%d")
            .to_string();
        let soon = (crate::services::today() + chrono::Duration::days(10))
            .format("%Y-%m-%d")
            .to_string();
        for (title, date) in [
            ("no revisit", None),
            ("ok", Some(far)),
            ("soon", Some(soon)),
            ("late", Some("2020-01-01".to_string())),
            ("also no revisit", None),
        ] {
            work_item_svc::create(
                &db,
                v,
                NewWorkItem {
                    revisit_by_date: date,
                    ..work(title)
                },
            )
            .await
            .unwrap();
        }
        let u = upcoming(&db, v).await.unwrap();
        assert_eq!(
            titles(&u.planned_work),
            vec!["late", "soon", "ok", "no revisit", "also no revisit"]
        );
    }

    #[tokio::test]
    async fn upcoming_works_for_a_bare_vehicle() {
        // No model template, no mileage readings, no purchase date, no records.
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let u = upcoming(&db, v).await.unwrap();
        assert!(u.reminders.reminders.is_empty());
        assert!(u.planned_work.is_empty() && u.recalls.is_empty() && u.open_incidents.is_empty());
        assert_eq!(u.planned_work_est_cost_cents, 0);
    }

    #[tokio::test]
    async fn a_dated_revisit_is_judged_without_any_mileage() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        work_item_svc::create(
            &db,
            v,
            NewWorkItem {
                revisit_by_date: Some("2020-01-01".into()),
                revisit_by_miles: Some(50000),
                ..work("Check mount")
            },
        )
        .await
        .unwrap();
        let u = upcoming(&db, v).await.unwrap();
        assert_eq!(u.planned_work[0].revisit_status, Some("overdue"));
    }

    #[tokio::test]
    async fn upcoming_for_an_unknown_vehicle_is_not_found() {
        let db = test_db().await;
        let err = upcoming(&db, 9999).await.unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)), "got {err:?}");
    }
}
