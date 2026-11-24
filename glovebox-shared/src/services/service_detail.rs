//! One service as a page shows it: the service read with the names and
//! amounts the read gives only as ids or leaves to the cost summary.

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::maintenance_schedule_item,
    error::DomainResult,
    money::format_cents,
    services::{
        costs, schedule,
        service_record::{self, ServiceRecordWithLinks},
        vehicle,
    },
};

/// A service with what its page prints beside the read.
#[derive(Debug, Serialize)]
pub struct ServiceDetail {
    /// The vehicle's name.
    pub vehicle_name: String,
    pub service: ServiceRecordWithLinks,
    /// The schedule items behind the service's `schedule_item_ids`, by name
    /// with ASCII case ignored.
    pub schedule_items: Vec<ScheduleItemRef>,
    /// The owner's share of the total; `None` when the service has no total.
    pub out_of_pocket_cents: Option<i64>,
    /// What others paid of the total; `None` when the service has no total.
    pub covered_cents: Option<i64>,
    /// Says by how much the lines differ from the total, in dollars. Present
    /// exactly when the service's `note` is.
    pub lines_note: Option<String>,
}

/// A schedule item a service counts toward.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScheduleItemRef {
    pub id: i32,
    pub name: String,
}

/// Reads service `id` of `vehicle_id` with its vehicle's name, the names of
/// its schedule items and the split of its total.
///
/// # Errors
/// `NotFound` for a service that does not exist, and the same `NotFound` for
/// one that belongs to another vehicle.
pub async fn get(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<ServiceDetail> {
    let service = service_record::get(db, vehicle_id, id).await?;
    let vehicle_name = vehicle::require(db, vehicle_id).await?.name;
    let schedule_items = schedule_items(db, &service.schedule_item_ids).await?;
    let out_of_pocket_cents = costs::out_of_pocket(db, std::slice::from_ref(&service.record))
        .await?
        .remove(&id);
    let covered_cents = service
        .record
        .total_cost_cents
        .zip(out_of_pocket_cents)
        .map(|(total, own)| i64::from(total) - own);
    Ok(ServiceDetail {
        vehicle_name,
        lines_note: lines_note(service.record.total_cost_cents, service.lines_total_cents),
        schedule_items,
        out_of_pocket_cents,
        covered_cents,
        service,
    })
}

// The items named by `ids` in one query, ordered as schedule names compare
// (see `schedule::name_key`) and then by id.
async fn schedule_items(
    db: &impl ConnectionTrait,
    ids: &[i32],
) -> DomainResult<Vec<ScheduleItemRef>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut named: Vec<(i32, String)> = maintenance_schedule_item::Entity::find()
        .select_only()
        .column(maintenance_schedule_item::Column::Id)
        .column(maintenance_schedule_item::Column::Name)
        .filter(maintenance_schedule_item::Column::Id.is_in(ids.to_vec()))
        .into_tuple()
        .all(db)
        .await?;
    named.sort_by_cached_key(|(id, name)| (schedule::name_key(name), *id));
    Ok(named
        .into_iter()
        .map(|(id, name)| ScheduleItemRef { id, name })
        .collect())
}

/// Says how the sum of the lines compares with the invoice's total, or
/// `None` when either is missing or they agree.
fn lines_note(total_cents: Option<i32>, lines_cents: Option<i64>) -> Option<String> {
    let (total, lines) = (i64::from(total_cents?), lines_cents?);
    let side = match lines.cmp(&total) {
        std::cmp::Ordering::Less => "less",
        std::cmp::Ordering::Equal => return None,
        std::cmp::Ordering::Greater => "more",
    };
    Some(format!(
        "The lines add up to {}, which is {} {side} than the total.",
        format_cents(lines),
        format_cents((total - lines).abs())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::{service_record::Payer, service_record_line_item::WorkKind},
        inputs::{
            schedule::NewScheduleItem,
            service_record::{NewLineItem, NewServiceRecord},
        },
        test_support::{assert_empty, date, seed_service, seed_vehicle, test_db},
    };

    fn line(description: &str, paid_by: Option<Payer>, cents: i32) -> NewLineItem {
        NewLineItem {
            description: description.into(),
            work_kind: WorkKind::Repair,
            paid_by,
            quantity: None,
            unit_cost_cents: None,
            cost_cents: Some(cents),
        }
    }

    #[tokio::test]
    async fn get_names_the_schedule_items_and_splits_the_total() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let mut items = Vec::new();
        for name in ["Wiper blades", "brake fluid"] {
            let item = NewScheduleItem {
                vehicle_id: Some(v),
                name: name.into(),
                interval_months: Some(24),
                ..Default::default()
            };
            items.push(schedule::create(&db, item).await.unwrap().id);
        }
        let id = service_record::create(
            &db,
            v,
            NewServiceRecord {
                service_date: date("2026-10-01"),
                total_cost_cents: Some(50_000),
                schedule_item_ids: Some(items.clone()),
                line_items: Some(vec![
                    line("Windshield", Some(Payer::Insurance), 20_000),
                    line("Wipers", None, 25_000),
                ]),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .record
        .id;

        let got = get(&db, v, id).await.unwrap();
        assert_eq!(got.vehicle_name, "Car");
        assert_eq!(
            got.schedule_items,
            [
                ScheduleItemRef {
                    id: items[1],
                    name: "brake fluid".into()
                },
                ScheduleItemRef {
                    id: items[0],
                    name: "Wiper blades".into()
                },
            ]
        );
        assert_eq!(got.out_of_pocket_cents, Some(30_000));
        assert_eq!(got.covered_cents, Some(20_000));
        assert_eq!(
            got.lines_note.as_deref(),
            Some("The lines add up to $450.00, which is $50.00 less than the total.")
        );
        assert!(got.service.note.is_some());
    }

    #[tokio::test]
    async fn a_service_with_no_total_has_no_split_and_no_note() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let id = seed_service(&db, v).await;
        let got = get(&db, v, id).await.unwrap();
        assert_empty(&got.schedule_items);
        assert_eq!(got.out_of_pocket_cents, None);
        assert_eq!(got.covered_cents, None);
        assert_eq!(got.lines_note, None);
    }

    // No ownership oracle: a service of another vehicle fails exactly as an
    // id of no service does.
    #[tokio::test]
    async fn another_vehicles_service_reads_as_missing() {
        let db = test_db().await;
        let mine = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let theirs = seed_service(&db, other).await;
        let wrong_vehicle = get(&db, mine, theirs).await.unwrap_err().to_string();
        assert_eq!(wrong_vehicle, format!("Service record {theirs} not found"));
        let missing = get(&db, mine, 999).await.unwrap_err().to_string();
        assert_eq!(missing, "Service record 999 not found");
    }

    #[test]
    fn lines_note_states_the_gap_only_when_the_amounts_differ() {
        assert_eq!(
            lines_note(Some(10_000), Some(12_550)).as_deref(),
            Some("The lines add up to $125.50, which is $25.50 more than the total.")
        );
        assert_eq!(lines_note(Some(10_000), Some(10_000)), None);
        assert_eq!(lines_note(None, Some(10_000)), None);
        assert_eq!(lines_note(Some(10_000), None), None);
    }
}
