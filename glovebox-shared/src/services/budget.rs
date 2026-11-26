//! 12-month spend forecast: schedule intervals × the vehicle's actual
//! mileage rate × each item's `est_cost_cents`, plus the planned-work
//! backlog. Money is integer cents throughout; only the day arithmetic
//! touches floats (the mileage rate).
//!
//! Baselines, due dates, and the mileage rate all come from
//! [`calculate_reminders`](crate::services::reminders::calculate_reminders):
//! the rate machinery lives in `reminders::estimate_mileage`, which the
//! reminders response surfaces as `avg_daily_miles`.

use std::collections::HashSet;

use serde::Serialize;

use crate::services::{reminders::RemindersResponse, work_item::PlannedItem};

/// Forecast horizon: days `0..HORIZON_DAYS` from today (an occurrence
/// landing exactly on day 365 is next year's spend).
const HORIZON_DAYS: i64 = 365;
const HORIZON_MONTHS: u32 = 12;

/// The two-bucket forecast model. Each dollar appears in exactly one
/// bucket: schedule projections (minus occurrences already on the planned
/// backlog) and the planned-work backlog.
#[derive(Debug, Serialize)]
pub struct BudgetForecast {
    pub horizon_months: u32,
    /// Σ projected schedule-item occurrences within the horizon. A
    /// schedule item that appears (via a work item's `schedule_item_id`) on
    /// the planned backlog skips its FIRST projected occurrence: that spend
    /// is already counted in `planned_work_cents` ("already planned").
    /// Later cycles still project.
    pub projected_maintenance_cents: i64,
    /// Σ planned work items carrying `est_cost_cents`: the backlog the user
    /// intends to do.
    pub planned_work_cents: i64,
    /// `projected_maintenance_cents + planned_work_cents`.
    pub total_cents: i64,
    pub lines: Vec<ForecastLine>,
    /// Schedule items the forecast expects to come due in the window that
    /// have no `est_cost_cents`. Their cost is missing from every figure
    /// above, so `total_cents` is a floor while this list is not empty.
    pub unestimated_items: Vec<UnestimatedItem>,
    /// The length of `unestimated_items`.
    pub unestimated_item_count: usize,
    /// Planned work items with no `est_cost_cents`, also missing from
    /// `total_cents`.
    pub unestimated_planned_work: Vec<UnestimatedWork>,
    /// The length of `unestimated_planned_work`.
    pub unestimated_planned_work_count: usize,
}

