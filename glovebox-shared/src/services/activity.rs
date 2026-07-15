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
    /// When the record was ADDED (its row's `created_at`) — a uniform
    /// `YYYY-MM-DD HH:MM:SS` timestamp, distinct from the domain `date`
    /// above. The garage-wide feed ([`recent_all`]) orders by this so a
    /// just-imported record with an old event date still surfaces as
    /// recent; the per-vehicle [`recent`] feed keeps ordering by `date`.
    /// Additive field: existing consumers simply gain it.
    pub created_at: String,
    pub summary: String,
    /// Odometer reading attached to the record, when present.
    pub mileage: Option<i32>,
    /// Present on `service` items only. Integer cents.
    pub total_cost_cents: Option<i32>,
}

/// Which timeline a feed is ordered against. [`recent`] is event-chronological
/// (`Event`); [`recent_all`] is added-chronological (`Added`) so freshly
/// imported records surface even when their event date is old. The order is
/// applied *both* in the per-entity SQL fetch (so the per-vehicle `limit`
/// truncation keeps the right rows) and in the final in-memory sort.
#[derive(Clone, Copy)]
enum Order {
    Event,
    Added,
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
    gather(db, vehicle_id, limit, Order::Event).await
}

/// Fetch, merge, sort, and truncate one vehicle's feed under a given [`Order`].
///
/// The `order` selects the per-entity SQL sort column *and* the final
/// in-memory comparator so the two agree: the per-vehicle `limit` truncation
/// must keep the same rows the caller will ultimately rank by, or a
/// recently-*added* old-*event* record would be dropped here before a
/// [`recent_all`] merge ever sees it.
async fn gather(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    limit: usize,
    order: Order,
) -> DomainResult<Vec<ActivityItem>> {
    let vehicle = super::vehicle::require(db, vehicle_id).await?;
    let vehicle_name = vehicle.name;
    let limit_u64 = limit as u64;

    let mut service_q =
        service_record::Entity::find().filter(service_record::Column::VehicleId.eq(vehicle_id));
    service_q = match order {
        Order::Event => service_q.order_by_desc(service_record::Column::ServiceDate),
        Order::Added => service_q.order_by_desc(service_record::Column::CreatedAt),
    };
    let services = service_q
        .order_by_desc(service_record::Column::Id)
        .limit(limit_u64)
        .all(db)
        .await?;

    let mut incident_q =
        incident::Entity::find().filter(incident::Column::VehicleId.eq(vehicle_id));
    incident_q = match order {
        Order::Event => incident_q.order_by_desc(incident::Column::OccurredAt),
        Order::Added => incident_q.order_by_desc(incident::Column::CreatedAt),
    };
    let incidents = incident_q
        .order_by_desc(incident::Column::Id)
        .limit(limit_u64)
        .all(db)
        .await?;

    let mut mileage_q = mileage_log::Entity::find()
        .filter(mileage_log::Column::VehicleId.eq(vehicle_id))
        .filter(mileage_log::Column::ServiceRecordId.is_null());
    mileage_q = match order {
        Order::Event => mileage_q.order_by_desc(mileage_log::Column::RecordedAt),
        Order::Added => mileage_q.order_by_desc(mileage_log::Column::CreatedAt),
    };
    let mileage_logs = mileage_q
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
            created_at: s.created_at,
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
            created_at: i.created_at,
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
            created_at: m.created_at,
            summary: m
                .notes
                .clone()
                .unwrap_or_else(|| format!("Odometer reading: {}", m.mileage)),
            mileage: Some(m.mileage),
            total_cost_cents: None,
        });
    }

    sort_by(&mut items, order);
    items.truncate(limit);
    Ok(items)
}

/// The garage-wide merged feed: every vehicle's records (per-vehicle loop —
/// this is a personal app with FEW vehicles), merged and sorted by *when they
/// were added* (`created_at`), capped at `limit`. This is what the dashboard's
/// "Recent activity" surfaces: what recently entered the garage's records, so
/// a just-imported service with an old service date still shows up. Each
/// vehicle is gathered under [`Order::Added`] so its per-vehicle `limit`
/// truncation keeps the most-recently-*added* rows, matching the merge order.
/// Archived vehicles' history is included: the feed is "what happened in the
/// garage", and archived cars' records are still records.
pub async fn recent_all(
    db: &impl ConnectionTrait,
    limit: usize,
) -> DomainResult<Vec<ActivityItem>> {
    let vehicles = super::vehicle::list(db).await?;
    let mut items: Vec<ActivityItem> = Vec::new();
    for v in vehicles {
        items.extend(gather(db, v.id, limit, Order::Added).await?);
    }
    sort_by(&mut items, Order::Added);
    items.truncate(limit);
    Ok(items)
}

