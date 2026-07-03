//! 12-month spend forecast (2hea unit G, decision ⑧): schedule intervals ×
//! the vehicle's actual mileage rate × each item's `est_cost_cents`, plus
//! open visits' estimated-cost rollups, plus the unattached planned-work
//! backlog. Money is integer cents throughout; only the day arithmetic
//! touches floats (the mileage rate).
//!
//! Baselines, due dates, and the mileage rate all come from
//! [`reminders::calculate_reminders`] (extract-don't-duplicate: the rate
//! machinery lives in `reminders::estimate_mileage`, which the reminders
//! response already surfaces as `avg_daily_miles`).

use std::collections::{HashMap, HashSet};

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{maintenance_schedule_item, work_item},
    error::DomainResult,
    services::{reminders, reminders::RemindersResponse, visit, work_item as work_item_svc},
};

/// Forecast horizon: days `0..HORIZON_DAYS` from today (an occurrence
/// landing exactly on day 365 is next year's spend).
const HORIZON_DAYS: i64 = 365;
const HORIZON_MONTHS: u32 = 12;

/// The three-bucket forecast model. Each dollar appears in exactly one
/// bucket: schedule projections (minus occurrences already planned in an
/// open visit), open visits' item rollups, and the unattached backlog.
#[derive(Debug, Serialize)]
pub struct BudgetForecast {
    pub horizon_months: u32,
    /// Σ projected schedule-item occurrences within the horizon. A
    /// schedule item that appears (via a work item's `schedule_item_id`)
    /// in an OPEN visit skips its FIRST projected occurrence — that spend
    /// is already counted in `planned_visits_cents` — but later cycles
    /// still project.
    pub projected_maintenance_cents: i64,
    /// Σ open (planned/scheduled) visits' `est_total_cents`.
    pub planned_visits_cents: i64,
    /// Σ participating (planned/scheduled) work items carrying
    /// `est_cost_cents` that are NOT attached to any visit — the backlog
    /// the user intends to do but hasn't scheduled.
    pub planned_work_cents: i64,
    /// `projected_maintenance_cents + planned_visits_cents +
    /// planned_work_cents`.
    pub total_cents: i64,
    pub lines: Vec<ForecastLine>,
}

/// One projected occurrence of a schedule item.
#[derive(Debug, Serialize)]
pub struct ForecastLine {
    pub label: String,
    /// Projected date, `YYYY-MM-DD` (today for overdue items).
    pub when: String,
    pub est_cents: i64,
}

/// Project the vehicle's next 12 months of maintenance spend.
///
/// For each enabled resolved schedule item carrying `est_cost_cents`:
/// occurrences repeat at the item's interval — `interval_months`, and/or
/// `interval_miles` divided by the vehicle's actual mileage rate (whichever
/// cycle is shorter). Overdue items count as one occurrence now. Items with
/// no projectable due date (e.g. mileage-only interval on a vehicle with no
/// mileage history) are skipped unless overdue.
pub async fn forecast(db: &DatabaseConnection, vehicle_id: i32) -> DomainResult<BudgetForecast> {
    let rems = reminders::calculate_reminders(db, vehicle_id).await?;
    forecast_from(db, vehicle_id, &rems).await
}