impl BudgetForecast {
    /// True when the forecast has nothing to say: no cost in either bucket
    /// and nothing left unpriced.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total_cents == 0 && self.unestimated_count() == 0
    }

    /// Counts the schedule items and planned jobs whose cost is missing from
    /// `total_cents`. The total is a floor while this is above zero.
    #[must_use]
    pub fn unestimated_count(&self) -> usize {
        self.unestimated_item_count + self.unestimated_planned_work_count
    }

    /// Says how many items the total leaves out, such as
    /// `3 items have no estimate`, or `None` when every item is priced.
    #[must_use]
    pub fn unestimated_note(&self) -> Option<String> {
        match self.unestimated_count() {
            0 => None,
            1 => Some("1 item has no estimate".to_owned()),
            n => Some(format!("{n} items have no estimate")),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct UnestimatedItem {
    pub id: i32,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct UnestimatedWork {
    pub id: i32,
    pub title: String,
}

/// One projected occurrence of a schedule item.
#[derive(Debug, Serialize)]
pub struct ForecastLine {
    pub label: String,
    /// Projected date (today for overdue items).
    pub when: chrono::NaiveDate,
    pub est_cents: i64,
}

/// The summed estimates of the planned backlog, and the items with none.
fn planned_work_cost(backlog: &[PlannedItem]) -> (i64, Vec<UnestimatedWork>) {
    let cents = backlog
        .iter()
        .filter_map(|p| p.item.est_cost_cents)
        .map(i64::from)
        .sum();
    // The backlog arrives most urgent revisit first, and this list is in the
    // order the jobs were planned.
    let mut unestimated: Vec<UnestimatedWork> = backlog
        .iter()
        .filter(|p| p.item.est_cost_cents.is_none())
        .map(|p| UnestimatedWork {
            id: p.item.id,
            title: p.item.title.clone(),
        })
        .collect();
    unestimated.sort_by_key(|work| work.id);
    (cents, unestimated)
}

/// Projects the vehicle's 12 months of maintenance spend from `today`, from reminders
/// the caller has already computed with
/// [`calculate_reminders`](crate::services::reminders::calculate_reminders)
/// and from `backlog`, the vehicle's planned jobs as
/// [`planned_work`](crate::services::upcoming::planned_work) lists them, where
/// each job's schedule item is the one the vehicle tracks.
///
/// For each enabled resolved schedule item carrying `est_cost_cents`:
/// occurrences repeat at the item's interval — `interval_months`, and/or
/// `interval_miles` divided by the vehicle's actual mileage rate (whichever
/// cycle is shorter). Overdue items count as one occurrence now. Items with
/// no projectable due date (e.g. mileage-only interval on a vehicle with no
/// mileage history) are skipped unless overdue.
pub(crate) fn forecast_from(
    rems: &RemindersResponse,
    backlog: &[PlannedItem],
    today: chrono::NaiveDate,
) -> BudgetForecast {
    // The planned backlog: its estimates are the second bucket, and its
    // schedule links suppress first occurrences ("already planned").
    let planned_schedule_items: HashSet<i32> = backlog
        .iter()
        .filter_map(|p| p.item.schedule_item_id)
        .collect();

    let avg_daily = rems.avg_daily_miles;
    let miles_to_days = |miles: i32| super::reminders::days_to_drive(miles, avg_daily);

    let mut lines: Vec<ForecastLine> = Vec::new();
    let mut unestimated_items: Vec<UnestimatedItem> = Vec::new();
    for r in &rems.reminders {
        let item = &r.item;

        // Days until the first occurrence; an item at or past its due point
        // counts as one now, whatever the mileage rate.
        let at_due_point =
            r.miles_remaining.is_some_and(|m| m <= 0) || r.days_remaining.is_some_and(|d| d <= 0);
        let first_due: Option<i64> = if at_due_point {
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

        let mut day = Ord::max(first, 0);
        // Already planned: the first occurrence is counted in
        // `planned_work_cents`; only later cycles project here.
        if planned_schedule_items.contains(&item.id) {
            let Some(cycle) = cycle_days else { continue };
            day += cycle;
            if day >= HORIZON_DAYS {
                continue;
            }
        }
        // An item expected to come due but with no estimate is named instead
        // of priced.
        let Some(cost) = item.est_cost_cents.map(i64::from) else {
            unestimated_items.push(UnestimatedItem {
                id: item.id,
                name: item.name.clone(),
            });
            continue;
        };
        loop {
            lines.push(ForecastLine {
                label: item.name.clone(),
                when: today + chrono::Duration::days(day),
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

    let (planned_work_cents, unestimated_planned_work) = planned_work_cost(backlog);

    BudgetForecast {
        horizon_months: HORIZON_MONTHS,
        projected_maintenance_cents,
        planned_work_cents,
        total_cents: projected_maintenance_cents + planned_work_cents,
        lines,
        unestimated_item_count: unestimated_items.len(),
        unestimated_items,
        unestimated_planned_work_count: unestimated_planned_work.len(),
        unestimated_planned_work,
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::*;

    use super::*;
    use crate::{
        entities::{
            maintenance_schedule_item, mileage_log, service_record, service_schedule_link,
            work_item::JobStatus,
        },
        error::{DomainError, DomainResult},
        inputs::work_item::{NewWorkItem, UpdateWorkItem},
        services::{reminders, work_item as work_item_svc},
        test_support::{VehicleFixture, days_ago, test_db},
    };

    async fn forecast(db: &DatabaseConnection, vehicle_id: i32) -> DomainResult<BudgetForecast> {
        let today = crate::services::today();
        let rems = reminders::calculate_reminders(db, vehicle_id).await?;
        let backlog = crate::services::upcoming::planned_work(db, vehicle_id, today).await?;
        Ok(forecast_from(&rems, &backlog, today))
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

    // The home page prints these two readings of a forecast.
    #[tokio::test]
    async fn a_forecast_says_how_many_items_it_could_not_price() {
        let db = test_db().await;
        let vid = VehicleFixture::new()
            .purchase_date(&days_ago(730))
            .purchase_mileage(0)
            .insert_id(&db)
            .await;
        let nothing = forecast(&db, vid).await.unwrap();
        assert!(nothing.is_empty());
        assert_eq!(nothing.unestimated_note(), None);

        let plan = |title: &str, est_cost_cents| NewWorkItem {
            title: title.into(),
            est_cost_cents,
            ..Default::default()
        };
        work_item_svc::create(&db, vid, plan("Priced", Some(7_500)))
            .await
            .unwrap();
        let priced = forecast(&db, vid).await.unwrap();
        assert!(!priced.is_empty());
        assert_eq!(priced.unestimated_note(), None);

        work_item_svc::create(&db, vid, plan("Unpriced", None))
            .await
            .unwrap();
        let one = forecast(&db, vid).await.unwrap();
        assert_eq!(
            one.unestimated_note().as_deref(),
            Some("1 item has no estimate")
        );

        // An overdue schedule item with no estimate counts beside the job.
        seed_item(&db, vid, "Timing belt check", None, Some(12), None).await;
        let two = forecast(&db, vid).await.unwrap();
        assert_eq!(two.unestimated_count(), 2);
        assert_eq!(
            two.unestimated_note().as_deref(),
            Some("2 items have no estimate")
        );
        assert_eq!(two.total_cents, 7_500);
    }

    // The planned jobs reach the forecast most urgent first, and the list of
    // unpriced ones stays in the order they were planned.
    #[tokio::test]
    async fn unpriced_planned_work_is_listed_in_id_order() {
        let db = test_db().await;
        let vid = VehicleFixture::new().insert_id(&db).await;
        let first = work_item_svc::create(
            &db,
            vid,
            NewWorkItem {
                title: "No revisit".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let second = work_item_svc::create(
            &db,
            vid,
            NewWorkItem {
                title: "Overdue revisit".into(),
                revisit_by_date: Some(days_ago(10)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let today = crate::services::today();
        let backlog = crate::services::upcoming::planned_work(&db, vid, today)
            .await
            .unwrap();
        let by_urgency: Vec<i32> = backlog.iter().map(|p| p.item.id).collect();
        assert_eq!(by_urgency, [second.id, first.id]);

        let f = forecast(&db, vid).await.unwrap();

        let listed: Vec<i32> = f.unestimated_planned_work.iter().map(|w| w.id).collect();
        assert_eq!(listed, [first.id, second.id]);
    }

    // A vehicle whose only forecast content is unpriced still has something
    // to show: the count of what the total leaves out.
    #[tokio::test]
    async fn a_forecast_of_only_unpriced_items_is_not_empty() {
        let db = test_db().await;
        let vid = VehicleFixture::new().insert_id(&db).await;
        work_item_svc::create(
            &db,
            vid,
            NewWorkItem {
                title: "Unpriced".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let f = forecast(&db, vid).await.unwrap();

        assert_eq!(f.total_cents, 0);
        assert!(!f.is_empty());
    }

    #[tokio::test]
    async fn empty_schedule_forecasts_zero() {
        let db = test_db().await;
        let vid = VehicleFixture::new().insert_id(&db).await;

        let f = forecast(&db, vid).await.unwrap();
        assert_eq!(f.horizon_months, 12);
        assert_eq!(f.projected_maintenance_cents, 0);
        assert_eq!(f.planned_work_cents, 0);
        assert_eq!(f.total_cents, 0);
        assert!(f.lines.is_empty());
    }

    // The pinned arithmetic scenario, seeded relative to today:
    // - "Annual inspection" ($100, 12 months, serviced ~6 months ago) occurs once.
    // - "Timing belt check" ($50, 12 months, never serviced) is overdue and
    //   occurs once, because the next one lands past the horizon.
    // - "Oil change" ($20, 5000 miles, 100 mi/day, overdue) cycles every 50
    //   days and occurs 8 times, at days 0, 50, ..., 350.
    // - A planned item with a $75 estimate counts toward `planned_work_cents`;
    //   a dropped item with a $999 estimate does not.
    #[tokio::test]
    async fn pinned_interval_and_rate_scenario() {
        let db = test_db().await;
        let vid = VehicleFixture::new()
            .purchase_date(&days_ago(730))
            .purchase_mileage(0)
            .insert_id(&db)
            .await;

        // Pin the mileage rate: 10_000 miles over 100 days, latest today.
        for (age, miles) in [(100, 10_000), (0, 20_000)] {
            mileage_log::ActiveModel {
                vehicle_id: Set(vid),
                mileage: Set(miles),
                recorded_at: Set(days_ago(age).into()),
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

        // One planned item ($75) and one dropped item ($999).
        work_item_svc::create(
            &db,
            vid,
            NewWorkItem {
                title: "Wipers".into(),
                est_cost_cents: Some(7_500),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let dead_item = work_item_svc::create(
            &db,
            vid,
            NewWorkItem {
                title: "Never happening".into(),
                est_cost_cents: Some(99_900),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        work_item_svc::update(
            &db,
            vid,
            dead_item.id,
            UpdateWorkItem {
                status: Some(JobStatus::Dropped),
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
        assert_eq!(f.planned_work_cents, 7_500, "dropped item excluded");
        assert_eq!(f.total_cents, 31_000 + 7_500);

        // Overdue occurrences land today; lines are date-ordered.
        assert_eq!(f.lines[0].when, days_ago(0));
        let whens: Vec<chrono::NaiveDate> = f.lines.iter().map(|l| l.when).collect();
        let mut sorted = whens.clone();
        sorted.sort_unstable();
        assert_eq!(whens, sorted);
    }

    // A schedule-linked work item on the planned backlog skips that
    // schedule item's FIRST projected occurrence, so the spend is not
    // counted in both `planned_work` and `projected_maintenance`. Later
    // cycles still project; an item whose only in-horizon occurrence is the
    // planned one projects nothing.
    #[tokio::test]
    async fn schedule_linked_backlog_item_skips_first_occurrence() {
        let db = test_db().await;
        let vid = VehicleFixture::new()
            .purchase_date(&days_ago(730))
            .purchase_mileage(0)
            .insert_id(&db)
            .await;
        // Rate pinned at exactly 100 mi/day (as in the pinned scenario).
        for (age, miles) in [(100, 10_000), (0, 20_000)] {
            mileage_log::ActiveModel {
                vehicle_id: Set(vid),
                mileage: Set(miles),
                recorded_at: Set(days_ago(age).into()),
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

        // Plan both onto the to-do list.
        for (title, sched, est) in [("Oil", oil, 2_200), ("Belt", belt, 5_500)] {
            work_item_svc::create(
                &db,
                vid,
                NewWorkItem {
                    title: title.into(),
                    schedule_item_id: Some(sched),
                    est_cost_cents: Some(est),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }

        let f = forecast(&db, vid).await.unwrap();
        // Oil: first occurrence suppressed, later cycles (days 50..350)
        // still project. Belt: its only in-horizon occurrence is the
        // planned one → nothing projects.
        assert_eq!(count(&f, "Oil change"), 7, "lines: {:?}", f.lines);
        assert_eq!(count(&f, "Timing belt check"), 0, "lines: {:?}", f.lines);
        assert_eq!(f.projected_maintenance_cents, 7 * 2_000);
        assert_eq!(f.planned_work_cents, 2_200 + 5_500);
        // No double count: the planned occurrences appear only as the
        // backlog estimates.
        assert_eq!(f.total_cents, 7 * 2_000 + 7_700);
    }

    // A first session of filing invoices logs two readings a couple of days
    // apart. The rate then comes from the purchase, so a mileage-interval
    // item still projects into the 12-month forecast.
    #[tokio::test]
    async fn mileage_items_forecast_when_recent_readings_are_close_together() {
        let db = test_db().await;
        let vid = VehicleFixture::new()
            .purchase_date("2017-03-05")
            .purchase_mileage(40)
            .insert_id(&db)
            .await;
        for age in [2, 0] {
            mileage_log::ActiveModel {
                vehicle_id: Set(vid),
                mileage: Set(64_000),
                recorded_at: Set(days_ago(age).into()),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        let item = seed_item(&db, vid, "Oil change", Some(10_000), None, Some(2_000)).await;
        let svc = service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set(days_ago(300)),
            mileage: Set(Some(60_000)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        service_schedule_link::ActiveModel {
            service_record_id: Set(svc.id),
            schedule_item_id: Set(item),
        }
        .insert(&db)
        .await
        .unwrap();

        let rems = reminders::calculate_reminders(&db, vid).await.unwrap();
        let f = forecast(&db, vid).await.unwrap();
        assert!(!f.lines.is_empty(), "rate {}", rems.avg_daily_miles);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a day count in the thousands"
        )]
        let first_in_days = (6_000.0 / rems.avg_daily_miles).ceil() as i64;
        assert_eq!(
            f.lines[0].when,
            crate::services::today() + chrono::Duration::days(first_in_days)
        );
    }

    fn plan(title: &str, est_cost_cents: Option<i32>) -> NewWorkItem {
        NewWorkItem {
            title: title.into(),
            est_cost_cents,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn an_item_at_its_due_mileage_is_forecast_without_a_mileage_rate() {
        let db = test_db().await;
        let vid = VehicleFixture::new().insert_id(&db).await;
        // One reading and no purchase data give a rate of zero, and the item
        // is due at exactly that reading.
        mileage_log::ActiveModel {
            vehicle_id: Set(vid),
            mileage: Set(5_000),
            recorded_at: Set(days_ago(0).into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        seed_item(&db, vid, "Oil", Some(5_000), None, Some(2_000)).await;
        seed_item(&db, vid, "Unpriced oil", Some(5_000), None, None).await;

        let rems = reminders::calculate_reminders(&db, vid).await.unwrap();
        assert_eq!(rems.reminders[0].miles_remaining, Some(0));
        let f = forecast(&db, vid).await.unwrap();
        assert_eq!(f.projected_maintenance_cents, 2_000);
        assert_eq!(f.lines.len(), 1);
        assert_eq!(f.unestimated_item_count, 1);
    }

    #[tokio::test]
    async fn the_forecast_names_what_it_leaves_out() {
        let db = test_db().await;
        let vid = VehicleFixture::new()
            .purchase_date(&days_ago(730))
            .insert_id(&db)
            .await;
        seed_item(&db, vid, "Priced", None, Some(12), Some(10_000)).await;
        let unpriced = seed_item(&db, vid, "Unpriced", None, Some(12), None).await;
        // Mileage-only with no mileage history: nothing projects, so it is not
        // an item the forecast expects to come due.
        seed_item(&db, vid, "Unknowable", Some(5_000), None, None).await;
        let far = seed_item(&db, vid, "Far off", None, Some(120), None).await;
        let far_svc = service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set(days_ago(10)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        service_schedule_link::ActiveModel {
            service_record_id: Set(far_svc.id),
            schedule_item_id: Set(far),
        }
        .insert(&db)
        .await
        .unwrap();
        work_item_svc::create(&db, vid, plan("Wipers", Some(7_500)))
            .await
            .unwrap();
        let blank = work_item_svc::create(&db, vid, plan("Alignment", None))
            .await
            .unwrap();

        let f = forecast(&db, vid).await.unwrap();
        assert_eq!(f.total_cents, 17_500);
        assert_eq!(f.unestimated_item_count, 1);
        assert_eq!(f.unestimated_items.len(), 1);
        assert_eq!(f.unestimated_items[0].id, unpriced);
        assert_eq!(f.unestimated_items[0].name, "Unpriced");
        assert_eq!(f.unestimated_planned_work_count, 1);
        assert_eq!(f.unestimated_planned_work[0].id, blank.id);
        assert_eq!(f.unestimated_planned_work[0].title, "Alignment");
    }
}