/// Sort newest-first under `order`. `Event` ranks by the domain date (via
/// [`sort_key`]'s end-of-day normalization for mixed granularity); `Added`
/// ranks by the uniform-timestamp `created_at`. Equal primary keys break by
/// `kind` first (ids are per-table sequences, so they aren't comparable across
/// kinds), then by `id` descending *within a kind* — where higher id = later
/// insert, so same-kind equal-key rows read as "more recently added". This
/// keeps equal-key ordering deterministic.
fn sort_by(items: &mut [ActivityItem], order: Order) {
    items.sort_by(|a, b| {
        let primary = match order {
            Order::Event => sort_key(&b.date).cmp(&sort_key(&a.date)),
            Order::Added => b.created_at.cmp(&a.created_at),
        };
        primary
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
            invoice_ref: None,
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

    /// Insert a service_record ActiveModel directly with an explicit
    /// `created_at`: the service `create` fn leaves `created_at` to the DB
    /// default (`datetime('now')`), so in-test rows would all share one
    /// wall-clock second and couldn't demonstrate added-order. Setting it
    /// explicitly (Set overrides the default) lets us decouple the added
    /// date from the event date.
    async fn insert_service_raw(
        db: &impl ConnectionTrait,
        vehicle_id: i32,
        service_date: &str,
        created_at: &str,
        description: &str,
    ) -> i32 {
        use crate::entities::service_record;
        service_record::ActiveModel {
            vehicle_id: Set(vehicle_id),
            service_date: Set(service_date.into()),
            description: Set(Some(description.into())),
            created_at: Set(created_at.into()),
            updated_at: Set(created_at.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn recent_all_sorts_by_created_at_not_event_date() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        // A: recent event, added long ago. B: old event, added recently
        // (the just-imported record). The garage feed must surface B first.
        insert_service_raw(
            &db,
            vid,
            "2026-06-01",
            "2026-01-01 00:00:00",
            "Recent event",
        )
        .await;
        insert_service_raw(
            &db,
            vid,
            "2026-01-01",
            "2026-06-01 00:00:00",
            "Just imported",
        )
        .await;

        let garage = recent_all(&db, DEFAULT_LIMIT).await.unwrap();
        assert_eq!(
            garage
                .iter()
                .map(|i| i.summary.as_str())
                .collect::<Vec<_>>(),
            vec!["Just imported", "Recent event"],
            "recent_all orders by created_at (added) desc",
        );

        // The per-vehicle feed stays event-chronological (Timeline + MCP).
        let per_vehicle = recent(&db, vid, DEFAULT_LIMIT).await.unwrap();
        assert_eq!(
            per_vehicle
                .iter()
                .map(|i| i.summary.as_str())
                .collect::<Vec<_>>(),
            vec!["Recent event", "Just imported"],
            "recent orders by event date desc, unchanged",
        );
    }

    #[tokio::test]
    async fn recent_all_keeps_recently_added_old_event_row_past_the_per_vehicle_limit() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        // `limit` records with recent events but old added-dates, plus ONE
        // just-added record with the OLDEST event date. An event-ordered
        // per-vehicle truncation to `limit` would cut the just-added row
        // before the merge — the exact pre-truncation bug. It must survive.
        let limit = 3;
        for day in 1..=limit {
            insert_service_raw(
                &db,
                vid,
                &format!("2026-06-{day:02}"),
                &format!("2026-01-{day:02} 00:00:00"),
                &format!("old-add {day}"),
            )
            .await;
        }
        insert_service_raw(&db, vid, "2025-01-01", "2026-12-31 00:00:00", "just added").await;

        let garage = recent_all(&db, limit).await.unwrap();
        assert_eq!(garage.len(), limit);
        assert_eq!(
            garage[0].summary, "just added",
            "the most-recently-added row survives the per-vehicle limit: {garage:?}",
        );
    }

    #[tokio::test]
    async fn recent_all_equal_created_at_tiebreaks_by_id_desc() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        // Same added timestamp (a bulk import in one second): the later
        // insert (higher id) reads as "more recently added".
        let first =
            insert_service_raw(&db, vid, "2026-03-01", "2026-06-01 00:00:00", "first").await;
        let second =
            insert_service_raw(&db, vid, "2026-02-01", "2026-06-01 00:00:00", "second").await;
        assert!(second > first);

        let garage = recent_all(&db, DEFAULT_LIMIT).await.unwrap();
        assert_eq!(
            garage.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![second, first],
            "equal created_at falls back to id desc",
        );
    }
}
