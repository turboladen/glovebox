//! A vehicle's service history as a page prints it for a buyer: every
//! service from the first to the last, then the parts on the car.

use chrono::NaiveDate;
use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{part::PartStatus, vehicle},
    error::DomainResult,
    inputs::part::PartFilter,
    services::{
        part::{self, PartView},
        reminders,
        service_record::{self, ServiceRecordWithLinks},
        vehicle as vehicle_svc,
    },
};

/// Everything done to a vehicle, and what is fitted to it.
#[derive(Debug, Serialize)]
pub struct VehicleHistory {
    pub vehicle: vehicle::Model,
    /// The year, make, model and trim, as [`vehicle_svc::description`] writes
    /// them.
    pub description: Option<String>,
    /// Every service of the vehicle, oldest first. Services of one day keep
    /// the order they were recorded in.
    pub services: Vec<ServiceRecordWithLinks>,
    /// The parts whose status is `installed`, in the order they went on the
    /// car, with the ones that record no install date last. A purchased,
    /// replaced or returned part is not on the car and is left out.
    pub installed_parts: Vec<PartView>,
    /// The day of the first service, or none when there is no service.
    pub first_service_on: Option<NaiveDate>,
    /// The latest day any service ran to, which is its `end_date` when it
    /// has one and its `service_date` otherwise. It is none when there is no
    /// service.
    pub last_service_on: Option<NaiveDate>,
    /// The lowest odometer reading any service records.
    pub lowest_mileage: Option<i32>,
    /// The highest odometer reading any service records.
    pub highest_mileage: Option<i32>,
}

impl VehicleHistory {
    /// The days the services span, such as `2024-03-01 to 2026-10-01`, or one
    /// day when they share it. It is none when there is no service.
    #[must_use]
    pub fn dates_text(&self) -> Option<String> {
        let (first, last) = self.first_service_on.zip(self.last_service_on)?;
        Some(span(super::date_label(first), &super::date_label(last)))
    }

    /// The odometer readings the services span, such as
    /// `52,000 mi to 61,200 mi`, or one reading when they share it. It is
    /// none when no service records a reading.
    #[must_use]
    pub fn miles_text(&self) -> Option<String> {
        let (low, high) = self.lowest_mileage.zip(self.highest_mileage)?;
        Some(span(
            reminders::judged_at(low, false),
            &reminders::judged_at(high, false),
        ))
    }
}

// Two ends that read the same are one value.
fn span(from: String, to: &str) -> String {
    if from == to {
        from
    } else {
        format!("{from} to {to}")
    }
}

