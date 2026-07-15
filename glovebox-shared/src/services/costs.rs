use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{part, service_record, vehicle},
    error::{DomainError, DomainResult},
};

#[derive(Serialize)]
pub struct CostSummary {
    pub vehicle_id: i32,
    pub total_service_cost_cents: i64,
    pub total_parts_cost_cents: i64,
    pub total_labor_cost_cents: i64,
    pub total_cost_cents: i64,
    /// What was actually paid out of pocket: self-paid service totals plus
    /// parts (parts have no payer). Sums with `covered_cents` to
    /// `total_cost_cents`.
    pub out_of_pocket_cents: i64,
    /// Service totals paid by someone else (`paid_by != "self"`).
    pub covered_cents: i64,
    pub service_count: usize,
    pub part_count: usize,
    pub cost_per_mile_cents: Option<i64>,
    pub monthly_costs: Vec<MonthlyCost>,
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

/// Aggregate cost totals for a vehicle. Currency is always `i32`/`i64` cents;
/// integer arithmetic only (no float division).
#[allow(clippy::too_many_lines)]
pub async fn summary(db: &impl ConnectionTrait, vehicle_id: i32) -> DomainResult<CostSummary> {
    let vehicle = vehicle::Entity::find_by_id(vehicle_id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Vehicle {vehicle_id} not found")))?;
    let purchase_mileage = vehicle.purchase_mileage;

    let services = service_record::Entity::find()
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .order_by_asc(service_record::Column::ServiceDate)
        .all(db)
        .await?;

    let parts = part::Entity::find()
        .filter(part::Column::VehicleId.eq(vehicle_id))
        .all(db)
        .await?;

    let total_service_cost_cents: i64 = services
        .iter()
        .filter_map(|s| s.total_cost_cents)
        .map(i64::from)
        .sum();

    let total_labor_cost_cents: i64 = services
        .iter()
        .filter_map(|s| s.labor_cost_cents)
        .map(i64::from)
        .sum();

    let total_parts_cost_from_services: i64 = services
        .iter()
        .filter_map(|s| s.parts_cost_cents)
        .map(i64::from)
        .sum();

    let total_parts_purchased: i64 = parts
        .iter()
        .filter_map(|p| p.cost_cents)
        .map(i64::from)
        .sum();

    // Parts cost: use purchased parts total (more accurate than service-reported)
    let total_parts_cost_cents = total_parts_purchased.max(total_parts_cost_from_services);
    // Total: service totals (which include labor + parts on the bill) plus any
    // separately purchased parts not already counted in service records
    let extra_parts_cost = (total_parts_purchased - total_parts_cost_from_services).max(0);
    let total_cost_cents = total_service_cost_cents + extra_parts_cost;

    // Payer split: services paid by someone else are covered; self-paid
    // services and all parts (no payer on parts) are out of pocket.
    let covered_cents: i64 = services
        .iter()
        .filter(|s| s.paid_by != "self")
        .filter_map(|s| s.total_cost_cents)
        .map(i64::from)
        .sum();
    let out_of_pocket_cents = total_cost_cents - covered_cents;

    // Cost per mile: total cost / (current mileage - purchase mileage)
    let cost_per_mile_cents = if let Some(purchase_mi) = purchase_mileage {
        // Find the most recent service mileage as a proxy for current mileage
        let latest_mileage = services.iter().rev().find_map(|s| s.mileage);
        latest_mileage.and_then(|current| {
            let miles_driven = i64::from(current) - i64::from(purchase_mi);
            if miles_driven > 0 {
                Some(total_cost_cents / miles_driven)
            } else {
                None
            }
        })
    } else {
        None
    };

    // Monthly cost breakdown: (service cents, parts cents, covered cents)
    let mut monthly_map: std::collections::BTreeMap<String, (i64, i64, i64)> =
        std::collections::BTreeMap::new();
    for svc in &services {
        let month = svc
            .service_date
            .get(..7)
            .unwrap_or(&svc.service_date)
            .to_string();
        let entry = monthly_map.entry(month).or_insert((0, 0, 0));
        let cents = i64::from(svc.total_cost_cents.unwrap_or(0));
        entry.0 += cents;
        if svc.paid_by != "self" {
            entry.2 += cents;
        }
    }
    for p in &parts {
        if let Some(date) = &p.purchase_date {
            let month = date.get(..7).unwrap_or(date).to_string();
            let entry = monthly_map.entry(month).or_insert((0, 0, 0));
            entry.1 += i64::from(p.cost_cents.unwrap_or(0));
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
        total_service_cost_cents,
        total_parts_cost_cents,
        total_labor_cost_cents,
        total_cost_cents,
        out_of_pocket_cents,
        covered_cents,
        service_count: services.len(),
        part_count: parts.len(),
        cost_per_mile_cents,
        monthly_costs,
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
        assert_eq!(s.cost_per_mile_cents, None);
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
        assert_eq!(s.total_service_cost_cents, 15_000);
        assert_eq!(s.total_labor_cost_cents, 5_000);
        // purchased parts (20_000) exceed service-reported parts (10_000)
        assert_eq!(s.total_parts_cost_cents, 20_000);
        // total = service totals (15_000) + extra parts (20_000 - 10_000)
        assert_eq!(s.total_cost_cents, 25_000);
        // cost per mile = 25_000 / (3000 - 1000) = 12
        assert_eq!(s.cost_per_mile_cents, Some(12));
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
        assert_eq!(s.total_service_cost_cents, 25_000);
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
}
