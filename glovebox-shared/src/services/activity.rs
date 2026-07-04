//! Recent-activity feed: one domain operation that merges a vehicle's
//! service records, incidents, and mileage logs into a single
//! reverse-chronological list. Consumed by the MCP surface
//! (`summarize_recent_activity` tool + `glovebox://vehicles/{id}/activity`
//! resource); available to the HTTP surface should it ever want the same
//! feed.

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{incident, mileage_log, service_record},
    error::DomainResult,
};

/// Item count used when the caller doesn't specify a limit.
pub const DEFAULT_LIMIT: usize = 20;

/// One entry in the merged feed. `date` is the record's own domain date
/// (`service_date`, `occurred_at`, or `recorded_at`) — ISO-8601
/// `YYYY-MM-DD[ HH:MM:SS]` strings. Granularity is mixed: services are
/// date-only while incidents/mileage carry timestamps, so sorting uses a
/// normalized key (see `sort_key`), not the raw string.
#[derive(Debug, Serialize)]
pub struct ActivityItem {
    /// `service`, `incident`, or `mileage`.
    pub kind: String,
    pub id: i32,
    /// Owning vehicle — lets garage-wide consumers ([`recent_all`]) label
    /// rows without a second lookup. Additive fields: existing consumers
    /// (MCP `summarize_recent_activity` + the activity resource) simply
    /// gain them.
    pub vehicle_id: i32,
    pub vehicle_name: String,
    pub date: String,
    pub summary: String,
    /// Odometer reading attached to the record, when present.
    pub mileage: Option<i32>,
    /// Present on `service` items only. Integer cents.
    pub total_cost_cents: Option<i32>,
}

/// The `limit` most recent services + incidents + mileage logs for a
/// vehicle, merged and sorted newest-first.
///
/// Mileage logs auto-created by service records are excluded: the service
/// item already carries that odometer reading, and showing both would
/// double-report every service. Exclusion keys on the real linkage
/// (`service_record_id`, maintained by the service-record create/update/
/// delete paths), not the free-text `source` label — so a manual log a
/// client happens to label `source: "service"` stays visible, and deleted
/// services take their auto-log with them instead of leaving a hidden ghost.
pub async fn recent(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    limit: usize,
) -> DomainResult<Vec<ActivityItem>> {
    let vehicle = super::vehicle::require(db, vehicle_id).await?;
    let vehicle_name = vehicle.name;
    let limit_u64 = limit as u64;

    let services = service_record::Entity::find()
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(service_record::Column::ServiceDate)
        .order_by_desc(service_record::Column::Id)
        .limit(limit_u64)
        .all(db)
        .await?;
    let incidents = incident::Entity::find()
        .filter(incident::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(incident::Column::OccurredAt)
        .order_by_desc(incident::Column::Id)
        .limit(limit_u64)
        .all(db)
        .await?;
    let mileage_logs = mileage_log::Entity::find()
        .filter(mileage_log::Column::VehicleId.eq(vehicle_id))
        .filter(mileage_log::Column::ServiceRecordId.is_null())
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
            vehicle_id,
            vehicle_name: vehicle_name.clone(),
            summary: s
                .description
                .clone()
                .unwrap_or_else(|| format!("Service on {}", s.service_date)),
            date: s.service_date,
            mileage: s.mileage,
            total_cost_cents: s.total_cost_cents,
        });
    }
    for i in incidents {
        items.push(ActivityItem {
            kind: "incident".into(),
            id: i.id,
            vehicle_id,
            vehicle_name: vehicle_name.clone(),
            date: i.occurred_at,
            summary: i.title,
            mileage: i.odometer,
            total_cost_cents: None,
        });
    }
    for m in mileage_logs {
        items.push(ActivityItem {
            kind: "mileage".into(),
            id: m.id,
            vehicle_id,
            vehicle_name: vehicle_name.clone(),
            date: m.recorded_at,
            summary: m
                .notes
                .clone()
                .unwrap_or_else(|| format!("Odometer reading: {}", m.mileage)),
            mileage: Some(m.mileage),
            total_cost_cents: None,
        });
    }

    sort_newest_first(&mut items);
    items.truncate(limit);
    Ok(items)
}

/// The garage-wide merged feed: [`recent`] for every vehicle (per-vehicle
/// loop — this is a personal app with FEW vehicles), merged and re-sorted
/// newest-first, capped at `limit`. Archived vehicles' history is included:
/// the feed is "what happened in the garage", and archived cars' records
/// are still records.
pub async fn recent_all(
    db: &impl ConnectionTrait,
    limit: usize,
) -> DomainResult<Vec<ActivityItem>> {
    let vehicles = super::vehicle::list(db).await?;
    let mut items: Vec<ActivityItem> = Vec::new();
    for v in vehicles {
        items.extend(recent(db, v.id, limit).await?);
    }
    sort_newest_first(&mut items);
    items.truncate(limit);
    Ok(items)
}

