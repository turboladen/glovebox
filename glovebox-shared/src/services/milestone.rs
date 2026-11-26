//! The next milestone service ("the 70k service"), derived from the
//! reminders and never stored.

use chrono::NaiveDate;
use serde::Serialize;

use crate::services::reminders::{
    self, DueStatus, LastServiceInfo, ReminderStatus, RemindersResponse,
};

/// The distance between two milestones.
pub const MILESTONE_STEP_MILES: i32 = 10_000;

/// The milestone service the car is due for and the tracked items that fall
/// due by it.
#[derive(Debug, Serialize, Clone)]
pub struct Milestone {
    /// The last multiple of [`MILESTONE_STEP_MILES`] the car has passed while
    /// a factory item due by it is still not done, otherwise the first
    /// multiple above the estimated mileage.
    pub mark_miles: i32,
    /// The milestone's name, such as `70k service`.
    pub name: String,
    /// True when the car is past `mark_miles` and that milestone's service is
    /// still open.
    pub passed: bool,
    /// The day the car is expected to reach `mark_miles` at its average daily
    /// mileage, or `None` when the mark is passed or the car has no mileage
    /// rate.
    pub reached_around: Option<NaiveDate>,
    /// Items the manufacturer recommends that are due by the milestone: due
    /// or overdue now, due at or below `mark_miles`, or due on or before
    /// `reached_around`.
    pub factory_items: Vec<MilestoneItem>,
    /// The other tracked items due by the milestone, by the same rule.
    pub other_items: Vec<MilestoneItem>,
    /// Factory items in the manufacturer's package at this mark (their
    /// mileage interval divides `mark_miles`) that are not due by it because
    /// a service did them recently.
    pub done_recently: Vec<MilestoneItem>,
}

impl Milestone {
    /// How many items are due by the milestone, factory and other.
    #[must_use]
    pub fn due_count(&self) -> usize {
        self.factory_items.len() + self.other_items.len()
    }
}

/// A tracked item of the milestone.
#[derive(Debug, Serialize, Clone)]
pub struct MilestoneItem {
    pub schedule_item_id: i32,
    pub name: String,
    pub status: DueStatus,
    pub due_at_miles: Option<i32>,
    pub due_at_date: Option<NaiveDate>,
    /// The service that last did the item, if any.
    pub last_service: Option<LastServiceInfo>,
}

impl From<&ReminderStatus> for MilestoneItem {
    fn from(r: &ReminderStatus) -> Self {
        Self {
            schedule_item_id: r.schedule_item.id,
            name: r.schedule_item.name.clone(),
            status: r.status,
            due_at_miles: r.due_at_miles,
            due_at_date: r.due_at_date,
            last_service: r.last_service.clone(),
        }
    }
}

/// The name of the milestone at `mark_miles`, such as `70k service`.
#[must_use]
pub fn label(mark_miles: i32) -> String {
    format!("{}k service", mark_miles / 1000)
}

/// Says where the milestone falls: `at 70,000 mi, around 2029-08-03`,
/// `at 70,000 mi` with no mileage rate, or `at 60,000 mi, already passed`.
#[must_use]
pub fn when(m: &Milestone) -> String {
    let at = format!("at {} mi", reminders::miles_label(m.mark_miles));
    match (m.passed, m.reached_around) {
        (true, _) => format!("{at}, already passed"),
        (false, Some(day)) => format!("{at}, around {}", super::date_label(day)),
        (false, None) => at,
    }
}

/// Says when a service last did an item: `61,000 mi on 2026-10-01`, or the
/// date alone when the service has no odometer reading.
#[must_use]
pub fn last_done(l: &LastServiceInfo) -> String {
    match l.odometer {
        Some(miles) => reminders::reading_text(miles, l.date),
        None => super::date_label(l.date),
    }
}

