use chrono::{Datelike, NaiveDate};
use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{maintenance_schedule_item, mileage_log, service_record, vehicle},
    error::{DomainError, DomainResult},
    services::schedule::{DEFAULT_WARNING_DAYS, DEFAULT_WARNING_MILES},
};

crate::vocab::vocabulary! {
    /// How close a schedule item or a planned job is to due.
    // A reminder is due soon inside its schedule item's warning miles or
    // days, and a planned job inside the fixed 30 days or 1000 miles that
    // `work_item::revisit_status` uses. Both judge through `DueStatus::judge`.
    // No column stores these texts.
    pub enum DueStatus {
        Overdue => "overdue",
        DueSoon => "due_soon",
        NotDue => "ok",
    }
}

impl DueStatus {
    /// Judges one reading: a negative `remaining` is overdue, one within
    /// `warning` (zero always included) is due soon, and any other is ok.
    pub(crate) fn judge(remaining: i64, warning: i64) -> Self {
        if remaining < 0 {
            Self::Overdue
        } else if remaining <= Ord::max(warning, 0) {
            Self::DueSoon
        } else {
            Self::NotDue
        }
    }

    /// Returns 0 for overdue, 1 for due soon and 2 for ok, so an ascending
    /// sort puts the most urgent first.
    #[must_use]
    pub const fn urgency_rank(self) -> u8 {
        match self {
            Self::Overdue => 0,
            Self::DueSoon => 1,
            Self::NotDue => 2,
        }
    }

    /// The most urgent of `statuses`, or ok when there are none.
    pub fn most_urgent(statuses: impl IntoIterator<Item = Self>) -> Self {
        statuses
            .into_iter()
            .min_by_key(|s| s.urgency_rank())
            .unwrap_or(Self::NotDue)
    }

    /// Whether the status is overdue or due soon.
    #[must_use]
    pub const fn is_due(self) -> bool {
        !matches!(self, Self::NotDue)
    }
}

crate::vocab::vocabulary! {
    /// Which reading of a reminder drives its status.
    // No column stores these texts.
    pub enum Trigger {
        Both => "both",
        Mileage => "mileage",
        Time => "time",
    }
}

impl Trigger {
    /// Names the reading whose status equals the more urgent of the two, or
    /// both when they match. An item that is not due has no trigger.
    pub(crate) fn of(miles: Option<DueStatus>, time: Option<DueStatus>) -> Option<Self> {
        let overall = DueStatus::most_urgent(miles.into_iter().chain(time));
        if !overall.is_due() {
            return None;
        }
        match (miles == Some(overall), time == Some(overall)) {
            (true, true) => Some(Self::Both),
            (true, false) => Some(Self::Mileage),
            _ => Some(Self::Time),
        }
    }
}

/// What a reminder's due point was measured from.
#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Baseline {
    /// The date and odometer of the latest service linked to the item.
    LastService,
    /// No linked service exists, so the vehicle's purchase date and mileage
    /// stand in. An overdue item with this baseline has no service on record;
    /// the work is not known to be late.
    Purchase,
    /// No linked service and no purchase date or mileage: the due date counts
    /// from the start of the model year, and the due mileage from zero. A
    /// vehicle with no model year either has no due date.
    None,
}

