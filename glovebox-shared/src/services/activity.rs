//! Recent-activity feed: one domain operation that merges a vehicle's
//! service records, observations, and mileage logs into a single
//! reverse-chronological list. Consumed by the MCP surface
//! (`summarize_recent_activity` tool + `glovebox://vehicles/{id}/activity`
//! resource); available to the HTTP surface should it ever want the same
//! feed.

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{mileage_log, observation, service_record},
    error::DomainResult,
};

/// Item count used when the caller doesn't specify a limit.
pub const DEFAULT_LIMIT: usize = 20;

/// One entry in the merged feed. `date` is the record's own domain date
/// (`service_date`, `observed_at`, or `recorded_at`) — ISO-8601
/// `YYYY-MM-DD[ HH:MM:SS]` strings. Granularity is mixed: services are
/// date-only while observations/mileage carry timestamps, so sorting uses a
/// normalized key (see `sort_key`), not the raw string.
#[derive(Debug, Serialize)]
pub struct ActivityItem {
    /// `service`, `observation`, or `mileage`.
    pub kind: String,
    pub id: i32,
    pub date: String,
    pub summary: String,
    /// Odometer reading attached to the record, when present.
    pub mileage: Option<i32>,
    /// Present on `service` items only. Integer cents.
    pub total_cost_cents: Option<i32>,
}

/// The `limit` most recent services + observations + mileage logs for a
/// vehicle, merged and sorted newest-first.
///
/// Mileage logs auto-created by service records (`source = "service"`) are
/// excluded: the service item already carries that odometer reading, and
/// showing both would double-report every service.
///
/// Known limits of that exclusion (`mileage_log` has no `service_record_id` FK,
/// so `source` is the only linkage): a log left behind by a since-deleted
/// service stays hidden here even though reminders still count it, and an
/// HTTP client that labels a manual log `source: "service"` hides it too.
/// The durable fix is a real FK — tracked as a data-model follow-up.
pub async fn recent(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    limit: usize,
) -> DomainResult<Vec<ActivityItem>> {
    super::vehicle::require(db, vehicle_id).await?;
    let limit_u64 = limit as u64;

    let services = service_record::Entity::find()
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(service_record::Column::ServiceDate)
        .order_by_desc(service_record::Column::Id)
        .limit(limit_u64)
        .all(db)
        .await?;
    let observations = observation::Entity::find()
        .filter(observation::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(observation::Column::ObservedAt)
        .order_by_desc(observation::Column::Id)
        .limit(limit_u64)
        .all(db)
        .await?;
    let mileage_logs = mileage_log::Entity::find()
        .filter(mileage_log::Column::VehicleId.eq(vehicle_id))
        .filter(
            Condition::any()
                .add(mileage_log::Column::Source.ne("service"))
                .add(mileage_log::Column::Source.is_null()),
        )
        .order_by_desc(mileage_log::Column::RecordedAt)
        .order_by_desc(mileage_log::Column::Id)
        .limit(limit_u64)
        .all(db)
        .await?;

    let mut items: Vec<ActivityItem> = Vec::new();
    for s in services {
        items.push(ActivityItem {
            kind: "service".into(),
            id: s.id,
            summary: s
                .description
                .clone()
                .unwrap_or_else(|| format!("Service on {}", s.service_date)),
            date: s.service_date,
            mileage: s.mileage,
            total_cost_cents: s.total_cost_cents,
        });
    }
    for o in observations {
        items.push(ActivityItem {
            kind: "observation".into(),
            id: o.id,
            date: o.observed_at,
            summary: o.title,
            mileage: o.odometer,
            total_cost_cents: None,
        });
    }
    for m in mileage_logs {
        items.push(ActivityItem {
            kind: "mileage".into(),
            id: m.id,
            date: m.recorded_at,
            summary: m
                .notes
                .clone()
                .unwrap_or_else(|| format!("Odometer reading: {}", m.mileage)),
            mileage: Some(m.mileage),
            total_cost_cents: None,
        });
    }

    // Newest first; kind/id tiebreakers keep equal-date ordering deterministic.
    items.sort_by(|a, b| {
        sort_key(&b.date)
            .cmp(&sort_key(&a.date))
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| b.id.cmp(&a.id))
    });
    items.truncate(limit);
    Ok(items)
}