/// The milestone the vehicle the reminders describe is due for, or `None` when
/// no tracked item is due by it and none was done recently, or the mileage is
/// too large for a mark.
///
/// An owner often has a milestone's service done after the car passes the
/// mark, so the last mark passed stays the milestone while a factory item due
/// at or before it is not done.
#[must_use]
pub fn from_reminders(rems: &RemindersResponse, today: NaiveDate) -> Option<Milestone> {
    let passed_mark = last_mark(rems.estimated_mileage).filter(|mark| {
        rems.reminders.iter().any(|r| {
            r.item.is_factory_recommended == Some(true)
                && r.due_at_miles.is_some_and(|due| due <= *mark)
        })
    });
    let (mark_miles, passed, reached_around) = if let Some(mark) = passed_mark {
        (mark, true, None)
    } else {
        let mark = next_mark(rems.estimated_mileage)?;
        let reached = reminders::days_to_drive(mark - rems.estimated_mileage, rems.avg_daily_miles)
            .and_then(|days| today.checked_add_signed(chrono::Duration::days(days)));
        (mark, false, reached)
    };
    let mut factory_items = Vec::new();
    let mut other_items = Vec::new();
    let mut done_recently = Vec::new();
    for r in &rems.reminders {
        let factory = r.item.is_factory_recommended == Some(true);
        let by_miles = r.due_at_miles.is_some_and(|m| m <= mark_miles);
        let by_date = matches!((r.due_at_date, reached_around), (Some(d), Some(on)) if d <= on);
        if r.status.is_due() || by_miles || by_date {
            if factory {
                factory_items.push(r.into());
            } else {
                other_items.push(r.into());
            }
        } else if factory
            && r.last_service.is_some()
            && r.item
                .interval_miles
                .is_some_and(|i| i > 0 && mark_miles % i == 0)
        {
            done_recently.push(r.into());
        }
    }
    if factory_items.is_empty() && other_items.is_empty() && done_recently.is_empty() {
        return None;
    }
    Some(Milestone {
        mark_miles,
        name: label(mark_miles),
        passed,
        reached_around,
        factory_items,
        other_items,
        done_recently,
    })
}

/// The last multiple of [`MILESTONE_STEP_MILES`] at or below `miles`, or
/// `None` below the first one.
fn last_mark(miles: i32) -> Option<i32> {
    let mark = miles / MILESTONE_STEP_MILES * MILESTONE_STEP_MILES;
    (mark > 0).then_some(mark)
}