/// Reads the service history of vehicle `vehicle_id`. An archived vehicle
/// reads like any other.
///
/// # Errors
/// `NotFound` for a vehicle that does not exist.
pub async fn for_vehicle(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<VehicleHistory> {
    let vehicle = vehicle_svc::require(db, vehicle_id).await?;
    let mut services = service_record::list(db, vehicle_id).await?;
    services.sort_by_key(|s| (s.record.service_date, s.record.id));
    let filter = PartFilter {
        status: Some(PartStatus::Installed),
    };
    let mut installed_parts = part::list(db, vehicle_id, filter).await?;
    installed_parts.sort_by_key(|p| {
        let on = p.part.installed_date;
        (on.is_none(), on, p.part.id)
    });
    let miles = || services.iter().filter_map(|s| s.record.mileage);
    Ok(VehicleHistory {
        description: vehicle_svc::description(&vehicle),
        first_service_on: services.first().map(|s| s.record.service_date),
        last_service_on: services
            .iter()
            .map(|s| s.record.end_date.unwrap_or(s.record.service_date))
            .max(),
        lowest_mileage: miles().min(),
        highest_mileage: miles().max(),
        vehicle,
        services,
        installed_parts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::{
            part as part_entity, service_record::Performer, service_record_line_item::WorkKind,
        },
        inputs::{
            part::{Install, NewFitting, NewPart},
            service_record::{NewLineItem, NewServiceRecord},
            shop::ShopChoice,
        },
        test_support::{add_shop, assert_empty, date, seed_vehicle, test_db},
    };

    async fn service(db: &DatabaseConnection, vehicle_id: i32, input: NewServiceRecord) -> i32 {
        let created = service_record::create(db, vehicle_id, input).await.unwrap();
        created.record.id
    }

    fn on(day: &str, description: &str) -> NewServiceRecord {
        NewServiceRecord {
            service_date: date(day),
            description: Some(description.into()),
            ..Default::default()
        }
    }

    async fn add_part(db: &DatabaseConnection, vehicle_id: i32, name: &str, fitting: NewFitting) {
        let input = NewPart {
            name: name.into(),
            fitting,
            ..Default::default()
        };
        part::create(db, vehicle_id, input).await.unwrap();
    }

    fn installed(day: &str, odometer: i32) -> NewFitting {
        NewFitting::Installed(Install {
            date: Some(date(day)),
            odometer: Some(odometer),
            ..Install::default()
        })
    }

    fn descriptions(history: &VehicleHistory) -> Vec<&str> {
        history
            .services
            .iter()
            .filter_map(|s| s.record.description.as_deref())
            .collect()
    }

    #[tokio::test]
    async fn services_read_oldest_first_with_their_shop_lines_and_span() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let smith = add_shop(&db, "Smith Auto").await;
        service(
            &db,
            v,
            NewServiceRecord {
                mileage: Some(61_200),
                total_cost_cents: Some(50_000),
                shop: Some(ShopChoice::Id(smith)),
                line_items: Some(vec![NewLineItem {
                    description: "Rear pads".into(),
                    work_kind: WorkKind::Maintenance,
                    paid_by: None,
                    quantity: None,
                    unit_cost_cents: None,
                    cost_cents: Some(25_000),
                }]),
                ..on("2026-10-01", "Brakes")
            },
        )
        .await;
        service(
            &db,
            v,
            NewServiceRecord {
                mileage: Some(52_000),
                ..on("2024-03-01", "First oil change")
            },
        )
        .await;
        // Two services of one day keep the order they were recorded in.
        service(&db, v, on("2025-06-15", "Alignment")).await;
        service(&db, v, on("2025-06-15", "Tires")).await;

        let got = for_vehicle(&db, v).await.unwrap();
        assert_eq!(
            descriptions(&got),
            ["First oil change", "Alignment", "Tires", "Brakes"]
        );
        let brakes = &got.services[3];
        assert_eq!(brakes.shop_name.as_deref(), Some("Smith Auto"));
        assert_eq!(brakes.record.total_cost_cents, Some(50_000));
        assert_eq!(brakes.line_items.len(), 1);
        assert_eq!(brakes.line_items[0].description, "Rear pads");
        assert_eq!(brakes.line_items[0].work_kind, WorkKind::Maintenance);
        assert_eq!(
            got.dates_text().as_deref(),
            Some("2024-03-01 to 2026-10-01")
        );
        assert_eq!(got.miles_text().as_deref(), Some("52,000 mi to 61,200 mi"));
    }

    #[tokio::test]
    async fn a_self_performed_service_with_no_total_names_no_shop_and_no_amount() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        service(
            &db,
            v,
            NewServiceRecord {
                performed_by: Some(Performer::Owner),
                mileage: Some(58_000),
                ..on("2026-05-02", "Cabin filter")
            },
        )
        .await;
        let got = for_vehicle(&db, v).await.unwrap();
        let only = &got.services[0];
        assert_eq!(only.record.performed_by, Performer::Owner);
        assert_eq!(only.shop_name, None);
        assert_eq!(only.record.total_cost_cents, None);
        assert_empty(&only.line_items);
        // One service spans one day and one reading.
        assert_eq!(got.dates_text().as_deref(), Some("2026-05-02"));
        assert_eq!(got.miles_text().as_deref(), Some("58,000 mi"));
    }

    #[tokio::test]
    async fn the_span_ends_on_the_last_day_a_service_ran_to() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        service(&db, v, on("2026-10-03", "Tires")).await;
        service(
            &db,
            v,
            NewServiceRecord {
                end_date: Some(date("2026-10-05")),
                ..on("2026-10-01", "Body work")
            },
        )
        .await;
        let got = for_vehicle(&db, v).await.unwrap();
        assert_eq!(descriptions(&got), ["Body work", "Tires"]);
        assert_eq!(got.last_service_on, Some(date("2026-10-05")));
        assert_eq!(
            got.dates_text().as_deref(),
            Some("2026-10-01 to 2026-10-05")
        );
    }

    #[tokio::test]
    async fn only_the_parts_on_the_car_are_listed_in_the_order_they_went_on() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        add_part(&db, v, "Wipers", installed("2026-02-01", 57_000)).await;
        add_part(&db, v, "Battery", installed("2025-01-10", 54_000)).await;
        add_part(
            &db,
            v,
            "Floor mats",
            NewFitting::Installed(Install::default()),
        )
        .await;
        add_part(&db, v, "Spare bulbs", NewFitting::Purchased).await;
        let old = Install {
            date: Some(date("2024-01-01")),
            ..Install::default()
        };
        add_part(&db, v, "Old battery", NewFitting::Replaced(old)).await;
        // Only a need's return sets `returned`, so the row is written as that
        // return leaves it.
        part_entity::ActiveModel {
            vehicle_id: Set(v),
            name: Set("Wrong filter".into()),
            status: Set(PartStatus::Returned),
            returned_on: Set(Some(date("2026-03-01"))),
            return_reason: Set(Some("Did not fit".into())),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let got = for_vehicle(&db, v).await.unwrap();
        let names: Vec<&str> = got
            .installed_parts
            .iter()
            .map(|p| p.part.name.as_str())
            .collect();
        assert_eq!(names, ["Battery", "Wipers", "Floor mats"]);
        assert_eq!(got.installed_parts[0].part.installed_odometer, Some(54_000));
    }

    #[tokio::test]
    async fn another_vehicles_records_never_appear() {
        let db = test_db().await;
        let mine = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        service(&db, other, on("2026-01-05", "Their service")).await;
        add_part(&db, other, "Their part", installed("2026-01-05", 10_000)).await;
        service(&db, mine, on("2026-02-05", "My service")).await;

        let got = for_vehicle(&db, mine).await.unwrap();
        assert_eq!(descriptions(&got), ["My service"]);
        assert_empty(&got.installed_parts);
        assert_eq!(got.miles_text(), None);
    }

    #[tokio::test]
    async fn a_vehicle_with_no_records_has_an_empty_history() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let got = for_vehicle(&db, v).await.unwrap();
        assert_empty(&got.services);
        assert_empty(&got.installed_parts);
        assert_eq!(got.dates_text(), None);
        assert_eq!(got.miles_text(), None);
    }

    #[tokio::test]
    async fn a_missing_vehicle_is_not_found() {
        let db = test_db().await;
        let error = for_vehicle(&db, 999).await.unwrap_err().to_string();
        assert_eq!(error, "Vehicle 999 not found");
    }
}