#[derive(Debug, Serialize, Clone)]
pub struct ReminderStatus {
    pub schedule_item: ScheduleItemSummary,
    /// How close the item is to due, by the more urgent of its mileage and
    /// time readings. An item with neither reading is ok.
    pub status: DueStatus,
    /// What `due_at_miles` and `due_at_date` count from.
    pub baseline: Baseline,
    pub last_service: Option<LastServiceInfo>,
    pub due_at_miles: Option<i32>,
    pub due_at_date: Option<NaiveDate>,
    pub miles_remaining: Option<i32>,
    pub days_remaining: Option<i64>,
    /// The reading that makes the item due: the one whose status equals
    /// `status`, or both when the two match. It is `None` when the item is ok.
    pub trigger: Option<Trigger>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ScheduleItemSummary {
    pub id: i32,
    pub name: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct LastServiceInfo {
    pub id: i32,
    pub date: NaiveDate,
    pub odometer: Option<i32>,
}

#[derive(Debug, Serialize, Clone)]
pub struct BundleSuggestion {
    pub reason: String,
    pub items: Vec<BundleItem>,
}

#[derive(Debug, Serialize, Clone)]
pub struct BundleItem {
    pub id: i32,
    pub name: String,
    pub status: DueStatus,
    pub due_in_miles: Option<i32>,
}

/// Warranty posture derived from the vehicle's nullable expiry fields.
/// `possibly_covered` is true when either bound still holds: today is on or
/// before `expires_on`, or the estimated mileage is at or below
/// `expires_miles`.
// Real warranties usually end at whichever bound comes first, but "possibly
// covered" errs toward telling the owner to check.
#[derive(Debug, Serialize, Clone)]
pub struct WarrantyStatus {
    pub expires_on: Option<NaiveDate>,
    pub expires_miles: Option<i32>,
    pub possibly_covered: bool,
}

#[derive(Debug, Serialize)]
pub struct RemindersResponse {
    pub vehicle_id: i32,
    pub estimated_mileage: i32,
    pub mileage_is_estimate: bool,
    pub mileage_as_of: NaiveDate,
    /// Holds the miles driven per day, which the mileage readings give, together
    /// with the purchase date and mileage when the readings span under 30 days.
    /// It is 0.0 when the vehicle has no readings.
    pub avg_daily_miles: f64,
    /// `None` when the vehicle has no warranty fields set.
    pub warranty: Option<WarrantyStatus>,
    pub reminders: Vec<ReminderStatus>,
    pub bundle_suggestions: Vec<BundleSuggestion>,
}

/// Derive the warranty flag from the vehicle record and the estimated
/// current mileage. `None` when neither expiry field is set.
fn warranty_status(
    v: &vehicle::Model,
    estimated_mileage: i32,
    today: NaiveDate,
) -> Option<WarrantyStatus> {
    if v.warranty_expires_on.is_none() && v.warranty_expires_miles.is_none() {
        return None;
    }
    let date_covers = v
        .warranty_expires_on
        .is_some_and(|expires| today <= expires);
    let miles_cover = v
        .warranty_expires_miles
        .is_some_and(|expires| estimated_mileage <= expires);
    Some(WarrantyStatus {
        expires_on: v.warranty_expires_on,
        expires_miles: v.warranty_expires_miles,
        possibly_covered: date_covers || miles_cover,
    })
}

/// Number of newest odometer readings the daily rate is measured over.
const RATE_WINDOW_READINGS: u64 = 50;

/// The fewest days the readings window must span for its rate to be trusted.
/// A shorter span is dominated by how the readings happened to be filed, so
/// the rate is measured from an earlier point instead.
const RATE_MIN_SPAN_DAYS: i64 = 30;

/// Miles per day between two dated odometer points. A span of zero or fewer
/// days and a falling odometer (a mistyped reading) both give zero.
#[allow(clippy::cast_precision_loss)]
fn daily_rate(from: (NaiveDate, i32), to: (NaiveDate, i32)) -> f64 {
    let days = (to.0 - from.0).num_days();
    if days <= 0 {
        return 0.0;
    }
    f64::from(Ord::max(to.1 - from.1, 0)) / days as f64
}

/// The oldest point to measure a rate from when the newest readings are too
/// close together: the vehicle's oldest logged reading when it falls outside
/// the readings window, else the purchase date and mileage when both are set.
async fn earlier_rate_anchor(
    db: &impl ConnectionTrait,
    v: &vehicle::Model,
    window_len: usize,
) -> DomainResult<Option<(NaiveDate, i32)>> {
    if window_len as u64 >= RATE_WINDOW_READINGS {
        let oldest = mileage_log::Entity::find()
            .filter(mileage_log::Column::VehicleId.eq(v.id))
            .order_by_asc(mileage_log::Column::RecordedAt)
            .one(db)
            .await?;
        if let Some(point) = oldest.map(|r| (r.recorded_at.day(), r.mileage)) {
            return Ok(Some(point));
        }
    }
    Ok(v.purchase_date.zip(v.purchase_mileage))
}

/// Estimate current mileage from `mileage_log` entries.
///
/// The daily rate is measured across the newest 50 readings when they span at
/// least 30 days. A shorter span is measured from an earlier point instead:
/// the oldest logged reading when it lies outside those 50, else the purchase
/// date and mileage when both are set. With neither, the rate comes from the
/// readings alone, and is zero when they share a date. The estimate starts at
/// the newest reading and projects forward at that rate.
pub(crate) async fn estimate_mileage(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    v: &vehicle::Model,
    today: NaiveDate,
) -> DomainResult<(i32, NaiveDate, f64, bool)> {
    let entries = mileage_log::Entity::find()
        .filter(mileage_log::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(mileage_log::Column::RecordedAt)
        .limit(RATE_WINDOW_READINGS)
        .all(db)
        .await?;

    // The stored text of a reading with no time of day sorts before a timed
    // reading of the same day, so the highest reading of the newest day is
    // the latest: an odometer only climbs.
    let Some(latest) = entries
        .iter()
        .max_by_key(|e| (e.recorded_at.day(), e.mileage))
    else {
        let mileage = v.purchase_mileage.unwrap_or(0);
        return Ok((mileage, today, 0.0, true));
    };
    let latest_date = latest.recorded_at.day();
    let latest_point = (latest_date, latest.mileage);

    let oldest = entries.last().unwrap_or(latest);
    let window_start = (oldest.recorded_at.day(), oldest.mileage);
    let avg_daily = if (latest_date - window_start.0).num_days() >= RATE_MIN_SPAN_DAYS {
        daily_rate(window_start, latest_point)
    } else {
        let from = earlier_rate_anchor(db, v, entries.len())
            .await?
            .unwrap_or(window_start);
        daily_rate(from, latest_point)
    };

    // Extrapolate from latest entry to today
    let days_since = Ord::max((today - latest_date).num_days(), 0);
    let is_estimate = days_since > 0;
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let estimated = latest.mileage + (avg_daily * days_since as f64) as i32;

    // When estimated, show the date of the last actual reading (what we extrapolated from).
    // When exact, the date is today (same as latest_date).
    Ok((estimated, latest_date, avg_daily, is_estimate))
}

/// Calculate reminders for a vehicle as of today.
pub async fn calculate_reminders(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<RemindersResponse> {
    calculate_reminders_on(db, vehicle_id, crate::services::today()).await
}

/// [`calculate_reminders`] as of `today`.
#[allow(clippy::too_many_lines)]
pub async fn calculate_reminders_on(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    today: NaiveDate,
) -> DomainResult<RemindersResponse> {
    let v = vehicle::Entity::find_by_id(vehicle_id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Vehicle {vehicle_id} not found")))?;

    let (estimated_mileage, mileage_as_of, avg_daily_miles, mileage_is_estimate) =
        estimate_mileage(db, vehicle_id, &v, today).await?;

    let effective_items: Vec<maintenance_schedule_item::Model> =
        super::schedule::resolve(db, vehicle_id)
            .await?
            .into_iter()
            .map(|r| r.effective_item)
            .collect();

    // Batch-load all schedule links and service records for this vehicle (avoids N+1)
    let item_ids: Vec<i32> = effective_items.iter().map(|i| i.id).collect();

    let all_links = super::schedule::links_to_items(db, vehicle_id, &item_ids).await?;

    let linked_service_ids: Vec<i32> = all_links
        .iter()
        .map(|(service_id, _)| *service_id)
        .collect();
    let all_services = if linked_service_ids.is_empty() {
        vec![]
    } else {
        service_record::Entity::find()
            .filter(service_record::Column::Id.is_in(linked_service_ids))
            .filter(service_record::Column::VehicleId.eq(vehicle_id))
            .all(db)
            .await?
    };

    // A service filed without an odometer reading is placed by the readings
    // around its date, which are loaded only when there is such a service.
    let readings = if all_services.iter().any(|s| s.mileage.is_none()) {
        dated_readings(db, &v).await?
    } else {
        Vec::new()
    };

    // For each schedule item, find last service and calculate status
    let mut reminders = Vec::new();

    for item in &effective_items {
        // Find the most recent service linked to this schedule item
        let item_service_ids: Vec<i32> = all_links
            .iter()
            .filter(|(_, item_id)| *item_id == item.id)
            .map(|(service_id, _)| *service_id)
            .collect();

        let last_service = all_services
            .iter()
            .filter(|s| item_service_ids.contains(&s.id))
            .max_by_key(|s| s.service_date);

        // Baseline: last service or vehicle purchase
        let (baseline_miles, baseline_date) = match &last_service {
            Some(svc) => {
                let date = svc.service_date;
                let miles = svc
                    .mileage
                    .or_else(|| odometer_on(date, &readings, avg_daily_miles))
                    .unwrap_or(v.purchase_mileage.unwrap_or(0));
                (miles, Some(date))
            }
            None => (
                v.purchase_mileage.unwrap_or(0),
                v.purchase_date.or_else(|| model_year_start(&v)),
            ),
        };

        // Calculate due mileage and date
        let due_at_miles = item.interval_miles.map(|i| baseline_miles + i);
        let due_at_date = item
            .interval_months
            .zip(baseline_date)
            .map(|(months, from)| add_months(from, months));

        let miles_remaining = due_at_miles.map(|due| due - estimated_mileage);
        let days_remaining = due_at_date.map(|due| (due - today).num_days());

        // Determine status based on whichever is closer to due
        let warning_miles = item.warning_miles.unwrap_or(DEFAULT_WARNING_MILES);
        let warning_days = item.warning_days.unwrap_or(DEFAULT_WARNING_DAYS);

        let miles_status =
            miles_remaining.map(|r| DueStatus::judge(i64::from(r), i64::from(warning_miles)));
        let time_status = days_remaining.map(|r| DueStatus::judge(r, i64::from(warning_days)));

        // Use the more urgent status
        let status = DueStatus::most_urgent(miles_status.into_iter().chain(time_status));

        let trigger = Trigger::of(miles_status, time_status);

        reminders.push(ReminderStatus {
            schedule_item: ScheduleItemSummary {
                id: item.id,
                name: item.name.clone(),
            },
            status,
            baseline: match (&last_service, &v.purchase_date, &v.purchase_mileage) {
                (Some(_), _, _) => Baseline::LastService,
                (None, None, None) => Baseline::None,
                (None, _, _) => Baseline::Purchase,
            },
            last_service: last_service.map(|s| LastServiceInfo {
                id: s.id,
                date: s.service_date,
                odometer: s.mileage,
            }),
            due_at_miles,
            due_at_date,
            miles_remaining,
            days_remaining,
            trigger,
        });
    }

    // Sort: overdue first, then due soon, then ok
    reminders.sort_by(|a, b| {
        a.status
            .urgency_rank()
            .cmp(&b.status.urgency_rank())
            .then(a.schedule_item.name.cmp(&b.schedule_item.name))
    });

    let bundle_suggestions = calculate_bundles(&reminders);

    Ok(RemindersResponse {
        vehicle_id,
        estimated_mileage,
        mileage_is_estimate,
        mileage_as_of,
        avg_daily_miles,
        warranty: warranty_status(&v, estimated_mileage, today),
        reminders,
        bundle_suggestions,
    })
}

/// The first day of the vehicle's model year, the earliest date any work on
/// it can have been done.
fn model_year_start(v: &vehicle::Model) -> Option<NaiveDate> {
    v.year.and_then(|year| NaiveDate::from_ymd_opt(year, 1, 1))
}

/// The vehicle's odometer readings with their dates, oldest first: every
/// logged reading, and the purchase reading when the vehicle has both a
/// purchase date and a purchase mileage.
async fn dated_readings(
    db: &impl ConnectionTrait,
    v: &vehicle::Model,
) -> DomainResult<Vec<(NaiveDate, i32)>> {
    let mut readings: Vec<(NaiveDate, i32)> = mileage_log::Entity::find()
        .filter(mileage_log::Column::VehicleId.eq(v.id))
        .all(db)
        .await?
        .into_iter()
        .map(|log| (log.recorded_at.day(), log.mileage))
        .collect();
    if let (Some(date), Some(miles)) = (v.purchase_date, v.purchase_mileage) {
        readings.push((date, miles));
    }
    readings.sort_unstable();
    Ok(readings)
}

/// Estimates the odometer on `date` from dated readings sorted oldest first.
/// Between two readings the estimate is interpolated. After the newest
/// reading or before the oldest one it moves away from that reading at
/// `avg_daily_miles`, never below zero. `None` when there are no readings.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn odometer_on(
    date: NaiveDate,
    readings: &[(NaiveDate, i32)],
    avg_daily_miles: f64,
) -> Option<i32> {
    let before = readings.iter().rev().find(|(day, _)| *day <= date);
    let after = readings.iter().find(|(day, _)| *day >= date);
    let drift = |from: NaiveDate| (avg_daily_miles * (date - from).num_days() as f64) as i32;
    match (before, after) {
        (Some(&(day0, miles0)), Some(&(day1, miles1))) => {
            let span = (day1 - day0).num_days();
            if span == 0 {
                return Some(miles0);
            }
            let elapsed = (date - day0).num_days();
            let gained = (i64::from(miles1) - i64::from(miles0)) * elapsed / span;
            Some(miles0 + gained as i32)
        }
        (Some(&(day, miles)), None) | (None, Some(&(day, miles))) => {
            Some(Ord::max(miles + drift(day), 0))
        }
        (None, None) => None,
    }
}

/// Writes a mileage with a comma between thousands, such as `61,200`.
#[must_use]
pub fn miles_label(miles: i32) -> String {
    let digits = miles.unsigned_abs().to_string();
    let mut out = String::from(if miles < 0 { "-" } else { "" });
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// Says how far past or short of due a schedule item is, from its
/// `miles_remaining` and `days_remaining`: `1,200 mi over`, `in 800 mi`,
/// `45 days over`, `in 1 day`, and `due now` or `due today` at exactly zero.
/// An item judged on both reads as both, mileage first.
#[must_use]
pub fn how_far(miles_remaining: Option<i32>, days_remaining: Option<i64>) -> String {
    let miles = miles_remaining.map(|r| match r {
        0 => "due now".to_owned(),
        r if r < 0 => format!("{} mi over", miles_label(r.saturating_neg())),
        r => format!("in {} mi", miles_label(r)),
    });
    let days = days_remaining.map(|r| {
        let noun = if r.abs() == 1 { "day" } else { "days" };
        match r {
            0 => "due today".to_owned(),
            r if r < 0 => format!("{} {noun} over", -r),
            r => format!("in {r} {noun}"),
        }
    });
    miles.into_iter().chain(days).collect::<Vec<_>>().join(", ")
}

/// Says where a schedule item comes due: `60,040 mi`, `2027-03-05`, or
/// `60,040 mi or 2027-03-05` for an item judged on both. It is empty for an
/// item with neither.
#[must_use]
pub fn due_at(miles: Option<i32>, date: Option<NaiveDate>) -> String {
    let miles = miles.map(|m| format!("{} mi", miles_label(m)));
    let date = date.map(super::date_label);
    miles
        .into_iter()
        .chain(date)
        .collect::<Vec<_>>()
        .join(" or ")
}

/// Says which odometer reading the maintenance was judged against:
/// `64,000 mi`, or `about 61,200 mi (estimated)` for a projected one.
#[must_use]
pub fn judged_at(miles: i32, is_estimate: bool) -> String {
    let miles = miles_label(miles);
    if is_estimate {
        format!("about {miles} mi (estimated)")
    } else {
        format!("{miles} mi")
    }
}

/// One shop-trip suggestion listing every overdue or due-soon item, or none
/// when fewer than two items are due.
fn calculate_bundles(reminders: &[ReminderStatus]) -> Vec<BundleSuggestion> {
    let items: Vec<BundleItem> = reminders
        .iter()
        .filter(|r| r.status.is_due())
        .map(|r| BundleItem {
            id: r.schedule_item.id,
            name: r.schedule_item.name.clone(),
            status: r.status,
            due_in_miles: r.miles_remaining,
        })
        .collect();
    if items.len() < 2 {
        return Vec::new();
    }
    vec![BundleSuggestion {
        reason: format!(
            "{} items are overdue or due soon; do them in one shop trip",
            items.len()
        ),
        items,
    }]
}

#[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
fn add_months(date: NaiveDate, months: i32) -> NaiveDate {
    if months <= 0 {
        return date;
    }
    let total_months = date.month() as i32 + months - 1;
    let new_year = date.year() + total_months / 12;
    let new_month = (total_months % 12 + 1) as u32;
    let new_day = Ord::min(date.day(), days_in_month(new_year, new_month));
    NaiveDate::from_ymd_opt(new_year, new_month, new_day).unwrap_or(date)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        2 => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
        // 30-day months + unreachable fallback for invalid month values
        _ => 30,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{
        entities::service_schedule_link,
        test_support::{VehicleFixture, date, days_ago, test_db},
    };

    #[test]
    fn due_at_names_the_mileage_the_date_or_both() {
        let day = date::<NaiveDate>("2027-03-05");
        assert_eq!(due_at(Some(60040), None), "60,040 mi");
        assert_eq!(due_at(None, Some(day)), "2027-03-05");
        assert_eq!(due_at(Some(60040), Some(day)), "60,040 mi or 2027-03-05");
        assert_eq!(due_at(None, None), "");
    }

    #[test]
    fn judged_at_marks_an_estimate() {
        assert_eq!(judged_at(64000, false), "64,000 mi");
        assert_eq!(judged_at(61200, true), "about 61,200 mi (estimated)");
    }

    #[test]
    fn miles_label_puts_a_comma_between_thousands() {
        for (miles, text) in [
            (0, "0"),
            (999, "999"),
            (1000, "1,000"),
            (61200, "61,200"),
            (1_234_567, "1,234,567"),
            (-1500, "-1,500"),
        ] {
            assert_eq!(miles_label(miles), text);
        }
    }

    #[test]
    fn how_far_words_each_reading_and_joins_both() {
        for (miles, days, text) in [
            (Some(-1200), None, "1,200 mi over"),
            (Some(800), None, "in 800 mi"),
            (Some(0), None, "due now"),
            (None, Some(-45), "45 days over"),
            (None, Some(1), "in 1 day"),
            (None, Some(-1), "1 day over"),
            (None, Some(0), "due today"),
            (Some(-3960), Some(30), "3,960 mi over, in 30 days"),
            (None, None, ""),
        ] {
            assert_eq!(how_far(miles, days), text);
        }
    }

    async fn seed_vehicle(
        db: &impl ConnectionTrait,
        warranty_expires_on: Option<&str>,
        warranty_expires_miles: Option<i32>,
    ) -> i32 {
        let mut fixture = VehicleFixture::new();
        if let Some(on) = warranty_expires_on {
            fixture = fixture.warranty_expires_on(&on);
        }
        if let Some(miles) = warranty_expires_miles {
            fixture = fixture.warranty_expires_miles(miles);
        }
        fixture.insert_id(db).await
    }

    #[tokio::test]
    async fn missing_vehicle_is_domain_not_found() {
        let db = test_db().await;
        assert!(matches!(
            calculate_reminders(&db, 999).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn warranty_flag_is_absent_without_fields_and_reflects_date_bound() {
        let db = test_db().await;

        // No warranty fields → no flag.
        let plain = seed_vehicle(&db, None, None).await;
        assert!(
            calculate_reminders(&db, plain)
                .await
                .unwrap()
                .warranty
                .is_none()
        );

        // Date bound still in the future → possibly covered.
        let covered = seed_vehicle(&db, Some("2099-01-01"), None).await;
        let w = calculate_reminders(&db, covered)
            .await
            .unwrap()
            .warranty
            .unwrap();
        assert!(w.possibly_covered);
        assert_eq!(w.expires_on, Some(date("2099-01-01")));

        // Date bound in the past, no mileage bound → not covered.
        let expired = seed_vehicle(&db, Some("2020-01-01"), None).await;
        let w = calculate_reminders(&db, expired)
            .await
            .unwrap()
            .warranty
            .unwrap();
        assert!(!w.possibly_covered);
    }

    #[tokio::test]
    async fn warranty_mileage_bound_covers_until_estimate_passes_it() {
        let db = test_db().await;
        // Expired date but a live mileage bound: EITHER bound suffices.
        let vid = seed_vehicle(&db, Some("2020-01-01"), Some(60_000)).await;
        let now = days_ago(0);
        mileage_log::ActiveModel {
            vehicle_id: Set(vid),
            mileage: Set(50_000),
            recorded_at: Set(now.into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let resp = calculate_reminders(&db, vid).await.unwrap();
        assert_eq!(resp.estimated_mileage, 50_000);
        assert!(resp.warranty.unwrap().possibly_covered);

        // Same bounds but the odometer is already past → not covered.
        let past = seed_vehicle(&db, Some("2020-01-01"), Some(40_000)).await;
        let now = days_ago(0);
        mileage_log::ActiveModel {
            vehicle_id: Set(past),
            mileage: Set(50_000),
            recorded_at: Set(now.into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        assert!(
            !calculate_reminders(&db, past)
                .await
                .unwrap()
                .warranty
                .unwrap()
                .possibly_covered
        );
    }

    #[tokio::test]
    async fn an_item_inside_its_warning_window_is_due_soon() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None, None).await;
        let today = crate::services::today();
        let item = maintenance_schedule_item::ActiveModel {
            vehicle_id: Set(Some(vid)),
            name: Set("Wipers".into()),
            interval_months: Set(Some(12)),
            enabled: Set(true),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        // Serviced 350 days ago: due in about 15 days, inside the 30-day default.
        let svc = service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set(today - chrono::Duration::days(350)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        service_schedule_link::ActiveModel {
            service_record_id: Set(svc.id),
            schedule_item_id: Set(item.id),
        }
        .insert(&db)
        .await
        .unwrap();

        let resp = calculate_reminders(&db, vid).await.unwrap();
        assert_eq!(resp.reminders[0].status, DueStatus::DueSoon);
        assert_eq!(resp.reminders[0].trigger, Some(Trigger::Time));
    }

    #[test]
    fn zero_remaining_is_due_soon_and_only_a_negative_remainder_is_overdue() {
        assert_eq!(DueStatus::judge(0, 1000), DueStatus::DueSoon);
        assert_eq!(DueStatus::judge(0, 0), DueStatus::DueSoon);
        assert_eq!(DueStatus::judge(0, -1), DueStatus::DueSoon);
        assert_eq!(DueStatus::judge(-1, 1000), DueStatus::Overdue);
        assert_eq!(DueStatus::judge(1000, 1000), DueStatus::DueSoon);
        assert_eq!(DueStatus::judge(1001, 1000), DueStatus::NotDue);
    }

    #[test]
    fn a_trigger_names_the_reading_with_the_overall_status() {
        use DueStatus::{DueSoon, NotDue, Overdue};
        // Mileage is due soon and time is overdue, so time drives the status.
        assert_eq!(
            Trigger::of(Some(DueSoon), Some(Overdue)),
            Some(Trigger::Time)
        );
        assert_eq!(
            Trigger::of(Some(Overdue), Some(DueSoon)),
            Some(Trigger::Mileage)
        );
        assert_eq!(
            Trigger::of(Some(DueSoon), Some(DueSoon)),
            Some(Trigger::Both)
        );
        assert_eq!(
            Trigger::of(Some(Overdue), Some(Overdue)),
            Some(Trigger::Both)
        );
        assert_eq!(
            Trigger::of(Some(NotDue), Some(DueSoon)),
            Some(Trigger::Time)
        );
        assert_eq!(
            Trigger::of(Some(DueSoon), Some(NotDue)),
            Some(Trigger::Mileage)
        );
        assert_eq!(Trigger::of(Some(NotDue), Some(NotDue)), None);
        assert_eq!(Trigger::of(Some(DueSoon), None), Some(Trigger::Mileage));
        assert_eq!(Trigger::of(None, Some(Overdue)), Some(Trigger::Time));
        assert_eq!(Trigger::of(None, Some(NotDue)), None);
        assert_eq!(Trigger::of(None, None), None);
    }

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn odometer_on_interpolates_between_readings_and_drifts_past_them() {
        let readings = [(day("2020-01-01"), 10_000), (day("2020-01-11"), 11_000)];
        assert_eq!(
            odometer_on(day("2020-01-01"), &readings, 50.0),
            Some(10_000)
        );
        assert_eq!(
            odometer_on(day("2020-01-06"), &readings, 50.0),
            Some(10_500)
        );
        assert_eq!(
            odometer_on(day("2020-01-21"), &readings, 50.0),
            Some(11_500)
        );
        assert_eq!(odometer_on(day("2019-12-22"), &readings, 50.0), Some(9_500));
        assert_eq!(odometer_on(day("2019-01-01"), &readings, 50.0), Some(0));
        assert_eq!(odometer_on(day("2020-01-06"), &[], 50.0), None);
    }

    #[tokio::test]
    async fn a_service_with_no_odometer_reading_still_restarts_a_mileage_interval() {
        let db = test_db().await;
        let vid = VehicleFixture::new()
            .purchase_date("2017-03-05")
            .purchase_mileage(40)
            .insert_id(&db)
            .await;
        mileage_log::ActiveModel {
            vehicle_id: Set(vid),
            mileage: Set(64_000),
            recorded_at: Set(days_ago(0).into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let item = maintenance_schedule_item::ActiveModel {
            vehicle_id: Set(Some(vid)),
            name: Set("Spark plugs".into()),
            interval_miles: Set(Some(60_000)),
            enabled: Set(true),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        assert_eq!(
            calculate_reminders(&db, vid).await.unwrap().reminders[0].status,
            DueStatus::Overdue
        );

        // The invoice had no odometer reading on it.
        let svc = service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set(days_ago(0)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        service_schedule_link::ActiveModel {
            service_record_id: Set(svc.id),
            schedule_item_id: Set(item.id),
        }
        .insert(&db)
        .await
        .unwrap();

        let plugs = &calculate_reminders(&db, vid).await.unwrap().reminders[0];
        assert_eq!(plugs.status, DueStatus::NotDue);
        assert_eq!(plugs.due_at_miles, Some(124_000));
    }

    async fn log_reading(db: &DatabaseConnection, vehicle_id: i32, age_days: i64, miles: i32) {
        mileage_log::ActiveModel {
            vehicle_id: Set(vehicle_id),
            mileage: Set(miles),
            recorded_at: Set(days_ago(age_days).into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn wipers_every_12_months(db: &DatabaseConnection, vehicle_id: i32) {
        maintenance_schedule_item::ActiveModel {
            vehicle_id: Set(Some(vehicle_id)),
            name: Set("Wipers".into()),
            interval_months: Set(Some(12)),
            enabled: Set(true),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    // With no purchase date and no service, the car's model year is the
    // earliest the work can have been done.
    #[tokio::test]
    async fn a_time_item_with_no_purchase_date_is_measured_from_the_model_year() {
        let db = test_db().await;
        let vid = VehicleFixture::new().insert_id(&db).await;
        vehicle::ActiveModel {
            id: Set(vid),
            year: Set(Some(2017)),
            ..Default::default()
        }
        .update(&db)
        .await
        .unwrap();
        wipers_every_12_months(&db, vid).await;

        let wipers = &calculate_reminders(&db, vid).await.unwrap().reminders[0];
        assert_eq!(wipers.due_at_date, Some(day("2018-01-01")));
        assert_eq!(wipers.status, DueStatus::Overdue);
        assert_eq!(wipers.baseline, Baseline::None);
    }

    #[tokio::test]
    async fn a_time_item_with_no_date_to_measure_from_has_no_due_date() {
        let db = test_db().await;
        let vid = VehicleFixture::new().insert_id(&db).await;
        wipers_every_12_months(&db, vid).await;

        let wipers = &calculate_reminders(&db, vid).await.unwrap().reminders[0];
        assert_eq!(wipers.due_at_date, None);
        assert_eq!(wipers.days_remaining, None);
        assert_eq!(wipers.trigger, None);
    }

    // A reading with no time of day is stored as text that sorts before a
    // timed reading of the same day, and the odometer only climbs.
    #[tokio::test]
    async fn the_highest_of_a_days_readings_is_the_latest() {
        let db = test_db().await;
        let vid = VehicleFixture::new().insert_id(&db).await;
        let today = crate::services::today();
        for (recorded_at, miles) in [
            (
                crate::dates::DayOrTime::Time(today.and_hms_opt(8, 0, 0).unwrap()),
                50_000,
            ),
            (crate::dates::DayOrTime::Day(today), 50_400),
        ] {
            mileage_log::ActiveModel {
                vehicle_id: Set(vid),
                mileage: Set(miles),
                recorded_at: Set(recorded_at),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        let v = vehicle::Entity::find_by_id(vid)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let (estimated, ..) = estimate_mileage(&db, vid, &v, today).await.unwrap();
        assert_eq!(estimated, 50_400);
    }

    async fn vehicle_bought_2017(db: &DatabaseConnection) -> i32 {
        VehicleFixture::new()
            .purchase_date("2017-03-05")
            .purchase_mileage(40)
            .insert_id(db)
            .await
    }

    #[expect(clippy::cast_precision_loss, reason = "a day count in the thousands")]
    fn days_since_purchase() -> f64 {
        (crate::services::today() - day("2017-03-05")).num_days() as f64
    }

    #[tokio::test]
    async fn close_together_readings_measure_the_rate_from_the_purchase() {
        let db = test_db().await;
        let vid = vehicle_bought_2017(&db).await;
        log_reading(&db, vid, 2, 64_000).await;
        log_reading(&db, vid, 0, 64_000).await;

        let v = vehicle::Entity::find_by_id(vid)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let (estimated, _, rate, _) = estimate_mileage(&db, vid, &v, crate::services::today())
            .await
            .unwrap();
        let expected = 63_960.0 / days_since_purchase();
        assert!(
            (rate - expected).abs() < 0.5,
            "rate {rate}, expected {expected}"
        );
        assert_eq!(estimated, 64_000);
    }

    #[tokio::test]
    async fn a_mileage_item_approaches_due_at_the_purchase_anchored_rate() {
        let db = test_db().await;
        let vid = vehicle_bought_2017(&db).await;
        log_reading(&db, vid, 2, 64_000).await;
        log_reading(&db, vid, 0, 64_000).await;
        let item = maintenance_schedule_item::ActiveModel {
            vehicle_id: Set(Some(vid)),
            name: Set("Oil".into()),
            interval_miles: Set(Some(10_000)),
            enabled: Set(true),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        // Last done at 54,500, so due at 64,500: 500 miles away.
        let svc = service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set(days_ago(300)),
            mileage: Set(Some(54_500)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        service_schedule_link::ActiveModel {
            service_record_id: Set(svc.id),
            schedule_item_id: Set(item.id),
        }
        .insert(&db)
        .await
        .unwrap();

        let resp = calculate_reminders(&db, vid).await.unwrap();
        assert!(resp.avg_daily_miles > 10.0, "rate {}", resp.avg_daily_miles);
        assert_eq!(resp.reminders[0].status, DueStatus::DueSoon);
        assert_eq!(resp.reminders[0].miles_remaining, Some(500));
    }

    #[tokio::test]
    async fn an_item_exactly_at_its_due_mileage_is_due_soon() {
        let db = test_db().await;
        let vid = vehicle_bought_2017(&db).await;
        log_reading(&db, vid, 2, 64_000).await;
        log_reading(&db, vid, 0, 64_000).await;
        let item = maintenance_schedule_item::ActiveModel {
            vehicle_id: Set(Some(vid)),
            name: Set("Oil".into()),
            interval_miles: Set(Some(10_000)),
            enabled: Set(true),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        // The last service was at 54,000, so the item is due at 64,000, which
        // is today's reading.
        let svc = service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set(days_ago(300)),
            mileage: Set(Some(54_000)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        service_schedule_link::ActiveModel {
            service_record_id: Set(svc.id),
            schedule_item_id: Set(item.id),
        }
        .insert(&db)
        .await
        .unwrap();

        let resp = calculate_reminders(&db, vid).await.unwrap();
        assert_eq!(resp.reminders[0].miles_remaining, Some(0));
        assert_eq!(resp.reminders[0].status, DueStatus::DueSoon);
        assert_eq!(resp.reminders[0].trigger, Some(Trigger::Mileage));
    }

    #[tokio::test]
    async fn a_window_spanning_ninety_days_keeps_its_own_rate() {
        let db = test_db().await;
        let vid = vehicle_bought_2017(&db).await;
        log_reading(&db, vid, 90, 60_000).await;
        log_reading(&db, vid, 0, 60_900).await;

        let v = vehicle::Entity::find_by_id(vid)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let (_, _, rate, _) = estimate_mileage(&db, vid, &v, crate::services::today())
            .await
            .unwrap();
        assert!((rate - 10.0).abs() < 1e-9, "rate {rate}");
    }

    #[tokio::test]
    async fn an_older_reading_outside_the_window_anchors_the_rate() {
        let db = test_db().await;
        let vid = vehicle_bought_2017(&db).await;
        // 49 readings on today's date plus one the day before fill the
        // window of 50; the reading 400 days back lies outside it.
        log_reading(&db, vid, 400, 50_000).await;
        log_reading(&db, vid, 1, 60_000).await;
        for _ in 0..49 {
            log_reading(&db, vid, 0, 60_000).await;
        }

        let v = vehicle::Entity::find_by_id(vid)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let (_, _, rate, _) = estimate_mileage(&db, vid, &v, crate::services::today())
            .await
            .unwrap();
        assert!((rate - 10_000.0 / 400.0).abs() < 1e-9, "rate {rate}");
    }

    #[tokio::test]
    async fn two_same_day_readings_without_purchase_data_give_a_zero_rate() {
        let db = test_db().await;
        let vid = VehicleFixture::new().insert_id(&db).await;
        log_reading(&db, vid, 0, 50_000).await;
        log_reading(&db, vid, 0, 50_400).await;

        let v = vehicle::Entity::find_by_id(vid)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let (estimated, _, rate, _) = estimate_mileage(&db, vid, &v, crate::services::today())
            .await
            .unwrap();
        assert!(rate.abs() < f64::EPSILON, "rate {rate}");
        assert_eq!(estimated, 50_400);
    }

    #[tokio::test]
    async fn one_reading_with_purchase_data_measures_from_the_purchase() {
        let db = test_db().await;
        let vid = vehicle_bought_2017(&db).await;
        log_reading(&db, vid, 0, 64_000).await;

        let v = vehicle::Entity::find_by_id(vid)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let (_, _, rate, _) = estimate_mileage(&db, vid, &v, crate::services::today())
            .await
            .unwrap();
        let expected = 63_960.0 / days_since_purchase();
        assert!(
            (rate - expected).abs() < 0.5,
            "rate {rate}, expected {expected}"
        );
    }

    #[tokio::test]
    async fn one_reading_without_purchase_data_gives_a_zero_rate() {
        let db = test_db().await;
        let vid = VehicleFixture::new().insert_id(&db).await;
        log_reading(&db, vid, 0, 64_000).await;

        let v = vehicle::Entity::find_by_id(vid)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let (_, _, rate, _) = estimate_mileage(&db, vid, &v, crate::services::today())
            .await
            .unwrap();
        assert!(rate.abs() < f64::EPSILON, "rate {rate}");
    }

    #[test]
    fn daily_rate_is_zero_for_a_zero_day_span_and_a_falling_odometer() {
        let start = (day("2020-01-01"), 1_000);
        assert!(daily_rate(start, (day("2020-01-01"), 5_000)).abs() < f64::EPSILON);
        assert!(daily_rate(start, (day("2019-12-31"), 5_000)).abs() < f64::EPSILON);
        assert!(daily_rate(start, (day("2020-01-11"), 500)).abs() < f64::EPSILON);
        assert!((daily_rate(start, (day("2020-01-11"), 1_500)) - 50.0).abs() < 1e-9);
    }

    async fn seed_monthly_item(db: &DatabaseConnection, vehicle_id: i32, name: &str) -> i32 {
        maintenance_schedule_item::ActiveModel {
            vehicle_id: Set(Some(vehicle_id)),
            name: Set(name.into()),
            interval_months: Set(Some(12)),
            enabled: Set(true),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn every_overdue_or_due_soon_item_is_offered_as_one_shop_trip() {
        let db = test_db().await;
        let vid = VehicleFixture::new()
            .purchase_date(&days_ago(3_000))
            .insert_id(&db)
            .await;
        for name in ["Brake fluid", "Cabin filter", "Spark plugs"] {
            seed_monthly_item(&db, vid, name).await;
        }

        let resp = calculate_reminders(&db, vid).await.unwrap();
        assert_eq!(resp.bundle_suggestions.len(), 1, "{resp:?}");
        let bundle = &resp.bundle_suggestions[0];
        let mut names: Vec<&str> = bundle.items.iter().map(|i| i.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, vec!["Brake fluid", "Cabin filter", "Spark plugs"]);
        assert!(bundle.items.iter().all(|i| i.status == DueStatus::Overdue));
        assert!(bundle.reason.contains('3'), "{}", bundle.reason);
    }

    #[tokio::test]
    async fn one_due_item_is_not_a_bundle() {
        let db = test_db().await;
        let vid = VehicleFixture::new()
            .purchase_date(&days_ago(3_000))
            .insert_id(&db)
            .await;
        seed_monthly_item(&db, vid, "Brake fluid").await;

        let resp = calculate_reminders(&db, vid).await.unwrap();
        assert!(resp.bundle_suggestions.is_empty());
    }

    #[tokio::test]
    async fn a_reminder_says_what_its_due_point_was_measured_from() {
        let db = test_db().await;
        let dated = VehicleFixture::new()
            .purchase_date(&days_ago(3_000))
            .insert_id(&db)
            .await;
        let serviced = seed_monthly_item(&db, dated, "Serviced").await;
        seed_monthly_item(&db, dated, "Never serviced").await;
        let svc = service_record::ActiveModel {
            vehicle_id: Set(dated),
            service_date: Set(days_ago(100)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        service_schedule_link::ActiveModel {
            service_record_id: Set(svc.id),
            schedule_item_id: Set(serviced),
        }
        .insert(&db)
        .await
        .unwrap();

        let resp = calculate_reminders(&db, dated).await.unwrap();
        let baseline = |name: &str| {
            resp.reminders
                .iter()
                .find(|r| r.schedule_item.name == name)
                .unwrap()
                .baseline
        };
        assert_eq!(baseline("Serviced"), Baseline::LastService);
        assert_eq!(baseline("Never serviced"), Baseline::Purchase);

        let undated = VehicleFixture::new().insert_id(&db).await;
        seed_monthly_item(&db, undated, "Never serviced").await;
        let resp = calculate_reminders(&db, undated).await.unwrap();
        assert_eq!(resp.reminders[0].baseline, Baseline::None);
        let json = serde_json::to_value(&resp.reminders[0]).unwrap();
        assert_eq!(json["baseline"], "none");
    }
}
