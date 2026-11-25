//! One vehicle as its page shows it: the record with its odometer, what it
//! has cost and what was last recorded about it.

use chrono::NaiveDate;
use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::vehicle,
    error::DomainResult,
    services::{
        activity::{self, ActivityItem},
        costs::{self, CostSummary},
        reminders, vehicle as vehicle_svc,
    },
};

/// A vehicle with what its page prints beside the record.
#[derive(Debug, Serialize)]
pub struct VehicleDetail {
    pub vehicle: vehicle::Model,
    /// The year, make, model and trim, as [`vehicle_svc::description`] writes
    /// them.
    pub description: Option<String>,
    /// The latest logged odometer reading dated no later than the day asked
    /// about, which is never an estimate. It is none when no reading is
    /// logged.
    pub odometer: Option<i32>,
    /// The day `odometer` was read. It is set exactly when `odometer` is.
    pub odometer_read_on: Option<NaiveDate>,
    /// The mileage projected from `odometer` to the day asked about. It is
    /// none for an archived vehicle, for one with no logged reading, for one
    /// read that day and when the projection equals `odometer`.
    pub estimated_mileage: Option<i32>,
    pub costs: CostSummary,
    /// The latest [`activity::DEFAULT_LIMIT`] services, incidents and
    /// odometer readings, newest first.
    pub recent_activity: Vec<ActivityItem>,
}

impl VehicleDetail {
    /// The odometer reading with the day it was read, such as
    /// `61,000 mi on 2026-11-01`, or none when no reading is logged.
    #[must_use]
    pub fn odometer_text(&self) -> Option<String> {
        let (miles, day) = self.odometer.zip(self.odometer_read_on)?;
        Some(reminders::reading_text(miles, day))
    }

    /// The projected mileage marked as an estimate, such as
    /// `about 61,200 mi (estimated)`, or none when there is no estimate.
    #[must_use]
    pub fn estimate_text(&self) -> Option<String> {
        self.estimated_mileage
            .map(|miles| reminders::judged_at(miles, true))
    }

    /// The miles the cost records cover and the day they start, such as
    /// `8,400 mi since 2025-01-10`, or none when the records cover no miles.
    #[must_use]
    pub fn records_cover(&self) -> Option<String> {
        let miles = i32::try_from(self.costs.miles_covered?).ok()?;
        let since = self.costs.history_starts_on?;
        Some(format!(
            "{} since {}",
            reminders::judged_at(miles, false),
            super::date_label(since)
        ))
    }
}

