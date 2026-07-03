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

/// Warranty posture derived from the vehicle's nullable expiry fields
/// (decision ⑩). `possibly_covered` is true when EITHER bound still holds —
/// today ≤ `expires_on` OR estimated mileage ≤ `expires_miles` (either is
/// sufficient on its own; real warranties are usually whichever-comes-first,
/// but "possibly covered" deliberately errs toward telling the user to
/// check).
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

/// Estimate current mileage from `mileage_log` entries.
/// Calculates average daily miles from the oldest and newest of the last 50 entries,
/// then extrapolates forward from the most recent entry to today.
pub async fn estimate_mileage(
    db: &DatabaseConnection,
    vehicle_id: i32,
    v: &vehicle::Model,
) -> DomainResult<(i32, String, f64, bool)> {
    let entries = mileage_log::Entity::find()
        .filter(mileage_log::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(mileage_log::Column::RecordedAt)
        .limit(50)
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

    // Calculate average daily miles from the entries we have
    #[allow(clippy::cast_precision_loss)]
    let avg_daily = if entries.len() >= 2 {
        let oldest = entries.last().unwrap();
        let oldest_date = parse_date(&oldest.recorded_at).unwrap_or(latest_date);
        let days = (latest_date - oldest_date).num_days().max(1);
        let miles = (latest.mileage - oldest.mileage).max(0);
        f64::from(miles) / days as f64
    } else {
        // With only one entry, use purchase mileage as baseline
        let purchase_miles = v.purchase_mileage.unwrap_or(0);
        let purchase_date = v
            .purchase_date
            .as_deref()
            .and_then(parse_date)
            .unwrap_or(latest_date);
        let days = (latest_date - purchase_date).num_days().max(1);
        let miles = (latest.mileage - purchase_miles).max(0);
        f64::from(miles) / days as f64
    };

    // Extrapolate from latest entry to today
    let days_since = (today - latest_date).num_days().max(0);
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
    db: &DatabaseConnection,
    vehicle_id: i32,
) -> DomainResult<RemindersResponse> {
    let v = vehicle::Entity::find_by_id(vehicle_id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Vehicle {vehicle_id} not found")))?;

    let (estimated_mileage, mileage_as_of, avg_daily_miles, mileage_is_estimate) =
        estimate_mileage(db, vehicle_id, &v).await?;

    // Resolve the effective schedule (same algorithm as api::schedules::resolve)
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
            Some(svc) => (
                svc.mileage.unwrap_or(v.purchase_mileage.unwrap_or(0)),
                parse_date(&svc.service_date).unwrap_or(today),
            ),
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
                "upcoming"
            } else {
                "ok"
            }
        });

        let time_status = days_remaining.map(|r| {
            if r <= 0 {
                "overdue"
            } else if r <= i64::from(warning_days) {
                "upcoming"
            } else {
                "ok"
            }
        });

        // Use the more urgent status
        let status = match (miles_status, time_status) {
            (Some("overdue"), _) | (_, Some("overdue")) => "overdue",
            (Some("upcoming"), _) | (_, Some("upcoming")) => "upcoming",
            _ => "ok",
        };

        let trigger = match (miles_status, time_status) {
            (Some(m), Some(t)) if m == t => Some("both"),
            (Some("overdue" | "upcoming"), _) | (Some(_), None) => Some("mileage"),
            (_, Some("overdue" | "upcoming")) | (None, Some(_)) => Some("time"),
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

    // Sort: overdue first, then upcoming, then ok
    reminders.sort_by(|a, b| {
        let priority = |s: &str| match s {
            "overdue" => 0,
            "upcoming" => 1,
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

/// Resolve effective schedule items for a vehicle (Platform → Model Template → Vehicle).
async fn resolve_schedule(
    db: &DatabaseConnection,
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
                .filter(|r| r.status == "overdue" || r.status == "upcoming")
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

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
fn add_months(date: NaiveDate, months: i32) -> NaiveDate {
    if months <= 0 {
        return date;
    }
    let total_months = date.month() as i32 + months - 1;
    let new_year = date.year() + total_months / 12;
    let new_month = (total_months % 12 + 1) as u32;
    let new_day = date.day().min(days_in_month(new_year, new_month));
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
    use crate::test_support::test_db;

    async fn seed_vehicle(
        db: &impl ConnectionTrait,
        warranty_expires_on: Option<&str>,
        warranty_expires_miles: Option<i32>,
    ) -> i32 {
        vehicle::ActiveModel {
            name: Set("Car".into()),
            warranty_expires_on: Set(warranty_expires_on.map(str::to_string)),
            warranty_expires_miles: Set(warranty_expires_miles),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
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
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
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
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
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
}
