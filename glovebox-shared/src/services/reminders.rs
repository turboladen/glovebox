use chrono::{Datelike, NaiveDate};
use sea_orm::*;
use serde::Serialize;
use std::collections::HashMap;

use crate::{
    entities::{
        maintenance_schedule_item, mileage_log, model_template, service_record,
        service_schedule_link, vehicle,
    },
    error::{DomainError, DomainResult},
};

#[derive(Debug, Serialize, Clone)]
pub struct ReminderStatus {
    pub schedule_item: ScheduleItemSummary,
    /// `overdue`, `due_soon` (within the item's warning miles or days) or
    /// `ok`: the same words planned work's `revisit_status` uses.
    pub status: String,
    pub last_service: Option<LastServiceInfo>,
    pub due_at_miles: Option<i32>,
    pub due_at_date: Option<String>,
    pub miles_remaining: Option<i32>,
    pub days_remaining: Option<i64>,
    pub trigger: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ScheduleItemSummary {
    pub id: i32,
    pub name: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct LastServiceInfo {
    pub id: i32,
    pub date: String,
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
    pub status: String,
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
    pub expires_on: Option<String>,
    pub expires_miles: Option<i32>,
    pub possibly_covered: bool,
}

#[derive(Debug, Serialize)]
pub struct RemindersResponse {
    pub vehicle_id: i32,
    pub estimated_mileage: i32,
    pub mileage_is_estimate: bool,
    pub mileage_as_of: String,
    /// Miles driven per day, measured as [`estimate_mileage`] describes.
    pub avg_daily_miles: f64,
    /// `None` when the vehicle has no warranty fields set.
    pub warranty: Option<WarrantyStatus>,
    pub reminders: Vec<ReminderStatus>,
    pub bundle_suggestions: Vec<BundleSuggestion>,
}

/// Derive the warranty flag from the vehicle record and the estimated
/// current mileage. `None` when neither expiry field is set.
fn warranty_status(v: &vehicle::Model, estimated_mileage: i32) -> Option<WarrantyStatus> {
    if v.warranty_expires_on.is_none() && v.warranty_expires_miles.is_none() {
        return None;
    }
    let today = chrono::Utc::now().date_naive();
    let date_covers = v
        .warranty_expires_on
        .as_deref()
        .and_then(parse_date)
        .is_some_and(|expires| today <= expires);
    let miles_cover = v
        .warranty_expires_miles
        .is_some_and(|expires| estimated_mileage <= expires);
    Some(WarrantyStatus {
        expires_on: v.warranty_expires_on.clone(),
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
        if let Some(point) = oldest.and_then(|r| Some((parse_date(&r.recorded_at)?, r.mileage))) {
            return Ok(Some(point));
        }
    }
    let purchased_on = v.purchase_date.as_deref().and_then(parse_date);
    Ok(purchased_on.zip(v.purchase_mileage))
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
) -> DomainResult<(i32, String, f64, bool)> {
    let entries = mileage_log::Entity::find()
        .filter(mileage_log::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(mileage_log::Column::RecordedAt)
        .limit(RATE_WINDOW_READINGS)
        .all(db)
        .await?;

    let today = chrono::Utc::now().date_naive();
    let now_str = today.format("%Y-%m-%d").to_string();

    if entries.is_empty() {
        let mileage = v.purchase_mileage.unwrap_or(0);
        return Ok((mileage, now_str, 0.0, true));
    }

    let latest = &entries[0];
    let latest_date = parse_date(&latest.recorded_at).unwrap_or(today);
    let latest_point = (latest_date, latest.mileage);

    let oldest = entries.last().unwrap();
    let window_start = (
        parse_date(&oldest.recorded_at).unwrap_or(latest_date),
        oldest.mileage,
    );
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
    let as_of = latest_date.format("%Y-%m-%d").to_string();

    Ok((estimated, as_of, avg_daily, is_estimate))
}

/// Calculate reminders for a vehicle.
#[allow(clippy::too_many_lines)]
pub async fn calculate_reminders(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<RemindersResponse> {
    let v = vehicle::Entity::find_by_id(vehicle_id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Vehicle {vehicle_id} not found")))?;

    let (estimated_mileage, mileage_as_of, avg_daily_miles, mileage_is_estimate) =
        estimate_mileage(db, vehicle_id, &v).await?;

    // The same layering as `schedule::resolve`: vehicle over template over platform.
    let effective_items = resolve_schedule(db, &v).await?;

    let today = chrono::Utc::now().date_naive();

    // Batch-load all schedule links and service records for this vehicle (avoids N+1)
    let item_ids: Vec<i32> = effective_items.iter().map(|i| i.id).collect();

    let all_links = if item_ids.is_empty() {
        vec![]
    } else {
        service_schedule_link::Entity::find()
            .filter(service_schedule_link::Column::ScheduleItemId.is_in(item_ids))
            .all(db)
            .await?
    };

    let linked_service_ids: Vec<i32> = all_links.iter().map(|l| l.service_record_id).collect();
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
            .filter(|l| l.schedule_item_id == item.id)
            .map(|l| l.service_record_id)
            .collect();

        let last_service = all_services
            .iter()
            .filter(|s| item_service_ids.contains(&s.id))
            .max_by(|a, b| a.service_date.cmp(&b.service_date));

        // Baseline: last service or vehicle purchase
        let (baseline_miles, baseline_date) = match &last_service {
            Some(svc) => {
                let date = parse_date(&svc.service_date).unwrap_or(today);
                let miles = svc
                    .mileage
                    .or_else(|| odometer_on(date, &readings, avg_daily_miles))
                    .unwrap_or(v.purchase_mileage.unwrap_or(0));
                (miles, date)
            }
            None => (
                v.purchase_mileage.unwrap_or(0),
                v.purchase_date
                    .as_deref()
                    .and_then(parse_date)
                    .unwrap_or(today),
            ),
        };

        // Calculate due mileage and date
        let due_at_miles = item.interval_miles.map(|i| baseline_miles + i);
        let due_at_date = item.interval_months.map(|m| add_months(baseline_date, m));

        let miles_remaining = due_at_miles.map(|due| due - estimated_mileage);
        let days_remaining = due_at_date.map(|due| (due - today).num_days());

        // Determine status based on whichever is closer to due
        let warning_miles = item.warning_miles.unwrap_or(1000);
        let warning_days = item.warning_days.unwrap_or(30);

        let miles_status = miles_remaining.map(|r| {
            if r <= 0 {
                "overdue"
            } else if r <= warning_miles {
                "due_soon"
            } else {
                "ok"
            }
        });

        let time_status = days_remaining.map(|r| {
            if r <= 0 {
                "overdue"
            } else if r <= i64::from(warning_days) {
                "due_soon"
            } else {
                "ok"
            }
        });

        // Use the more urgent status
        let status = match (miles_status, time_status) {
            (Some("overdue"), _) | (_, Some("overdue")) => "overdue",
            (Some("due_soon"), _) | (_, Some("due_soon")) => "due_soon",
            _ => "ok",
        };

        let trigger = match (miles_status, time_status) {
            (Some(m), Some(t)) if m == t => Some("both"),
            (Some("overdue" | "due_soon"), _) | (Some(_), None) => Some("mileage"),
            (_, Some("overdue" | "due_soon")) | (None, Some(_)) => Some("time"),
            _ => None,
        };

        reminders.push(ReminderStatus {
            schedule_item: ScheduleItemSummary {
                id: item.id,
                name: item.name.clone(),
            },
            status: status.to_string(),
            last_service: last_service.map(|s| LastServiceInfo {
                id: s.id,
                date: s.service_date.clone(),
                odometer: s.mileage,
            }),
            due_at_miles,
            due_at_date: due_at_date.map(|d| d.format("%Y-%m-%d").to_string()),
            miles_remaining,
            days_remaining,
            trigger: trigger.map(std::string::ToString::to_string),
        });
    }

    // Sort: overdue first, then due soon, then ok
    reminders.sort_by(|a, b| {
        let priority = |s: &str| match s {
            "overdue" => 0,
            "due_soon" => 1,
            _ => 2,
        };
        priority(&a.status)
            .cmp(&priority(&b.status))
            .then(a.schedule_item.name.cmp(&b.schedule_item.name))
    });

    // Bundle suggestions
    let bundle_suggestions = calculate_bundles(&reminders, 5000);

    Ok(RemindersResponse {
        vehicle_id,
        estimated_mileage,
        mileage_is_estimate,
        mileage_as_of,
        avg_daily_miles,
        warranty: warranty_status(&v, estimated_mileage),
        reminders,
        bundle_suggestions,
    })
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
        .filter_map(|log| Some((parse_date(&log.recorded_at)?, log.mileage)))
        .collect();
    let purchased_on = v.purchase_date.as_deref().and_then(parse_date);
    if let (Some(date), Some(miles)) = (purchased_on, v.purchase_mileage) {
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

/// Resolve effective schedule items for a vehicle (Platform → Model Template → Vehicle).
async fn resolve_schedule(
    db: &impl ConnectionTrait,
    v: &vehicle::Model,
) -> DomainResult<Vec<maintenance_schedule_item::Model>> {
    let mut schedule: HashMap<String, maintenance_schedule_item::Model> = HashMap::new();

    if let Some(mt_id) = v.model_template_id {
        let mt = model_template::Entity::find_by_id(mt_id).one(db).await?;
        if let Some(mt) = &mt {
            // Layer 1: Platform items
            if let Some(platform_id) = mt.platform_id {
                let items = maintenance_schedule_item::Entity::find()
                    .filter(maintenance_schedule_item::Column::PlatformId.eq(platform_id))
                    .all(db)
                    .await?;
                for item in items {
                    schedule.insert(item.name.clone(), item);
                }
            }

            // Layer 2: Model template items
            let items = maintenance_schedule_item::Entity::find()
                .filter(maintenance_schedule_item::Column::ModelTemplateId.eq(mt_id))
                .all(db)
                .await?;
            for item in items {
                schedule.insert(item.name.clone(), item);
            }
        }
    }

    // Layer 3: Vehicle-level items
    let items = maintenance_schedule_item::Entity::find()
        .filter(maintenance_schedule_item::Column::VehicleId.eq(v.id))
        .all(db)
        .await?;
    for item in items {
        schedule.insert(item.name.clone(), item);
    }

    // Filter disabled
    Ok(schedule.into_values().filter(|i| i.enabled).collect())
}

/// Calculate bundle suggestions based on `labor_categories`.
fn calculate_bundles(reminders: &[ReminderStatus], bundling_window: i32) -> Vec<BundleSuggestion> {
    let mut suggestions = Vec::new();

    for reminder in reminders {
        if reminder.status == "ok"
            && let Some(miles) = reminder.miles_remaining
            && miles <= bundling_window
        {
            // This item is within bundling window of being due
            let due_items: Vec<&ReminderStatus> = reminders
                .iter()
                .filter(|r| r.status == "overdue" || r.status == "due_soon")
                .collect();

            if !due_items.is_empty() {
                let mut items: Vec<BundleItem> = due_items
                    .iter()
                    .map(|r| BundleItem {
                        id: r.schedule_item.id,
                        name: r.schedule_item.name.clone(),
                        status: r.status.clone(),
                        due_in_miles: r.miles_remaining,
                    })
                    .collect();

                items.push(BundleItem {
                    id: reminder.schedule_item.id,
                    name: reminder.schedule_item.name.clone(),
                    status: reminder.status.clone(),
                    due_in_miles: reminder.miles_remaining,
                });

                suggestions.push(BundleSuggestion {
                    reason: format!(
                        "{} is due within {} miles — consider doing it with upcoming services",
                        reminder.schedule_item.name, miles
                    ),
                    items,
                });
            }
        }
    }

    suggestions
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    // Extract just the date portion (handles "2024-01-15", "2024-01-15 10:30:00", "2024-01-15T10:30:00")
    let date_part = s.split([' ', 'T']).next().unwrap_or(s);
    NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()
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
    use crate::test_support::{VehicleFixture, days_ago, test_db};

    async fn seed_vehicle(
        db: &impl ConnectionTrait,
        warranty_expires_on: Option<&str>,
        warranty_expires_miles: Option<i32>,
    ) -> i32 {
        let mut fixture = VehicleFixture::new();
        if let Some(on) = warranty_expires_on {
            fixture = fixture.warranty_expires_on(on);
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
        assert_eq!(w.expires_on.as_deref(), Some("2099-01-01"));

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
        let now = crate::services::now_stamp();
        mileage_log::ActiveModel {
            vehicle_id: Set(vid),
            mileage: Set(50_000),
            recorded_at: Set(now),
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
        let now = crate::services::now_stamp();
        mileage_log::ActiveModel {
            vehicle_id: Set(past),
            mileage: Set(50_000),
            recorded_at: Set(now),
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
        let today = chrono::Utc::now().date_naive();
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
            service_date: Set((today - chrono::Duration::days(350))
                .format("%Y-%m-%d")
                .to_string()),
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
        assert_eq!(resp.reminders[0].status, "due_soon");
        assert_eq!(resp.reminders[0].trigger.as_deref(), Some("time"));
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
            recorded_at: Set(days_ago(0)),
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
            "overdue"
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
        assert_eq!(plugs.status, "ok");
        assert_eq!(plugs.due_at_miles, Some(124_000));
    }

    async fn log_reading(db: &DatabaseConnection, vehicle_id: i32, age_days: i64, miles: i32) {
        mileage_log::ActiveModel {
            vehicle_id: Set(vehicle_id),
            mileage: Set(miles),
            recorded_at: Set(days_ago(age_days)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn vehicle_bought_2017(db: &DatabaseConnection) -> i32 {
        VehicleFixture::new()
            .purchase_date("2017-03-05")
            .purchase_mileage(40)
            .insert_id(db)
            .await
    }

    fn days_since_purchase() -> f64 {
        (chrono::Utc::now().date_naive() - day("2017-03-05")).num_days() as f64
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
        let (estimated, _, rate, _) = estimate_mileage(&db, vid, &v).await.unwrap();
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
        assert_eq!(resp.reminders[0].status, "due_soon");
        assert_eq!(resp.reminders[0].miles_remaining, Some(500));
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
        let (_, _, rate, _) = estimate_mileage(&db, vid, &v).await.unwrap();
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
        let (_, _, rate, _) = estimate_mileage(&db, vid, &v).await.unwrap();
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
        let (estimated, _, rate, _) = estimate_mileage(&db, vid, &v).await.unwrap();
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
        let (_, _, rate, _) = estimate_mileage(&db, vid, &v).await.unwrap();
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
        let (_, _, rate, _) = estimate_mileage(&db, vid, &v).await.unwrap();
        assert!(rate.abs() < f64::EPSILON, "rate {rate}");
    }
}
