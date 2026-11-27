//! Recent-activity feed: one domain operation that merges a vehicle's
//! service records, incidents, and mileage logs into a single
//! reverse-chronological list. Consumed by the MCP surface
//! (`summarize_recent_activity` tool + `glovebox://vehicles/{id}/activity`
//! resource).

use sea_orm::*;
use serde::Serialize;

use crate::{
    dates::DayOrTime,
    entities::{incident, mileage_log, service_record},
    error::DomainResult,
};

/// Item count used when the caller doesn't specify a limit.
pub const DEFAULT_LIMIT: usize = 20;
/// The most items [`recent`] returns, whatever `limit` asks for.
pub const MAX_LIMIT: usize = 200;

crate::vocab::vocabulary! {
    /// The kind of record an [`ActivityItem`] stands for.
    // No column stores these texts.
    pub enum ActivityKind {
        Service => "service",
        Incident => "incident",
        Mileage => "mileage",
    }
}

/// One entry in the merged feed. `date` is the record's own domain date
/// (`service_date`, `occurred_at`, or `recorded_at`), printed
/// `YYYY-MM-DD[ HH:MM:SS]`. Granularity is mixed: a service has a day, while
/// an incident or a reading may carry a time.
#[derive(Debug, Serialize)]
pub struct ActivityItem {
    pub kind: ActivityKind,
    pub id: i32,
    /// Owning vehicle.
    pub vehicle_id: i32,
    pub vehicle_name: String,
    /// The day of a service, or the day or time of an incident or reading.
    pub date: DayOrTime,
    /// When the record was added (its row's `created_at`), in UTC. It is
    /// distinct from the domain `date`.
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub summary: String,
    /// Odometer reading attached to the record, when present.
    pub mileage: Option<i32>,
    /// Present on `service` items only. Integer cents.
    pub total_cost_cents: Option<i32>,
    /// The notes of an odometer reading, or none when they are blank and on
    /// every other kind. `summary` already carries them, so they are left
    /// out of the serialized item.
    #[serde(skip)]
    pub reading_notes: Option<String>,
}

/// The `limit` most recent services + incidents + mileage logs for a
/// vehicle, merged and sorted newest-first by their own dates. A `limit`
/// above [`MAX_LIMIT`] returns that many.
///
/// The reading a service logs is left out, because the service's own item
/// already carries that odometer reading.
pub async fn recent(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    limit: usize,
) -> DomainResult<Vec<ActivityItem>> {
    let vehicle = super::vehicle::require(db, vehicle_id).await?;
    recent_of(db, &vehicle, limit).await
}

/// [`recent`] for a vehicle the caller has already read.
pub(crate) async fn recent_of(
    db: &impl ConnectionTrait,
    vehicle: &crate::entities::vehicle::Model,
    limit: usize,
) -> DomainResult<Vec<ActivityItem>> {
    let vehicle_id = vehicle.id;
    let vehicle_name = &vehicle.name;
    let limit = limit.min(MAX_LIMIT);
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
        // The link the service paths keep, not the `source` text, decides,
        // so a deleted service takes its reading with it.
        .filter(mileage_log::Column::ServiceRecordId.is_null())
        .order_by_desc(mileage_log::Column::RecordedAt)
        .order_by_desc(mileage_log::Column::Id)
        .limit(limit_u64)
        .all(db)
        .await?;

    let mut items: Vec<ActivityItem> = Vec::new();
    for s in services {
        items.push(ActivityItem {
            kind: ActivityKind::Service,
            id: s.id,
            vehicle_id,
            vehicle_name: vehicle_name.clone(),
            summary: s
                .description
                .as_deref()
                .and_then(super::trim_to_none)
                .unwrap_or_else(|| format!("Service on {}", s.service_date)),
            date: s.service_date.into(),
            created_at: s.created_at,
            mileage: s.mileage,
            total_cost_cents: s.total_cost_cents,
            reading_notes: None,
        });
    }
    for i in incidents {
        items.push(ActivityItem {
            kind: ActivityKind::Incident,
            id: i.id,
            vehicle_id,
            vehicle_name: vehicle_name.clone(),
            date: i.occurred_at,
            created_at: i.created_at,
            summary: i.title,
            mileage: i.odometer,
            total_cost_cents: None,
            reading_notes: None,
        });
    }
    for m in mileage_logs {
        let reading_notes = m.notes.as_deref().and_then(super::trim_to_none);
        items.push(ActivityItem {
            kind: ActivityKind::Mileage,
            id: m.id,
            vehicle_id,
            vehicle_name: vehicle_name.clone(),
            date: m.recorded_at,
            created_at: m.created_at,
            summary: reading_notes
                .clone()
                .unwrap_or_else(|| format!("Odometer reading: {}", m.mileage)),
            mileage: Some(m.mileage),
            total_cost_cents: None,
            reading_notes,
        });
    }

    sort_newest_first(&mut items);
    items.truncate(limit);
    Ok(items)
}