/// Newest first; kind/id tiebreakers keep equal-date ordering deterministic.
fn sort_newest_first(items: &mut [ActivityItem]) {
    items.sort_by(|a, b| {
        sort_key(&b.date)
            .cmp(&sort_key(&a.date))
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| b.id.cmp(&a.id))
    });
}

/// Normalize mixed-granularity dates into one sortable key. Date-only values
/// (services' `service_date`) sort at end-of-day so a service records as
/// *newer* than that same day's timestamped incidents — matching the
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
            incident::NewIncident, mileage::NewMileageEntry, service_record::NewServiceRecord,
        },
        services::{incident as incident_svc, mileage as mileage_svc, service_record as svc_svc},
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

    fn incident(title: &str, occurred_at: &str) -> NewIncident {
        NewIncident {
            category: "note".into(),
            title: title.into(),
            occurred_at: Some(occurred_at.into()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn merges_all_three_kinds_newest_first() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        svc_svc::create(&db, vid, service("2026-01-10", "Oil change", None))
            .await
            .unwrap();
        incident_svc::create(&db, vid, incident("Squeak", "2026-02-20 08:00:00"))
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
            vec!["mileage", "incident", "service"],
        );
        assert_eq!(items[2].summary, "Oil change");
        assert_eq!(items[2].total_cost_cents, Some(12_345));
        assert_eq!(items[0].mileage, Some(50_000));
    }

    #[tokio::test]
    async fn same_day_service_sorts_newer_than_timestamped_items() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        // Morning incident, then a date-only service the same day: the
        // service must surface as the newer item ("noticed it in the
        // morning, fixed it that afternoon").
        incident_svc::create(&db, vid, incident("Squeak", "2026-06-01 09:00:00"))
            .await
            .unwrap();
        svc_svc::create(&db, vid, service("2026-06-01", "Fixed squeak", None))
            .await
            .unwrap();

        let items = recent(&db, vid, DEFAULT_LIMIT).await.unwrap();
        assert_eq!(
            items.iter().map(|i| i.kind.as_str()).collect::<Vec<_>>(),
            vec!["service", "incident"],
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
    async fn manual_log_labeled_service_is_still_visible() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        // An HTTP client may label a manual log `source: "service"`. Exclusion
        // keys on the real FK now, so this log must still appear in the feed.
        mileage_svc::create(
            &db,
            vid,
            NewMileageEntry {
                mileage: 60_000,
                recorded_at: Some("2026-04-01 10:00:00".into()),
                source: Some("service".into()),
                notes: None,
            },
        )
        .await
        .unwrap();

        let items = recent(&db, vid, DEFAULT_LIMIT).await.unwrap();
        assert_eq!(
            items.len(),
            1,
            "manual 'service' log must be visible: {items:?}"
        );
        assert_eq!(items[0].kind, "mileage");
        assert_eq!(items[0].mileage, Some(60_000));
    }

    #[tokio::test]
    async fn deleting_a_service_removes_its_auto_log_from_the_feed() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = svc_svc::create(&db, vid, service("2026-01-10", "Brakes", Some(48_000)))
            .await
            .unwrap();

        svc_svc::delete(&db, vid, created.record.id).await.unwrap();

        // No orphaned auto-log lingers: the feed is empty, not hiding a ghost.
        assert!(recent(&db, vid, DEFAULT_LIMIT).await.unwrap().is_empty());
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

    #[tokio::test]
    async fn items_carry_vehicle_id_and_name() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        svc_svc::create(&db, vid, service("2026-01-10", "Oil change", None))
            .await
            .unwrap();

        let items = recent(&db, vid, DEFAULT_LIMIT).await.unwrap();
        assert_eq!(items[0].vehicle_id, vid);
        assert_eq!(items[0].vehicle_name, "Car");
    }

    #[tokio::test]
    async fn recent_all_merges_across_vehicles_newest_first_with_names() {
        let db = test_db().await;
        use crate::entities::vehicle;
        let a = vehicle::ActiveModel {
            name: Set("Golf".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        let b = vehicle::ActiveModel {
            name: Set("Vanagon".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        svc_svc::create(&db, a, service("2026-01-10", "Brakes", None))
            .await
            .unwrap();
        incident_svc::create(&db, b, incident("Squeak", "2026-02-20 08:00:00"))
            .await
            .unwrap();

        let items = recent_all(&db, DEFAULT_LIMIT).await.unwrap();
        assert_eq!(
            items
                .iter()
                .map(|i| (i.kind.as_str(), i.vehicle_name.as_str()))
                .collect::<Vec<_>>(),
            vec![("incident", "Vanagon"), ("service", "Golf")],
        );
        assert_eq!(items[1].vehicle_id, a);

        // The cap applies to the MERGED feed, not per vehicle.
        let capped = recent_all(&db, 1).await.unwrap();
        assert_eq!(capped.len(), 1);
        assert_eq!(capped[0].kind, "incident");
    }

    #[tokio::test]
    async fn recent_all_on_empty_garage_is_empty() {
        let db = test_db().await;
        assert!(recent_all(&db, DEFAULT_LIMIT).await.unwrap().is_empty());
    }
}
