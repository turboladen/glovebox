use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{part, service_record, vehicle},
    error::{DomainError, DomainResult},
};

/// The `parts.supplied_by` value of a part the shop sourced and billed for.
const SUPPLIED_BY_SHOP: &str = "shop";

#[derive(Serialize)]
pub struct CostSummary {
    pub vehicle_id: i32,
    /// The sum of every service's `total_cost_cents`. Parts the shop supplied
    /// are billed on those invoices, so they are already inside this figure.
    pub service_totals_cents: i64,
    /// The cost of parts the owner bought outside any invoice (`supplied_by`
    /// `self`). It is not part of any service total.
    pub self_supplied_parts_cents: i64,
    /// `service_totals_cents + self_supplied_parts_cents`.
    pub total_cost_cents: i64,
    /// What was actually paid out of pocket: self-paid service totals plus
    /// the parts the owner bought (parts have no payer). Sums with
    /// `covered_cents` to `total_cost_cents`.
    pub out_of_pocket_cents: i64,
    /// Service totals paid by someone else (`paid_by != "self"`).
    pub covered_cents: i64,
    /// The sum of the labor subtotals entered on services, or `None` when no
    /// service has one.
    pub labor_subtotals_cents: Option<i64>,
    /// The sum of the parts subtotals entered on services, or `None` when no
    /// service has one.
    pub parts_subtotals_cents: Option<i64>,
    pub service_count: usize,
    pub part_count: usize,
    /// The latest service mileage minus the purchase mileage, or `None` when
    /// either is unknown or the difference is not positive.
    pub miles_driven: Option<i64>,
    /// `total_cost_cents` per 1000 of `miles_driven`, rounded to the nearest
    /// cent; `None` when `miles_driven` is.
    pub cost_per_1000_miles_cents: Option<i64>,
    pub monthly_costs: Vec<MonthlyCost>,
    /// Self-supplied part costs with neither a purchase nor an install date,
    /// which no month holds. The months plus this figure add up to
    /// `total_cost_cents`.
    pub unbucketed_parts_cents: i64,
}

#[derive(Serialize)]
pub struct MonthlyCost {
    pub month: String,
    pub service_cost_cents: i64,
    pub parts_cost_cents: i64,
    pub out_of_pocket_cents: i64,
    pub covered_cents: i64,
    pub total_cents: i64,
}

fn sum_present(values: impl Iterator<Item = Option<i32>>) -> Option<i64> {
    values
        .flatten()
        .map(i64::from)
        .fold(None, |acc, v| Some(acc.unwrap_or(0) + v))
}

fn month_of(date: &str) -> String {
    date.get(..7).unwrap_or(date).to_string()
}