/// Sort newest-first by the domain date, a bare day counting as the end of
/// that day (see [`DayOrTime::or_end_of_day`]). Equal dates break by `kind` first
/// (ids are per-table sequences, so they aren't comparable across kinds),
/// then by `id` descending within a kind, where a higher id is a later
/// insert. This keeps equal-date ordering deterministic.
fn sort_newest_first(items: &mut [ActivityItem]) {
    items.sort_by(|a, b| {
        b.date
            .or_end_of_day()
            .cmp(&a.date.or_end_of_day())
            .then_with(|| a.kind.as_str().cmp(b.kind.as_str()))
            .then_with(|| b.id.cmp(&a.id))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::incident::IncidentCategory,
        error::DomainError,
        inputs::{
            incident::NewIncident, mileage::NewMileageEntry, service_record::NewServiceRecord,
        },
        services::{incident as incident_svc, mileage as mileage_svc, service_record as svc_svc},
        test_support::{date, test_db},
    };

    use crate::test_support::seed_vehicle;

    fn service(date: &str, description: &str, mileage: Option<i32>) -> NewServiceRecord {
        NewServiceRecord {
            service_date: crate::test_support::date(date),
            mileage,
            description: Some(description.into()),
            parts_cost_cents: None,
            labor_cost_cents: None,
            total_cost_cents: Some(12_345),
            shop: None,
            notes: None,
            paid_by: None,
            payer_note: None,
            schedule_item_ids: None,
            part_ids: None,
            line_items: None,
            invoice_ref: None,
            end_date: None,
            performed_by: None,
            work_item_ids: None,
            resolved_incident_ids: None,
            noticed_incident_ids: None,
            recall_finding_ids: None,
            parts: None,
            appointment_id: None,
            wear_readings: None,
        }
    }

    fn incident(title: &str, occurred_at: &str) -> NewIncident {
        NewIncident {
            category: IncidentCategory::Note,
            title: title.into(),
            occurred_at: Some(date(occurred_at)),
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
                recorded_at: Some(date("2026-03-01 09:00:00")),
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

    // Text that is blank names nothing, so the item reads as one with no
    // text does, and the serialized item carries no notes of its own.
    #[tokio::test]
    async fn a_blank_description_or_note_reads_as_none() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        svc_svc::create(&db, vid, service("2026-01-10", "  ", None))
            .await
            .unwrap();
        for (mileage, day, notes) in [
            (50_000, "2026-03-01", "   "),
            (50_500, "2026-04-01", " After the trip "),
        ] {
            let entry = NewMileageEntry {
                mileage,
                recorded_at: Some(date(day)),
                source: None,
                notes: Some(notes.into()),
            };
            mileage_svc::create(&db, vid, entry).await.unwrap();
        }

        let items = recent(&db, vid, DEFAULT_LIMIT).await.unwrap();
        let summaries: Vec<&str> = items.iter().map(|i| i.summary.as_str()).collect();
        assert_eq!(
            summaries,
            [
                "After the trip",
                "Odometer reading: 50000",
                "Service on 2026-01-10"
            ]
        );
        let notes: Vec<Option<&str>> = items.iter().map(|i| i.reading_notes.as_deref()).collect();
        assert_eq!(notes, [Some("After the trip"), None, None]);
        let json = serde_json::to_value(&items[0]).unwrap();
        assert_eq!(json.get("reading_notes"), None);
        assert_eq!(json["summary"], "After the trip");
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
        assert_eq!(items[0].kind, ActivityKind::Service);
        assert_eq!(items[0].mileage, Some(48_000));
    }

    #[tokio::test]
    async fn a_manual_log_cannot_claim_a_service_as_its_source() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let err = mileage_svc::create(
            &db,
            vid,
            NewMileageEntry {
                mileage: 60_000,
                recorded_at: Some(date("2026-04-01 10:00:00")),
                source: Some(" Service".into()),
                notes: None,
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "source: a reading from a service is recorded with the service"
        );
        crate::test_support::assert_empty(&recent(&db, vid, DEFAULT_LIMIT).await.unwrap());
    }

    #[tokio::test]
    async fn deleting_a_service_removes_its_auto_log_from_the_feed() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = svc_svc::create(&db, vid, service("2026-01-10", "Brakes", Some(48_000)))
            .await
            .unwrap();

        svc_svc::delete(&db, vid, created.record.id, svc_svc::UNDO)
            .await
            .unwrap();

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
        assert_eq!(items[0].date, date("2026-01-05"));
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
}