/// The first multiple of [`MILESTONE_STEP_MILES`] above `miles`, or `None`
/// past the largest mileage a reading can hold.
fn next_mark(miles: i32) -> Option<i32> {
    (Ord::max(miles, 0) / MILESTONE_STEP_MILES + 1).checked_mul(MILESTONE_STEP_MILES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::maintenance_schedule_item,
        services::reminders::{Baseline, ScheduleItemSummary},
    };

    fn today() -> NaiveDate {
        crate::test_support::date("2026-11-13")
    }

    struct Item {
        id: i32,
        interval_miles: Option<i32>,
        factory: Option<bool>,
        due_at_miles: Option<i32>,
        due_at_date: Option<&'static str>,
        last_odometer: Option<i32>,
    }

    impl Item {
        fn due(id: i32, interval_miles: i32, factory: Option<bool>, due_at_miles: i32) -> Self {
            Self {
                id,
                interval_miles: Some(interval_miles),
                factory,
                due_at_miles: Some(due_at_miles),
                due_at_date: None,
                last_odometer: None,
            }
        }

        fn done_at(mut self, odometer: i32) -> Self {
            self.last_odometer = Some(odometer);
            self
        }

        fn by_date(id: i32, due_at_date: &'static str) -> Self {
            Self {
                id,
                interval_miles: None,
                factory: Some(true),
                due_at_miles: None,
                due_at_date: Some(due_at_date),
                last_odometer: None,
            }
        }

        fn reminder(self) -> ReminderStatus {
            let now = chrono::Utc::now();
            ReminderStatus {
                schedule_item: ScheduleItemSummary {
                    id: self.id,
                    name: format!("Item {}", self.id),
                },
                status: DueStatus::NotDue,
                baseline: if self.last_odometer.is_some() {
                    Baseline::LastService
                } else {
                    Baseline::Purchase
                },
                last_service: self.last_odometer.map(|odometer| LastServiceInfo {
                    id: 100 + self.id,
                    date: crate::test_support::date("2026-10-01"),
                    odometer: Some(odometer),
                }),
                due_at_miles: self.due_at_miles,
                due_at_date: self.due_at_date.map(crate::test_support::date),
                miles_remaining: None,
                days_remaining: None,
                trigger: None,
                item: maintenance_schedule_item::Model {
                    id: self.id,
                    platform_id: None,
                    model_template_id: None,
                    vehicle_id: None,
                    overrides_item_id: None,
                    name: format!("Item {}", self.id),
                    description: None,
                    interval_miles: self.interval_miles,
                    interval_months: None,
                    labor_categories: None,
                    created_at: now,
                    updated_at: now,
                    warning_miles: None,
                    warning_days: None,
                    enabled: true,
                    source: None,
                    notes: None,
                    is_factory_recommended: self.factory,
                    est_cost_cents: None,
                },
            }
        }
    }

    fn rems(estimated: i32, avg_daily: f64, items: Vec<Item>) -> RemindersResponse {
        RemindersResponse {
            vehicle_id: 1,
            estimated_mileage: estimated,
            mileage_is_estimate: true,
            mileage_as_of: today(),
            avg_daily_miles: avg_daily,
            warranty: None,
            reminders: items.into_iter().map(Item::reminder).collect(),
            bundle_suggestions: Vec::new(),
        }
    }

    fn ids(items: &[MilestoneItem]) -> Vec<i32> {
        items.iter().map(|i| i.schedule_item_id).collect()
    }

    #[test]
    fn the_mark_is_the_next_multiple_of_ten_thousand_above_the_mileage() {
        assert_eq!(next_mark(59_999), Some(60_000));
        assert_eq!(next_mark(60_000), Some(70_000));
        assert_eq!(next_mark(60_060), Some(70_000));
        assert_eq!(next_mark(0), Some(10_000));
        assert_eq!(next_mark(i32::MAX), None);
        assert_eq!(label(70_000), "70k service");
        assert_eq!(label(100_000), "100k service");
    }

    #[test]
    fn an_item_due_by_the_mark_is_split_by_the_factory_flag() {
        let r = rems(
            60_060,
            0.0,
            vec![
                Item::due(1, 10_000, Some(true), 69_900).done_at(59_900),
                Item::due(2, 5_000, Some(true), 61_000),
                Item::due(3, 60_000, Some(false), 65_000),
                Item::due(4, 60_000, None, 66_000),
                Item::due(5, 20_000, Some(true), 72_000),
            ],
        );
        let m = from_reminders(&r, today()).expect("items are due by 70k");
        assert_eq!(m.mark_miles, 70_000);
        assert_eq!(m.name, "70k service");
        assert_eq!(when(&m), "at 70,000 mi");
        assert_eq!(ids(&m.factory_items), [1, 2]);
        assert_eq!(ids(&m.other_items), [3, 4]);
        assert_eq!(m.due_count(), 4);
    }

    #[test]
    fn an_item_due_by_date_counts_when_the_date_falls_before_the_mark_is_reached() {
        // 9,940 miles at 10 a day is 994 days away.
        let r = rems(
            60_060,
            10.0,
            vec![
                Item::by_date(1, "2027-06-01"),
                Item::by_date(2, "2030-01-01"),
            ],
        );
        let m = from_reminders(&r, today()).expect("one item is due by date");
        assert_eq!(
            m.reached_around,
            Some(crate::test_support::date("2029-08-03"))
        );
        assert_eq!(ids(&m.factory_items), [1]);
        assert_eq!(when(&m), "at 70,000 mi, around 2029-08-03");
    }

    #[test]
    fn a_passed_mark_stays_the_milestone_while_an_item_due_by_it_is_not_done() {
        // At 64,500 with the oil due at 60,000 still not done.
        let mut oil = Item::due(1, 10_000, Some(true), 60_000)
            .done_at(50_000)
            .reminder();
        oil.status = DueStatus::Overdue;
        let r = RemindersResponse {
            reminders: vec![oil],
            ..rems(64_500, 10.0, Vec::new())
        };
        let m = from_reminders(&r, today()).expect("the 60k service is open");
        assert_eq!(m.name, "60k service");
        assert!(m.passed);
        assert_eq!(m.reached_around, None);
        assert_eq!(when(&m), "at 60,000 mi, already passed");
        assert_eq!(ids(&m.factory_items), [1]);
    }

    #[test]
    fn a_service_after_the_mark_moves_the_milestone_on_and_counts_from_its_reading() {
        // The 60k visit happened at 64,000: the oil is next due at 74,000 and
        // the tire rotation at 69,000.
        let r = rems(
            64_500,
            0.0,
            vec![
                Item::due(1, 10_000, Some(true), 74_000).done_at(64_000),
                Item::due(2, 5_000, Some(true), 69_000).done_at(64_000),
            ],
        );
        let m = from_reminders(&r, today()).expect("the tire rotation is due by 70k");
        assert_eq!(m.name, "70k service");
        assert!(!m.passed);
        assert_eq!(ids(&m.factory_items), [2]);
        assert_eq!(ids(&m.done_recently), [1]);
    }

    #[test]
    fn an_item_due_just_past_the_mark_does_not_hold_it_open() {
        // The tire rotation is due soon at 65,000, after the 60k mark.
        let mut rotation = Item::due(1, 5_000, Some(true), 65_000)
            .done_at(60_000)
            .reminder();
        rotation.status = DueStatus::DueSoon;
        let r = RemindersResponse {
            reminders: vec![rotation],
            ..rems(64_500, 0.0, Vec::new())
        };
        let m = from_reminders(&r, today()).expect("the rotation is due now");
        assert_eq!(m.name, "70k service");
        assert_eq!(ids(&m.factory_items), [1]);
    }

    #[tokio::test]
    async fn an_item_done_past_its_mark_is_next_due_one_interval_after_that_service() {
        use crate::{
            entities::service_record::Performer,
            inputs::{schedule::NewScheduleItem, service_record::NewServiceRecord},
            services::{schedule, service_record},
            test_support::{VehicleFixture, test_db},
        };
        let db = test_db().await;
        let v = VehicleFixture::new()
            .purchase_date("2024-01-01")
            .purchase_mileage(40_000)
            .insert_id(&db)
            .await;
        let oil = schedule::create_for_vehicle(
            &db,
            v,
            NewScheduleItem {
                name: "Oil".into(),
                interval_miles: Some(10_000),
                interval_months: Some(12),
                is_factory_recommended: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        service_record::create(
            &db,
            v,
            NewServiceRecord {
                service_date: crate::test_support::date("2026-11-01"),
                performed_by: Some(Performer::Owner),
                mileage: Some(64_000),
                schedule_item_ids: Some(vec![oil]),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let rems = reminders::calculate_reminders_on(&db, v, today())
            .await
            .unwrap();
        let reminder = &rems.reminders[0];
        assert_eq!(reminder.due_at_miles, Some(74_000));
        assert_eq!(
            reminder.due_at_date,
            Some(crate::test_support::date("2027-11-01"))
        );
        let m = from_reminders(&rems, today()).expect("the oil is in the 70k package");
        assert_eq!(m.name, "70k service");
        assert_eq!(ids(&m.done_recently), [oil]);
    }

    #[test]
    fn with_no_mileage_rate_a_later_date_does_not_count() {
        let r = rems(60_060, 0.0, vec![Item::by_date(1, "2027-06-01")]);
        assert!(from_reminders(&r, today()).is_none());
    }

    #[test]
    fn an_item_due_now_counts_with_no_mileage_rate_and_is_never_done_recently() {
        // Oil done at 61,000 and overdue by date: its miles fall past the
        // mark and its interval divides it, but it is due now.
        let mut oil = Item::due(1, 10_000, Some(true), 71_000)
            .done_at(61_000)
            .reminder();
        oil.status = DueStatus::Overdue;
        let mut wipers = Item::by_date(2, "2025-12-15").reminder();
        wipers.status = DueStatus::DueSoon;
        let r = RemindersResponse {
            reminders: vec![oil, wipers],
            ..rems(60_060, 0.0, Vec::new())
        };
        let m = from_reminders(&r, today()).expect("both items are due now");
        assert_eq!(ids(&m.factory_items), [1, 2]);
        crate::test_support::assert_empty(&m.done_recently);
    }

    #[test]
    fn a_factory_item_whose_interval_divides_the_mark_reads_done_recently() {
        let r = rems(
            60_060,
            0.0,
            vec![
                // Oil at 10k, done at 61,000: next due past the mark.
                Item::due(1, 10_000, Some(true), 71_000).done_at(61_000),
                // Air filter at 20k does not divide 70k.
                Item::due(2, 20_000, Some(true), 78_000).done_at(58_000),
                // A community item at 10k is not in the factory package.
                Item::due(3, 10_000, Some(false), 71_000).done_at(61_000),
                // A factory item with no service on record is not done.
                Item::due(4, 5_000, Some(true), 75_000),
            ],
        );
        let m = from_reminders(&r, today()).expect("oil was done recently");
        assert_eq!(ids(&m.done_recently), [1]);
        let last = m.done_recently[0].last_service.as_ref().unwrap();
        assert_eq!(last_done(last), "61,000 mi on 2026-10-01");
        assert_eq!(m.due_count(), 0);
    }

    #[test]
    fn nothing_due_and_nothing_done_is_no_milestone() {
        let r = rems(60_060, 0.0, vec![Item::due(1, 30_000, Some(true), 90_000)]);
        assert!(from_reminders(&r, today()).is_none());
        assert!(from_reminders(&rems(60_060, 0.0, Vec::new()), today()).is_none());
    }

    #[test]
    fn a_rate_too_slow_to_reach_the_mark_gives_no_date() {
        let r = rems(60_060, 0.001, vec![Item::due(1, 5_000, Some(true), 65_000)]);
        let m = from_reminders(&r, today()).expect("the item is due by miles");
        assert_eq!(m.reached_around, None);
    }
}