/// Normalize mixed-granularity dates into one sortable key. Date-only values
/// (services' `service_date`) sort at end-of-day so a service records as
/// *newer* than that same day's timestamped observations — matching the
/// common "noticed it in the morning, fixed it that afternoon" flow. Raw
/// lexicographic order would instead sort every date-only service before
/// any timestamped item on its own day.
fn sort_key(date: &str) -> std::borrow::Cow<'_, str> {
    if date.len() == 10 {
        std::borrow::Cow::Owned(format!("{date} 23:59:59"))
    } else {
        std::borrow::Cow::Borrowed(date)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        error::DomainError,
        inputs::{
            mileage::NewMileageEntry, observation::NewObservation, service_record::NewServiceRecord,
        },
        services::{mileage as mileage_svc, observation as obs_svc, service_record as svc_svc},
        test_support::test_db,
    };

    async fn seed_vehicle(db: &impl ConnectionTrait) -> i32 {
        use crate::entities::vehicle;
        vehicle::ActiveModel {
            name: Set("Car".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    fn service(date: &str, description: &str, mileage: Option<i32>) -> NewServiceRecord {
        NewServiceRecord {
            service_date: date.into(),
            mileage,
            description: Some(description.into()),
            parts_cost_cents: None,
            parts_cost_currency: None,
            labor_cost_cents: None,
            labor_cost_currency: None,
            total_cost_cents: Some(12_345),
            total_cost_currency: None,
            shop_name: None,
            shop_id: None,
            notes: None,
            build_id: None,
            paid_by: None,
            payer_note: None,
            schedule_item_ids: None,
            part_ids: None,
            line_items: None,
        }
    }

    fn observation(title: &str, observed_at: &str) -> NewObservation {
        NewObservation {
            category: "note".into(),
            title: title.into(),
            description: None,
            odometer: None,
            observed_at: Some(observed_at.into()),
            obd_codes: None,
            notes: None,
            build_id: None,
        }
    }

    #[tokio::test]
    async fn merges_all_three_kinds_newest_first() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        svc_svc::create(&db, vid, service("2026-01-10", "Oil change", None))
            .await
            .unwrap();
        obs_svc::create(&db, vid, observation("Squeak", "2026-02-20 08:00:00"))
            .await
            .unwrap();
        mileage_svc::create(
            &db,
            vid,
            NewMileageEntry {
                mileage: 50_000,
                recorded_at: Some("2026-03-01 09:00:00".into()),
                source: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        let items = recent(&db, vid, DEFAULT_LIMIT).await.unwrap();
        assert_eq!(
            items.iter().map(|i| i.kind.as_str()).collect::<Vec<_>>(),
            vec!["mileage", "observation", "service"],
        );
        assert_eq!(items[2].summary, "Oil change");
        assert_eq!(items[2].total_cost_cents, Some(12_345));
        assert_eq!(items[0].mileage, Some(50_000));
    }

    #[tokio::test]
    async fn same_day_service_sorts_newer_than_timestamped_items() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        // Morning observation, then a date-only service the same day: the
        // service must surface as the newer item ("noticed it in the
        // morning, fixed it that afternoon").
        obs_svc::create(&db, vid, observation("Squeak", "2026-06-01 09:00:00"))
            .await
            .unwrap();
        svc_svc::create(&db, vid, service("2026-06-01", "Fixed squeak", None))
            .await
            .unwrap();

        let items = recent(&db, vid, DEFAULT_LIMIT).await.unwrap();
        assert_eq!(
            items.iter().map(|i| i.kind.as_str()).collect::<Vec<_>>(),
            vec!["service", "observation"],
        );
    }

    #[tokio::test]
    async fn excludes_service_sourced_mileage_logs() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        // Creating a service WITH mileage auto-creates a `source="service"`
        // mileage log; the feed must show one service item, not two entries.
        svc_svc::create(&db, vid, service("2026-01-10", "Brakes", Some(48_000)))
            .await
            .unwrap();

        let items = recent(&db, vid, DEFAULT_LIMIT).await.unwrap();
        assert_eq!(items.len(), 1, "expected only the service item: {items:?}");
        assert_eq!(items[0].kind, "service");
        assert_eq!(items[0].mileage, Some(48_000));
    }

    #[tokio::test]
    async fn limit_caps_the_merged_feed() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        for day in 1..=5 {
            svc_svc::create(&db, vid, service(&format!("2026-01-{day:02}"), "svc", None))
                .await
                .unwrap();
        }
        let items = recent(&db, vid, 3).await.unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].date, "2026-01-05");
    }

    #[tokio::test]
    async fn scoped_to_vehicle() {
        let db = test_db().await;
        let mine = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        svc_svc::create(&db, other, service("2026-01-10", "Not mine", None))
            .await
            .unwrap();
        assert!(recent(&db, mine, DEFAULT_LIMIT).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn missing_vehicle_is_not_found() {
        let db = test_db().await;
        assert!(matches!(
            recent(&db, 999, DEFAULT_LIMIT).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }
}