/// Reads vehicle `vehicle_id` with its odometer as of `today`, its cost
/// summary and its recent activity. An archived vehicle reads like any other.
///
/// # Errors
/// `NotFound` for a vehicle that does not exist.
pub async fn get(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    today: NaiveDate,
) -> DomainResult<VehicleDetail> {
    let vehicle = vehicle_svc::require(db, vehicle_id).await?;
    let reading = reminders::last_reading(db, vehicle_id, today).await?;
    // A sold car is driven by someone else, so a projection of its mileage
    // says nothing about the records. A reading taken today needs none, and
    // a projection equal to the reading (one reading gives no rate) adds
    // nothing to it.
    let estimated_mileage = match reading {
        Some((read, _)) if vehicle.archived_at.is_none() => {
            let (miles, _, _, is_estimate) =
                reminders::estimate_mileage(db, vehicle_id, &vehicle, today).await?;
            (is_estimate && miles != read).then_some(miles)
        }
        _ => None,
    };
    Ok(VehicleDetail {
        description: vehicle_svc::description(&vehicle),
        odometer: reading.map(|(miles, _)| miles),
        odometer_read_on: reading.map(|(_, day)| day),
        estimated_mileage,
        costs: costs::summary(db, vehicle_id).await?,
        recent_activity: activity::recent(db, vehicle_id, activity::DEFAULT_LIMIT).await?,
        vehicle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inputs::{
            mileage::NewMileageEntry, service_record::NewServiceRecord, vehicle::VehicleSale,
        },
        services::{mileage, service_record},
        test_support::{VehicleFixture, assert_empty, date, seed_vehicle, test_db},
    };

    fn today() -> NaiveDate {
        date("2026-11-13")
    }

    async fn log(db: &DatabaseConnection, vehicle_id: i32, miles: i32, on: &str) {
        let entry = NewMileageEntry {
            mileage: miles,
            recorded_at: Some(date(on)),
            source: None,
            notes: None,
        };
        mileage::create(db, vehicle_id, entry).await.unwrap();
    }

    #[tokio::test]
    async fn get_reads_the_last_reading_the_estimate_the_costs_and_the_activity() {
        let db = test_db().await;
        let v = VehicleFixture::new()
            .make("Volkswagen")
            .insert_id(&db)
            .await;
        log(&db, v, 60_000, "2026-08-05").await;
        log(&db, v, 61_000, "2026-11-03").await;
        service_record::create(
            &db,
            v,
            NewServiceRecord {
                service_date: date("2026-09-01"),
                description: Some("Oil change".into()),
                total_cost_cents: Some(12_000),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let got = get(&db, v, today()).await.unwrap();
        assert_eq!(got.description.as_deref(), Some("Volkswagen"));
        assert_eq!(
            got.odometer_text().as_deref(),
            Some("61,000 mi on 2026-11-03")
        );
        // The readings climb 1,000 miles in 90 days, and the last one is ten
        // days old.
        assert_eq!(got.estimated_mileage, Some(61_111));
        assert_eq!(
            got.estimate_text().as_deref(),
            Some("about 61,111 mi (estimated)")
        );
        assert_eq!(got.costs.total_cost_cents, 12_000);
        assert_eq!(
            got.records_cover().as_deref(),
            Some("1,000 mi since 2026-08-05")
        );
        let summaries: Vec<&str> = got
            .recent_activity
            .iter()
            .map(|a| a.summary.as_str())
            .collect();
        assert_eq!(
            summaries,
            [
                "Odometer reading: 61000",
                "Oil change",
                "Odometer reading: 60000"
            ]
        );
    }

    #[tokio::test]
    async fn a_vehicle_with_no_records_has_no_odometer_and_no_estimate() {
        let db = test_db().await;
        let v = VehicleFixture::new()
            .purchase_mileage(40_000)
            .insert_id(&db)
            .await;
        let got = get(&db, v, today()).await.unwrap();
        assert_eq!(got.description, None);
        assert_eq!(got.odometer_text(), None);
        assert_eq!(got.estimate_text(), None);
        assert_eq!(got.records_cover(), None);
        assert_empty(&got.recent_activity);
    }

    #[tokio::test]
    async fn a_reading_taken_today_has_no_estimate() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        log(&db, v, 60_000, "2026-08-05").await;
        log(&db, v, 61_000, "2026-11-13").await;
        let got = get(&db, v, today()).await.unwrap();
        assert_eq!(got.odometer, Some(61_000));
        assert_eq!(got.estimated_mileage, None);
    }

    // One reading and no purchase mileage give no rate to project at.
    #[tokio::test]
    async fn one_old_reading_has_no_estimate() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        log(&db, v, 61_000, "2026-11-03").await;
        let got = get(&db, v, today()).await.unwrap();
        assert_eq!(
            got.odometer_text().as_deref(),
            Some("61,000 mi on 2026-11-03")
        );
        assert_eq!(got.estimated_mileage, None);
        assert_eq!(got.estimate_text(), None);
    }

    #[tokio::test]
    async fn an_archived_vehicle_reads_with_its_reading_and_no_estimate() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        log(&db, v, 60_000, "2026-08-05").await;
        log(&db, v, 61_000, "2026-11-03").await;
        let sale = VehicleSale {
            sold_date: Some(date("2026-11-05")),
            ..Default::default()
        };
        vehicle_svc::archive(&db, v, sale).await.unwrap();
        let got = get(&db, v, today()).await.unwrap();
        assert!(got.vehicle.archived_at.is_some());
        assert_eq!(got.vehicle.sold_date, Some(date("2026-11-05")));
        assert_eq!(got.odometer, Some(61_000));
        assert_eq!(got.estimated_mileage, None);
    }

    #[tokio::test]
    async fn a_missing_vehicle_is_not_found() {
        let db = test_db().await;
        let error = get(&db, 999, today()).await.unwrap_err().to_string();
        assert_eq!(error, "Vehicle 999 not found");
    }
}