/// Aggregate cost totals for a vehicle. Currency is always `i32`/`i64` cents;
/// integer arithmetic only (no float division).
pub async fn summary(db: &impl ConnectionTrait, vehicle_id: i32) -> DomainResult<CostSummary> {
    let vehicle = vehicle::Entity::find_by_id(vehicle_id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Vehicle {vehicle_id} not found")))?;

    let services = service_record::Entity::find()
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .order_by_asc(service_record::Column::ServiceDate)
        .all(db)
        .await?;
    let parts = part::Entity::find()
        .filter(part::Column::VehicleId.eq(vehicle_id))
        .all(db)
        .await?;

    let service_totals_cents =
        sum_present(services.iter().map(|s| s.total_cost_cents)).unwrap_or(0);
    // A part the shop supplied is billed on its service's invoice, so its cost
    // is already inside that service's total. Only owner-bought parts are
    // spend beyond the invoices.
    let self_parts: Vec<&part::Model> = parts
        .iter()
        .filter(|p| p.supplied_by != SUPPLIED_BY_SHOP)
        .collect();
    let self_supplied_parts_cents =
        sum_present(self_parts.iter().map(|p| p.cost_cents)).unwrap_or(0);
    let total_cost_cents = service_totals_cents + self_supplied_parts_cents;

    // Services paid by someone else are covered; self-paid services and
    // owner-bought parts (no payer on parts) are out of pocket.
    let covered_cents: i64 = services
        .iter()
        .filter(|s| s.paid_by != "self")
        .filter_map(|s| s.total_cost_cents)
        .map(i64::from)
        .sum();

    // The latest service mileage stands in for the current odometer.
    let miles_driven = vehicle.purchase_mileage.and_then(|purchase| {
        let latest = services.iter().rev().find_map(|s| s.mileage)?;
        let miles = i64::from(latest) - i64::from(purchase);
        (miles > 0).then_some(miles)
    });
    let cost_per_1000_miles_cents =
        miles_driven.map(|miles| (total_cost_cents * 1000 + miles / 2) / miles);

    // Month buckets: (service cents, parts cents, covered cents).
    let mut monthly_map: std::collections::BTreeMap<String, (i64, i64, i64)> =
        std::collections::BTreeMap::new();
    for svc in &services {
        let entry = monthly_map
            .entry(month_of(&svc.service_date))
            .or_insert((0, 0, 0));
        let cents = i64::from(svc.total_cost_cents.unwrap_or(0));
        entry.0 += cents;
        if svc.paid_by != "self" {
            entry.2 += cents;
        }
    }
    let mut unbucketed_parts_cents = 0;
    for p in self_parts {
        let cents = i64::from(p.cost_cents.unwrap_or(0));
        match p.purchase_date.as_ref().or(p.installed_date.as_ref()) {
            Some(date) => monthly_map.entry(month_of(date)).or_insert((0, 0, 0)).1 += cents,
            None => unbucketed_parts_cents += cents,
        }
    }
    let monthly_costs: Vec<MonthlyCost> = monthly_map
        .into_iter()
        .map(|(month, (svc, prt, covered))| MonthlyCost {
            month,
            service_cost_cents: svc,
            parts_cost_cents: prt,
            out_of_pocket_cents: svc + prt - covered,
            covered_cents: covered,
            total_cents: svc + prt,
        })
        .collect();

    Ok(CostSummary {
        vehicle_id,
        service_totals_cents,
        self_supplied_parts_cents,
        total_cost_cents,
        out_of_pocket_cents: total_cost_cents - covered_cents,
        covered_cents,
        labor_subtotals_cents: sum_present(services.iter().map(|s| s.labor_cost_cents)),
        parts_subtotals_cents: sum_present(services.iter().map(|s| s.parts_cost_cents)),
        service_count: services.len(),
        part_count: parts.len(),
        miles_driven,
        cost_per_1000_miles_cents,
        monthly_costs,
        unbucketed_parts_cents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{VehicleFixture, test_db};

    async fn seed_vehicle(db: &impl ConnectionTrait, purchase_mileage: Option<i32>) -> i32 {
        let mut fixture = VehicleFixture::new();
        if let Some(m) = purchase_mileage {
            fixture = fixture.purchase_mileage(m);
        }
        fixture.insert_id(db).await
    }

    #[tokio::test]
    async fn empty_vehicle_has_zero_totals() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.total_cost_cents, 0);
        assert_eq!(s.service_count, 0);
        assert_eq!(s.part_count, 0);
        assert_eq!(s.cost_per_1000_miles_cents, None);
        assert_eq!(s.miles_driven, None);
        assert_eq!(s.labor_subtotals_cents, None);
        assert_eq!(s.parts_subtotals_cents, None);
        assert!(s.monthly_costs.is_empty());
    }

    #[tokio::test]
    async fn aggregates_services_and_parts_with_cost_per_mile() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, Some(1000)).await;

        // Two services on the vehicle.
        service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set("2024-01-15".into()),
            mileage: Set(Some(2000)),
            total_cost_cents: Set(Some(10_000)),
            labor_cost_cents: Set(Some(4_000)),
            parts_cost_cents: Set(Some(6_000)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set("2024-02-20".into()),
            mileage: Set(Some(3000)),
            total_cost_cents: Set(Some(5_000)),
            labor_cost_cents: Set(Some(1_000)),
            parts_cost_cents: Set(Some(4_000)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        // A separately purchased part not counted in service records.
        part::ActiveModel {
            vehicle_id: Set(vid),
            name: Set("Filter".into()),
            status: Set("purchased".into()),
            cost_cents: Set(Some(20_000)),
            purchase_date: Set(Some("2024-01-10".into())),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.service_count, 2);
        assert_eq!(s.part_count, 1);
        assert_eq!(s.service_totals_cents, 15_000);
        assert_eq!(s.labor_subtotals_cents, Some(5_000));
        assert_eq!(s.parts_subtotals_cents, Some(10_000));
        assert_eq!(s.self_supplied_parts_cents, 20_000);
        // total = service totals (15_000) + self-supplied parts (20_000)
        assert_eq!(s.total_cost_cents, 35_000);
        // 35_000 cents over (3000 - 1000) miles is 17_500 cents per 1000 miles
        assert_eq!(s.miles_driven, Some(2_000));
        assert_eq!(s.cost_per_1000_miles_cents, Some(17_500));
        // two service months + one part month (Jan overlaps a service month)
        assert_eq!(s.monthly_costs.len(), 2);
        // No payer info seeded -> everything defaults to out-of-pocket.
        assert_eq!(s.covered_cents, 0);
        assert_eq!(s.out_of_pocket_cents, s.total_cost_cents);
    }

    #[tokio::test]
    async fn splits_out_of_pocket_vs_covered() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;

        // $100 self-paid service in January.
        service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set("2024-01-15".into()),
            total_cost_cents: Set(Some(10_000)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        // $150 insurance-paid service in February.
        service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set("2024-02-20".into()),
            total_cost_cents: Set(Some(15_000)),
            paid_by: Set("insurance".into()),
            payer_note: Set(Some("claim #123".into())),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        // $20 separately purchased part in January (no payer: out-of-pocket).
        part::ActiveModel {
            vehicle_id: Set(vid),
            name: Set("Wiper blades".into()),
            status: Set("purchased".into()),
            cost_cents: Set(Some(2_000)),
            purchase_date: Set(Some("2024-01-10".into())),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let s = summary(&db, vid).await.unwrap();
        // Existing formulas unchanged: total = service totals + extra parts.
        assert_eq!(s.service_totals_cents, 25_000);
        assert_eq!(s.total_cost_cents, 27_000);
        // Split: self services + parts are out-of-pocket; the rest is covered.
        assert_eq!(s.out_of_pocket_cents, 12_000);
        assert_eq!(s.covered_cents, 15_000);
        assert_eq!(s.out_of_pocket_cents + s.covered_cents, s.total_cost_cents);

        // Monthly split mirrors the same rule per bucket.
        assert_eq!(s.monthly_costs.len(), 2);
        let jan = &s.monthly_costs[0];
        assert_eq!(jan.month, "2024-01");
        assert_eq!(jan.out_of_pocket_cents, 12_000);
        assert_eq!(jan.covered_cents, 0);
        assert_eq!(jan.total_cents, 12_000);
        let feb = &s.monthly_costs[1];
        assert_eq!(feb.month, "2024-02");
        assert_eq!(feb.out_of_pocket_cents, 0);
        assert_eq!(feb.covered_cents, 15_000);
        assert_eq!(feb.total_cents, 15_000);
    }

    #[tokio::test]
    async fn a_shop_supplied_part_is_not_counted_on_top_of_its_invoice() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let service_id = service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set("2024-03-01".into()),
            total_cost_cents: Set(Some(20_000)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        for (name, supplied_by, cents) in [("Plugs", "shop", 6_000), ("Pads", "self", 9_000)] {
            part::ActiveModel {
                vehicle_id: Set(vid),
                name: Set(name.into()),
                status: Set("installed".into()),
                supplied_by: Set(supplied_by.into()),
                cost_cents: Set(Some(cents)),
                purchase_date: Set(Some("2024-03-01".into())),
                installed_service_id: Set(Some(service_id)),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }

        // The invoice total already holds the shop's part; only the pads the
        // owner bought are spend beyond it.
        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.self_supplied_parts_cents, 9_000);
        assert_eq!(s.total_cost_cents, 29_000);
        assert_eq!(s.out_of_pocket_cents, 29_000);
        assert_eq!(s.monthly_costs[0].parts_cost_cents, 9_000);
        assert_eq!(s.monthly_costs[0].total_cents, 29_000);

        // The same holds when the invoice also states its parts portion.
        let mut active: service_record::ActiveModel =
            service_record::Entity::find_by_id(service_id)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .into();
        active.parts_cost_cents = Set(Some(6_000));
        active.update(&db).await.unwrap();
        assert_eq!(summary(&db, vid).await.unwrap().total_cost_cents, 29_000);
    }

    async fn insert_service(
        db: &impl ConnectionTrait,
        vid: i32,
        date: &str,
        total: Option<i32>,
        paid_by: &str,
    ) -> i32 {
        service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set(date.into()),
            total_cost_cents: Set(total),
            paid_by: Set(paid_by.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn insert_part(
        db: &impl ConnectionTrait,
        vid: i32,
        supplied_by: &str,
        cents: i32,
        purchase_date: Option<&str>,
        installed_date: Option<&str>,
    ) {
        part::ActiveModel {
            vehicle_id: Set(vid),
            name: Set("Part".into()),
            status: Set("installed".into()),
            supplied_by: Set(supplied_by.into()),
            cost_cents: Set(Some(cents)),
            purchase_date: Set(purchase_date.map(Into::into)),
            installed_date: Set(installed_date.map(Into::into)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_shop_part_inside_an_invoice_total_is_not_added_again() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        insert_service(&db, vid, "2026-11-03", Some(56_900), "self").await;
        insert_part(&db, vid, "shop", 2_200, None, Some("2026-11-03")).await;
        insert_part(&db, vid, "self", 8_900, Some("2026-10-20"), None).await;

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.service_totals_cents, 56_900);
        assert_eq!(s.self_supplied_parts_cents, 8_900);
        assert_eq!(s.total_cost_cents, 65_800);
    }

    #[tokio::test]
    async fn the_headline_is_the_service_totals_plus_the_self_supplied_parts() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        insert_service(&db, vid, "2026-11-03", Some(57_500), "self").await;
        insert_service(&db, vid, "2026-11-10", None, "self").await;
        insert_service(&db, vid, "2026-11-12", None, "self").await;
        insert_part(&db, vid, "shop", 2_200, None, Some("2026-11-03")).await;
        // No purchase date, so the install date places it in a month.
        insert_part(&db, vid, "self", 8_900, None, Some("2026-10-30")).await;

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.total_cost_cents, 66_400);
        assert_eq!(
            s.total_cost_cents,
            s.service_totals_cents + s.self_supplied_parts_cents
        );
        assert_eq!(s.unbucketed_parts_cents, 0);
        let months: i64 = s.monthly_costs.iter().map(|m| m.total_cents).sum();
        assert_eq!(months, s.total_cost_cents);
        let oct = s
            .monthly_costs
            .iter()
            .find(|m| m.month == "2026-10")
            .unwrap();
        assert_eq!(oct.parts_cost_cents, 8_900);
    }

    #[tokio::test]
    async fn a_part_with_no_date_is_reported_as_unbucketed() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        insert_service(&db, vid, "2026-11-03", Some(10_000), "self").await;
        insert_part(&db, vid, "self", 3_000, None, None).await;

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.unbucketed_parts_cents, 3_000);
        assert_eq!(s.total_cost_cents, 13_000);
        let months: i64 = s.monthly_costs.iter().map(|m| m.total_cents).sum();
        assert_eq!(months + s.unbucketed_parts_cents, s.total_cost_cents);
    }

    #[tokio::test]
    async fn subtotals_nobody_entered_are_null_and_manufacturer_pays_count_as_covered() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        insert_service(&db, vid, "2026-11-03", Some(10_000), "self").await;
        insert_service(&db, vid, "2026-11-04", Some(5_000), "manufacturer").await;

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.labor_subtotals_cents, None);
        assert_eq!(s.parts_subtotals_cents, None);
        assert_eq!(s.covered_cents, 5_000);
        assert_eq!(s.out_of_pocket_cents, 10_000);
    }

    #[tokio::test]
    async fn cost_per_1000_miles_is_null_without_a_positive_mileage() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, Some(5_000)).await;
        service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set("2026-11-03".into()),
            mileage: Set(Some(5_000)),
            total_cost_cents: Set(Some(10_000)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.miles_driven, None);
        assert_eq!(s.cost_per_1000_miles_cents, None);
    }
}