/// [`forecast`] with the reminders precomputed — for callers (the MCP's
/// `check_due_maintenance`) that already have a [`RemindersResponse`] and
/// must not compute it twice.
#[allow(clippy::cast_possible_truncation)] // day counts, not money
pub async fn forecast_from(
    db: &DatabaseConnection,
    vehicle_id: i32,
    rems: &RemindersResponse,
) -> DomainResult<BudgetForecast> {
    // The reminders response carries id + name; the intervals and est_cost
    // live on the schedule items themselves.
    let item_ids: Vec<i32> = rems.reminders.iter().map(|r| r.schedule_item.id).collect();
    let items: HashMap<i32, maintenance_schedule_item::Model> = if item_ids.is_empty() {
        HashMap::new()
    } else {
        maintenance_schedule_item::Entity::find()
            .filter(maintenance_schedule_item::Column::Id.is_in(item_ids))
            .all(db)
            .await?
            .into_iter()
            .map(|i| (i.id, i))
            .collect()
    };

    // Open (planned/scheduled) visits: their item rollups are one bucket,
    // and their participating items' schedule links suppress the first
    // projected occurrence below (it's the same spend, already planned).
    let open_visits = visit::list(db, vehicle_id, false).await?;
    let planned_in_visit: HashSet<i32> = open_visits
        .iter()
        .flat_map(|v| v.items.iter())
        .filter(|i| work_item_svc::participates(&i.status))
        .filter_map(|i| i.schedule_item_id)
        .collect();

    let today = chrono::Utc::now().date_naive();
    let avg_daily = rems.avg_daily_miles;
    let miles_to_days = |miles: i32| -> Option<i64> {
        (avg_daily > 0.0).then(|| (f64::from(miles) / avg_daily).ceil() as i64)
    };

    let mut lines: Vec<ForecastLine> = Vec::new();
    for r in &rems.reminders {
        let Some(item) = items.get(&r.schedule_item.id) else {
            continue;
        };
        let Some(cost) = item.est_cost_cents else {
            continue;
        };
        let cost = i64::from(cost);

        // Days until the first occurrence; overdue counts as one now.
        let first_due: Option<i64> = if r.status == "overdue" {
            Some(0)
        } else {
            [r.days_remaining, r.miles_remaining.and_then(miles_to_days)]
                .into_iter()
                .flatten()
                .min()
        };
        let Some(first) = first_due else { continue };
        if first >= HORIZON_DAYS {
            continue;
        }

        // Repeat cycle in days: the shorter of the time-based and
        // mileage-based intervals. No projectable cycle → one occurrence.
        let cycle_days: Option<i64> = [
            item.interval_months.map(|m| i64::from(m) * 365 / 12),
            item.interval_miles.and_then(miles_to_days),
        ]
        .into_iter()
        .flatten()
        .filter(|d| *d >= 1)
        .min();

        let mut day = first.max(0);
        // Already planned in an open visit: the first occurrence is counted
        // in `planned_visits_cents`; only later cycles project here.
        if planned_in_visit.contains(&item.id) {
            let Some(cycle) = cycle_days else { continue };
            day += cycle;
            if day >= HORIZON_DAYS {
                continue;
            }
        }
        loop {
            lines.push(ForecastLine {
                label: item.name.clone(),
                when: (today + chrono::Duration::days(day))
                    .format("%Y-%m-%d")
                    .to_string(),
                est_cents: cost,
            });
            let Some(cycle) = cycle_days else { break };
            day += cycle;
            if day >= HORIZON_DAYS {
                break;
            }
        }
    }
    lines.sort_by(|a, b| a.when.cmp(&b.when).then_with(|| a.label.cmp(&b.label)));
    let projected_maintenance_cents: i64 = lines.iter().map(|l| l.est_cents).sum();

    let planned_visits_cents: i64 = open_visits.iter().map(|v| v.est_total_cents).sum();

    // The unattached backlog: participating items with an estimate that
    // aren't scheduled into any visit yet.
    let planned_work_cents: i64 = work_item::Entity::find()
        .filter(work_item::Column::VehicleId.eq(vehicle_id))
        .filter(work_item::Column::VisitId.is_null())
        .filter(work_item::Column::Status.is_in(["planned", "scheduled"]))
        .all(db)
        .await?
        .iter()
        .filter_map(|i| i.est_cost_cents)
        .map(i64::from)
        .sum();

    Ok(BudgetForecast {
        horizon_months: HORIZON_MONTHS,
        projected_maintenance_cents,
        planned_visits_cents,
        planned_work_cents,
        total_cents: projected_maintenance_cents + planned_visits_cents + planned_work_cents,
        lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::{mileage_log, service_record, service_schedule_link, vehicle},
        error::DomainError,
        inputs::{
            visit::{NewVisit, UpdateVisit},
            work_item::NewWorkItem,
        },
        services::work_item as work_item_svc,
        test_support::test_db,
    };

    fn days_ago(days: i64) -> String {
        (chrono::Utc::now().date_naive() - chrono::Duration::days(days))
            .format("%Y-%m-%d")
            .to_string()
    }

    async fn seed_item(
        db: &impl ConnectionTrait,
        vehicle_id: i32,
        name: &str,
        interval_miles: Option<i32>,
        interval_months: Option<i32>,
        est_cost_cents: Option<i32>,
    ) -> i32 {
        maintenance_schedule_item::ActiveModel {
            vehicle_id: Set(Some(vehicle_id)),
            name: Set(name.into()),
            interval_miles: Set(interval_miles),
            interval_months: Set(interval_months),
            est_cost_cents: Set(est_cost_cents),
            enabled: Set(true),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn missing_vehicle_is_not_found() {
        let db = test_db().await;
        assert!(matches!(
            forecast(&db, 999).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn empty_schedule_forecasts_zero() {
        let db = test_db().await;
        let vid = vehicle::ActiveModel {
            name: Set("Car".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;

        let f = forecast(&db, vid).await.unwrap();
        assert_eq!(f.horizon_months, 12);
        assert_eq!(f.projected_maintenance_cents, 0);
        assert_eq!(f.planned_visits_cents, 0);
        assert_eq!(f.planned_work_cents, 0);
        assert_eq!(f.total_cents, 0);
        assert!(f.lines.is_empty());
    }

    /// The pinned arithmetic scenario. Everything is seeded relative to
    /// today (no wall-clock flakiness):
    ///
    /// - "Annual inspection": 12-month interval, $100, last serviced ~6
    ///   months ago → due in ~6 months → exactly 1 occurrence.
    /// - "Timing belt check": 12-month interval, $50, never serviced on a
    ///   vehicle purchased 2 years ago → overdue → 1 occurrence now (the
    ///   next lands ~12 months out, past the horizon).
    /// - "Oil change": 5000-mile interval, $20, mileage rate pinned at
    ///   exactly 100 mi/day (logs 100 days apart, 10k miles apart, latest
    ///   today) → overdue (est. 20k vs due at 5k), cycle = 50 days →
    ///   occurrences at days 0,50,...,350 = 8.
    /// - An open visit with a $75 item; a canceled visit with a $999 item
    ///   that must NOT count as a visit — canceling detaches the item back
    ///   to the backlog, so it lands in `planned_work_cents` instead.
    #[tokio::test]
    async fn pinned_interval_and_rate_scenario() {
        let db = test_db().await;
        let vid = vehicle::ActiveModel {
            name: Set("Car".into()),
            purchase_date: Set(Some(days_ago(730))),
            purchase_mileage: Set(Some(0)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;

        // Pin the mileage rate: 10_000 miles over 100 days, latest today.
        for (age, miles) in [(100, 10_000), (0, 20_000)] {
            mileage_log::ActiveModel {
                vehicle_id: Set(vid),
                mileage: Set(miles),
                recorded_at: Set(days_ago(age)),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }

        let annual = seed_item(&db, vid, "Annual inspection", None, Some(12), Some(10_000)).await;
        seed_item(&db, vid, "Timing belt check", None, Some(12), Some(5_000)).await;
        seed_item(&db, vid, "Oil change", Some(5_000), None, Some(2_000)).await;
        // An item without est_cost contributes nothing.
        seed_item(&db, vid, "Free checkup", None, Some(12), None).await;

        // The annual inspection was serviced ~6 months ago (mileage-less
        // record: no auto-log interference with the pinned rate).
        let svc = service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set(days_ago(182)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        service_schedule_link::ActiveModel {
            service_record_id: Set(svc.id),
            schedule_item_id: Set(annual),
        }
        .insert(&db)
        .await
        .unwrap();

        // One open visit ($75 item) and one canceled visit ($999 item).
        let open_item = work_item_svc::create(
            &db,
            vid,
            NewWorkItem {
                title: "Wipers".into(),
                notes: None,
                schedule_item_id: None,
                research_finding_id: None,
                incident_id: None,
                build_id: None,
                est_cost_cents: Some(7_500),
                visit_id: None,
            },
        )
        .await
        .unwrap();
        crate::services::visit::create(
            &db,
            vid,
            NewVisit {
                planned_date: None,
                shop_name: None,
                shop_id: None,
                notes: None,
                work_item_ids: Some(vec![open_item.id]),
            },
        )
        .await
        .unwrap();
        let dead_item = work_item_svc::create(
            &db,
            vid,
            NewWorkItem {
                title: "Never happening".into(),
                notes: None,
                schedule_item_id: None,
                research_finding_id: None,
                incident_id: None,
                build_id: None,
                est_cost_cents: Some(99_900),
                visit_id: None,
            },
        )
        .await
        .unwrap();
        let canceled = crate::services::visit::create(
            &db,
            vid,
            NewVisit {
                planned_date: None,
                shop_name: None,
                shop_id: None,
                notes: None,
                work_item_ids: Some(vec![dead_item.id]),
            },
        )
        .await
        .unwrap();
        crate::services::visit::update(
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

        let f = forecast(&db, vid).await.unwrap();

        let count = |label: &str| f.lines.iter().filter(|l| l.label == label).count();
        assert_eq!(count("Annual inspection"), 1, "lines: {:?}", f.lines);
        assert_eq!(count("Timing belt check"), 1, "lines: {:?}", f.lines);
        assert_eq!(count("Oil change"), 8, "lines: {:?}", f.lines);
        assert_eq!(count("Free checkup"), 0, "no est_cost, no forecast");

        // 1×10_000 + 1×5_000 + 8×2_000 = 31_000, all integer cents.
        assert_eq!(f.projected_maintenance_cents, 31_000);
        assert_eq!(f.planned_visits_cents, 7_500, "canceled visit excluded");
        // Canceling detached the $999 item back to the planned backlog.
        assert_eq!(f.planned_work_cents, 99_900, "unattached backlog bucket");
        assert_eq!(f.total_cents, 31_000 + 7_500 + 99_900);

        // Overdue occurrences land today; lines are date-ordered.
        assert_eq!(f.lines[0].when, days_ago(0));
        let whens: Vec<&str> = f.lines.iter().map(|l| l.when.as_str()).collect();
        let mut sorted = whens.clone();
        sorted.sort_unstable();
        assert_eq!(whens, sorted);
    }

    /// Fix 10a: a schedule item planned into an OPEN visit (via a work
    /// item's `schedule_item_id`) skips its FIRST projected occurrence —
    /// that spend is the visit's — while later cycles still project. An
    /// item whose next cycle falls past the horizon projects nothing.
    #[tokio::test]
    async fn schedule_item_in_open_visit_skips_first_occurrence() {
        let db = test_db().await;
        let vid = vehicle::ActiveModel {
            name: Set("Car".into()),
            purchase_date: Set(Some(days_ago(730))),
            purchase_mileage: Set(Some(0)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        // Rate pinned at exactly 100 mi/day (as in the pinned scenario).
        for (age, miles) in [(100, 10_000), (0, 20_000)] {
            mileage_log::ActiveModel {
                vehicle_id: Set(vid),
                mileage: Set(miles),
                recorded_at: Set(days_ago(age)),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }

        // Overdue, cycle 50 days → 8 occurrences when unplanned.
        let oil = seed_item(&db, vid, "Oil change", Some(5_000), None, Some(2_000)).await;
        // Overdue, cycle 12 months → exactly 1 occurrence when unplanned.
        let belt = seed_item(&db, vid, "Timing belt check", None, Some(12), Some(5_000)).await;

        let baseline = forecast(&db, vid).await.unwrap();
        let count =
            |f: &BudgetForecast, label: &str| f.lines.iter().filter(|l| l.label == label).count();
        assert_eq!(count(&baseline, "Oil change"), 8);
        assert_eq!(count(&baseline, "Timing belt check"), 1);
        assert_eq!(baseline.total_cents, 8 * 2_000 + 5_000);

        // Plan both into one open visit.
        let mut ids = vec![];
        for (title, sched, est) in [("Oil", oil, 2_200), ("Belt", belt, 5_500)] {
            ids.push(
                work_item_svc::create(
                    &db,
                    vid,
                    NewWorkItem {
                        title: title.into(),
                        notes: None,
                        schedule_item_id: Some(sched),
                        research_finding_id: None,
                        incident_id: None,
                        build_id: None,
                        est_cost_cents: Some(est),
                        visit_id: None,
                    },
                )
                .await
                .unwrap()
                .id,
            );
        }
        crate::services::visit::create(
            &db,
            vid,
            NewVisit {
                planned_date: None,
                shop_name: None,
                shop_id: None,
                notes: None,
                work_item_ids: Some(ids),
            },
        )
        .await
        .unwrap();

        let f = forecast(&db, vid).await.unwrap();
        // Oil: first occurrence suppressed, later cycles (days 50..350)
        // still project. Belt: the only occurrence within the horizon is
        // the planned one → nothing projects.
        assert_eq!(count(&f, "Oil change"), 7, "lines: {:?}", f.lines);
        assert_eq!(count(&f, "Timing belt check"), 0, "lines: {:?}", f.lines);
        assert_eq!(f.projected_maintenance_cents, 7 * 2_000);
        assert_eq!(f.planned_visits_cents, 2_200 + 5_500);
        assert_eq!(f.planned_work_cents, 0);
        // No double count: the planned occurrences appear only as the
        // visit's estimate.
        assert_eq!(f.total_cents, 7 * 2_000 + 7_700);
    }

    /// Fix 9: `forecast_from` with precomputed reminders is `forecast` —
    /// same buckets, same lines.
    #[tokio::test]
    async fn forecast_from_matches_forecast() {
        let db = test_db().await;
        let vid = vehicle::ActiveModel {
            name: Set("Car".into()),
            purchase_date: Set(Some(days_ago(730))),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        seed_item(&db, vid, "Timing belt check", None, Some(12), Some(5_000)).await;

        let via_forecast = forecast(&db, vid).await.unwrap();
        let rems = crate::services::reminders::calculate_reminders(&db, vid)
            .await
            .unwrap();
        let via_from = forecast_from(&db, vid, &rems).await.unwrap();

        assert_eq!(
            via_from.projected_maintenance_cents,
            via_forecast.projected_maintenance_cents
        );
        assert_eq!(via_from.total_cents, via_forecast.total_cents);
        assert_eq!(via_from.lines.len(), via_forecast.lines.len());
        assert_eq!(via_from.total_cents, 5_000);
    }
}
