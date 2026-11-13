use std::collections::HashMap;

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{mileage_log, part, service_record, service_record_line_item},
    error::DomainResult,
    services::{
        recall::{self, RecallRef},
        service_record::{PAYER_SELF, effective_payer},
    },
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
    /// What the owner paid: for a self-paid service its total minus its lines
    /// someone else paid (never below zero); for a service someone else paid,
    /// its lines marked `self` (never above its total); plus the parts the
    /// owner bought. A service with no total counts for nothing. Sums with
    /// `covered_cents` to `total_cost_cents`.
    pub out_of_pocket_cents: i64,
    /// What was charged but paid by someone else: per service, its total
    /// minus its out-of-pocket share (see `out_of_pocket_cents`).
    pub covered_cents: i64,
    /// The length of `covered_work`.
    pub covered_work_count: usize,
    /// Every service whose own payer is not `self` or that has a line someone
    /// else paid, newest first. A service with no total or no lines still
    /// appears here, although it adds nothing to `covered_cents`.
    pub covered_work: Vec<CoveredWork>,
    /// The sum of the labor subtotals entered on services, or `None` when no
    /// service has one.
    pub labor_subtotals_cents: Option<i64>,
    /// The sum of the parts subtotals entered on services, or `None` when no
    /// service has one.
    pub parts_subtotals_cents: Option<i64>,
    pub service_count: usize,
    pub part_count: usize,
    /// The odometer miles the records cover: the highest odometer reading on
    /// record minus the mileage of the earliest-dated service or reading.
    /// `None` when no such reading exists or the difference is not positive.
    pub miles_covered: Option<i64>,
    /// The date (`YYYY-MM-DD`) of the earliest-dated service or odometer
    /// reading that starts `miles_covered`, or `None` when there is none.
    pub history_starts_on: Option<String>,
    /// `total_cost_cents` per 1000 of `miles_covered`, rounded to the nearest
    /// cent; `None` when `miles_covered` is, or is under
    /// [`MIN_MILES_FOR_COST_PER_1000`], since a short history makes the rate
    /// noise.
    pub cost_per_1000_miles_cents: Option<i64>,
    pub monthly_costs: Vec<MonthlyCost>,
    /// Self-supplied part costs with neither a purchase nor an install date,
    /// which no month holds. The months plus this figure add up to
    /// `total_cost_cents`.
    pub unbucketed_parts_cents: i64,
}

/// The fewest recorded miles over which a cost per 1000 miles is reported.
pub const MIN_MILES_FOR_COST_PER_1000: i64 = 1_000;

/// A service with work someone other than the owner paid for.
#[derive(Serialize)]
pub struct CoveredWork {
    pub service_id: i32,
    pub service_date: String,
    pub description: Option<String>,
    /// Who paid the invoice overall.
    pub paid_by: String,
    pub payer_note: Option<String>,
    /// What others paid for this service: its total minus
    /// `out_of_pocket_cents`. Zero when the service has no total. Summed
    /// over `covered_work`, these add up to the summary's `covered_cents`.
    pub covered_by_others_cents: i64,
    /// What the owner paid for this service, by the rules of the summary's
    /// `out_of_pocket_cents`. Zero when the service has no total.
    pub out_of_pocket_cents: i64,
    /// The recalls this service performed.
    pub recalls: Vec<RecallRef>,
    /// The lines whose effective payer is not `self`.
    pub lines: Vec<CoveredLine>,
}

#[derive(Serialize)]
pub struct CoveredLine {
    pub description: String,
    pub work_kind: String,
    /// The line's own payer, or the service's when the line has none.
    pub paid_by: String,
    pub cost_cents: Option<i32>,
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

fn day_of(date: &str) -> String {
    date.get(..10).unwrap_or(date).to_string()
}

fn month_of(date: &str) -> String {
    date.get(..7).unwrap_or(date).to_string()
}

type LinesByService = HashMap<i32, Vec<service_record_line_item::Model>>;

async fn load_lines(
    db: &impl ConnectionTrait,
    services: &[service_record::Model],
) -> DomainResult<LinesByService> {
    let mut by_service = LinesByService::new();
    for line in service_record_line_item::Entity::find()
        .filter(
            service_record_line_item::Column::ServiceRecordId.is_in(services.iter().map(|s| s.id)),
        )
        .order_by_asc(service_record_line_item::Column::Id)
        .all(db)
        .await?
    {
        by_service
            .entry(line.service_record_id)
            .or_default()
            .push(line);
    }
    Ok(by_service)
}

/// Per service, the owner's share of its total. A self-paid service is the
/// owner's except for the lines someone else paid; a service someone else paid
/// is the owner's only for the lines marked `self`. A service with no total has
/// nothing to split and is left out. What others paid is the total minus this.
fn out_of_pocket_per_service(
    services: &[service_record::Model],
    lines_by_service: &LinesByService,
) -> HashMap<i32, i64> {
    let mut out_of_pocket_by_service = HashMap::new();
    for svc in services {
        let Some(total) = svc.total_cost_cents.map(i64::from) else {
            continue;
        };
        let lines = lines_by_service.get(&svc.id).map_or(&[][..], Vec::as_slice);
        let line_cents = |covered: bool| -> i64 {
            lines
                .iter()
                .filter(|l| {
                    (effective_payer(l.paid_by.as_deref(), &svc.paid_by) != PAYER_SELF) == covered
                })
                .map(|l| i64::from(l.cost_cents.unwrap_or(0)))
                .sum()
        };
        let out_of_pocket = if svc.paid_by == PAYER_SELF {
            Ord::max(total - line_cents(true), 0)
        } else {
            Ord::min(line_cents(false), total)
        };
        out_of_pocket_by_service.insert(svc.id, out_of_pocket);
    }
    out_of_pocket_by_service
}

/// The services with work someone else paid for, newest first, with the
/// lines that were covered.
async fn covered_work(
    db: &impl ConnectionTrait,
    services: &[service_record::Model],
    lines_by_service: &LinesByService,
    out_of_pocket_by_service: &HashMap<i32, i64>,
) -> DomainResult<Vec<CoveredWork>> {
    let mut covered: Vec<&service_record::Model> = services
        .iter()
        .filter(|s| {
            s.paid_by != PAYER_SELF
                || lines_by_service.get(&s.id).is_some_and(|lines| {
                    lines
                        .iter()
                        .any(|l| effective_payer(l.paid_by.as_deref(), &s.paid_by) != PAYER_SELF)
                })
        })
        .collect();
    covered.sort_by(|a, b| (&b.service_date, b.id).cmp(&(&a.service_date, a.id)));
    let ids: Vec<i32> = covered.iter().map(|s| s.id).collect();
    let mut recalls_by_service = recall::load_performed(db, &ids).await?;
    Ok(covered
        .into_iter()
        .map(|s| {
            let out_of_pocket = out_of_pocket_by_service.get(&s.id).copied().unwrap_or(0);
            CoveredWork {
                service_id: s.id,
                service_date: s.service_date.clone(),
                description: s.description.clone(),
                paid_by: s.paid_by.clone(),
                payer_note: s.payer_note.clone(),
                covered_by_others_cents: i64::from(s.total_cost_cents.unwrap_or(0)) - out_of_pocket,
                out_of_pocket_cents: out_of_pocket,
                recalls: recalls_by_service.remove(&s.id).unwrap_or_default(),
                lines: lines_by_service
                    .get(&s.id)
                    .into_iter()
                    .flatten()
                    .filter_map(|l| {
                        let payer = effective_payer(l.paid_by.as_deref(), &s.paid_by);
                        (payer != PAYER_SELF).then(|| CoveredLine {
                            description: l.description.clone(),
                            work_kind: l.work_kind.clone(),
                            paid_by: payer.to_owned(),
                            cost_cents: l.cost_cents,
                        })
                    })
                    .collect(),
            }
        })
        .collect())
}

/// The odometer miles the records cover and the date they start. The
/// purchase reading is left out on purpose: the owner may have bought the car
/// years before the records begin, and the costs cover only the miles driven
/// since the first record.
async fn recorded_miles(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    services: &[service_record::Model],
) -> DomainResult<(Option<i64>, Option<String>)> {
    let readings = mileage_log::Entity::find()
        .filter(mileage_log::Column::VehicleId.eq(vehicle_id))
        .all(db)
        .await?;
    let points: Vec<(String, i32)> = services
        .iter()
        .filter_map(|s| Some((day_of(&s.service_date), s.mileage?)))
        .chain(readings.iter().map(|r| (day_of(&r.recorded_at), r.mileage)))
        .collect();
    let start = points.iter().min();
    let end = points.iter().map(|(_, m)| *m).max();
    let miles = start
        .zip(end)
        .map(|((_, from), to)| i64::from(to) - i64::from(*from))
        .filter(|miles| *miles > 0);
    Ok((miles, start.map(|(date, _)| date.clone())))
}

/// Aggregate cost totals for a vehicle. Currency is always `i32`/`i64` cents;
/// integer arithmetic only (no float division).
pub async fn summary(db: &impl ConnectionTrait, vehicle_id: i32) -> DomainResult<CostSummary> {
    crate::services::vehicle::require(db, vehicle_id).await?;

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

    let lines_by_service = load_lines(db, &services).await?;
    let out_of_pocket_by_service = out_of_pocket_per_service(&services, &lines_by_service);
    let covered_by_service: HashMap<i32, i64> = services
        .iter()
        .filter_map(|s| {
            let total = i64::from(s.total_cost_cents?);
            Some((s.id, total - out_of_pocket_by_service.get(&s.id)?))
        })
        .collect();
    let covered_cents: i64 = covered_by_service.values().sum();
    let covered_work =
        covered_work(db, &services, &lines_by_service, &out_of_pocket_by_service).await?;
    let (miles_covered, history_starts_on) = recorded_miles(db, vehicle_id, &services).await?;
    let cost_per_1000_miles_cents = miles_covered
        .filter(|miles| *miles >= MIN_MILES_FOR_COST_PER_1000)
        .map(|miles| (total_cost_cents * 1000 + miles / 2) / miles);

    // Month buckets: (service cents, parts cents, covered cents).
    let mut monthly_map: std::collections::BTreeMap<String, (i64, i64, i64)> =
        std::collections::BTreeMap::new();
    for svc in &services {
        let entry = monthly_map
            .entry(month_of(&svc.service_date))
            .or_insert((0, 0, 0));
        entry.0 += i64::from(svc.total_cost_cents.unwrap_or(0));
        entry.2 += covered_by_service.get(&svc.id).copied().unwrap_or(0);
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
        covered_work_count: covered_work.len(),
        covered_work,
        labor_subtotals_cents: sum_present(services.iter().map(|s| s.labor_cost_cents)),
        parts_subtotals_cents: sum_present(services.iter().map(|s| s.parts_cost_cents)),
        service_count: services.len(),
        part_count: parts.len(),
        miles_covered,
        history_starts_on,
        cost_per_1000_miles_cents,
        monthly_costs,
        unbucketed_parts_cents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::{mileage_log, service_recall_link, service_record_line_item},
        test_support::{VehicleFixture, test_db},
    };

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
        assert_eq!(s.miles_covered, None);
        assert_eq!(s.history_starts_on, None);
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
        // 35_000 cents over the 1000 miles from the first service (2000) to the
        // latest reading (3000) is 35_000 cents per 1000 miles.
        assert_eq!(s.miles_covered, Some(1_000));
        assert_eq!(s.history_starts_on.as_deref(), Some("2024-01-15"));
        assert_eq!(s.cost_per_1000_miles_cents, Some(35_000));
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
    async fn cost_per_1000_miles_is_null_when_the_records_cover_no_miles() {
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
        assert_eq!(s.miles_covered, None);
        assert_eq!(s.cost_per_1000_miles_cents, None);
    }

    async fn insert_reading(db: &impl ConnectionTrait, vid: i32, date: &str, miles: i32) {
        mileage_log::ActiveModel {
            vehicle_id: Set(vid),
            mileage: Set(miles),
            recorded_at: Set(date.into()),
            source: Set(Some("manual".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn the_cost_per_1000_miles_covers_only_the_recorded_miles() {
        let db = test_db().await;
        let vid = VehicleFixture::new()
            .purchase_date("2017-05-01")
            .purchase_mileage(40)
            .insert_id(&db)
            .await;
        for (date, miles, total) in [
            ("2026-10-01", 62_800, Some(57_500)),
            ("2026-11-09", 63_950, None),
        ] {
            let id = insert_service(&db, vid, date, total, "self").await;
            let mut active: service_record::ActiveModel = service_record::Entity::find_by_id(id)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .into();
            active.mileage = Set(Some(miles));
            active.update(&db).await.unwrap();
            insert_reading(&db, vid, date, miles).await;
        }
        insert_reading(&db, vid, "2026-11-12 09:00:00", 64_000).await;
        insert_part(&db, vid, "self", 8_900, None, Some("2026-10-30")).await;

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.total_cost_cents, 66_400);
        assert_eq!(s.miles_covered, Some(1_200));
        assert_eq!(s.history_starts_on.as_deref(), Some("2026-10-01"));
        // 66_400 cents over 1200 miles is 55_333 cents per 1000 miles.
        assert_eq!(s.cost_per_1000_miles_cents, Some(55_333));
    }

    async fn insert_line(
        db: &impl ConnectionTrait,
        service_id: i32,
        description: &str,
        work_kind: &str,
        cost: Option<i32>,
        paid_by: Option<&str>,
    ) {
        service_record_line_item::ActiveModel {
            service_record_id: Set(service_id),
            description: Set(description.into()),
            work_kind: Set(work_kind.into()),
            paid_by: Set(paid_by.map(Into::into)),
            cost_cents: Set(cost),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn covered_work_lists_every_service_someone_else_paid_newest_first() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        insert_service(&db, vid, "2026-10-01", Some(10_000), "self").await;
        let old = insert_service(&db, vid, "2026-10-05", Some(5_000), "insurance").await;
        let new = insert_service(&db, vid, "2026-11-09", None, "manufacturer").await;
        let finding = crate::services::research::record_recalls(
            &db,
            vid,
            &[crate::services::nhtsa::RecallInfo {
                campaign_number: "26V001000".into(),
                manufacturer: None,
                title: "Fuel Pump".into(),
                summary: None,
                consequence: None,
                remedy: None,
                report_date: None,
                component: None,
                action_number: None,
            }],
        )
        .await
        .unwrap()
        .recalls[0]
            .finding_id;
        service_recall_link::ActiveModel {
            service_record_id: Set(new),
            research_finding_id: Set(finding),
        }
        .insert(&db)
        .await
        .unwrap();

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.covered_cents, 5_000);
        assert_eq!(s.covered_work_count, 2);
        let ids: Vec<i32> = s.covered_work.iter().map(|c| c.service_id).collect();
        assert_eq!(ids, vec![new, old]);
        let first = &s.covered_work[0];
        assert_eq!(first.paid_by, "manufacturer");
        assert_eq!(first.covered_by_others_cents, 0);
        assert_eq!(first.out_of_pocket_cents, 0);
        assert!(first.lines.is_empty());
        assert_eq!(first.recalls.len(), 1);
        assert_eq!(first.recalls[0].finding_id, finding);
        assert_eq!(first.recalls[0].title, "Fuel Pump");
        assert_eq!(s.covered_work[1].covered_by_others_cents, 5_000);
        assert_eq!(s.covered_work[1].out_of_pocket_cents, 0);
        assert!(s.covered_work[1].recalls.is_empty());
    }

    #[tokio::test]
    async fn a_warranty_line_with_no_cost_on_an_owner_paid_invoice_is_listed_but_costs_nothing() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let id = insert_service(&db, vid, "2026-11-03", Some(12_000), "self").await;
        insert_line(&db, id, "Oil change", "maintenance", Some(12_000), None).await;
        insert_line(
            &db,
            id,
            "Replace failed water pump under warranty",
            "repair",
            None,
            Some("manufacturer"),
        )
        .await;

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.out_of_pocket_cents, 12_000);
        assert_eq!(s.covered_cents, 0);
        assert_eq!(s.covered_work_count, 1);
        let work = &s.covered_work[0];
        assert_eq!(work.service_id, id);
        assert_eq!(work.paid_by, "self");
        assert_eq!(work.lines.len(), 1);
        assert_eq!(
            work.lines[0].description,
            "Replace failed water pump under warranty"
        );
        assert_eq!(work.lines[0].work_kind, "repair");
        assert_eq!(work.lines[0].paid_by, "manufacturer");
        assert_eq!(work.lines[0].cost_cents, None);
    }

    #[tokio::test]
    async fn a_line_paid_by_insurance_on_an_owner_paid_invoice_is_covered() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let id = insert_service(&db, vid, "2026-11-03", Some(90_000), "self").await;
        insert_line(
            &db,
            id,
            "Bodywork",
            "repair",
            Some(80_000),
            Some("insurance"),
        )
        .await;
        insert_line(
            &db,
            id,
            "Deductible-side repair",
            "repair",
            Some(10_000),
            None,
        )
        .await;

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.out_of_pocket_cents, 10_000);
        assert_eq!(s.covered_cents, 80_000);
        assert_eq!(s.covered_work[0].lines.len(), 1);
        assert_eq!(s.covered_work[0].lines[0].cost_cents, Some(80_000));
    }

    #[tokio::test]
    async fn a_covered_line_total_never_pushes_out_of_pocket_below_zero() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let id = insert_service(&db, vid, "2026-11-03", Some(10_000), "self").await;
        insert_line(&db, id, "Work", "repair", Some(15_000), Some("insurance")).await;

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.out_of_pocket_cents, 0);
        assert_eq!(s.covered_cents, 10_000);
    }

    #[tokio::test]
    async fn a_manufacturer_paid_recall_with_no_total_costs_nothing_and_is_listed() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let id = insert_service(&db, vid, "2026-11-09", None, "manufacturer").await;
        insert_line(&db, id, "Fuel pump recall", "recall", None, None).await;

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.out_of_pocket_cents, 0);
        assert_eq!(s.covered_cents, 0);
        assert_eq!(s.covered_work_count, 1);
        assert_eq!(s.covered_work[0].lines[0].paid_by, "manufacturer");
        assert_eq!(s.covered_work[0].lines[0].cost_cents, None);
    }

    #[tokio::test]
    async fn covered_work_figures_add_up_to_the_summary_figures() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        // The worked example: $450 warranty repair, $129 oil change and $11
        // in fees on one invoice with a grand total of $590.
        let a = insert_service(&db, vid, "2026-11-03", Some(59_000), "self").await;
        insert_line(
            &db,
            a,
            "Water pump",
            "repair",
            Some(45_000),
            Some("manufacturer"),
        )
        .await;
        insert_line(&db, a, "Oil change", "maintenance", Some(12_900), None).await;
        insert_line(&db, a, "Shop fees", "fee", Some(1_100), None).await;
        let b = insert_service(&db, vid, "2026-11-05", Some(30_000), "insurance").await;
        insert_line(&db, b, "Bodywork", "repair", Some(30_000), None).await;
        let c = insert_service(&db, vid, "2026-11-06", Some(8_000), "manufacturer").await;
        insert_line(&db, c, "Tax", "fee", Some(500), Some("self")).await;

        let s = summary(&db, vid).await.unwrap();
        let first = s.covered_work.iter().find(|w| w.service_id == a).unwrap();
        assert_eq!(first.out_of_pocket_cents, 14_000);
        assert_eq!(first.covered_by_others_cents, 45_000);
        let covered: i64 = s
            .covered_work
            .iter()
            .map(|w| w.covered_by_others_cents)
            .sum();
        let out_of_pocket: i64 = s.covered_work.iter().map(|w| w.out_of_pocket_cents).sum();
        assert_eq!(covered, s.covered_cents);
        assert_eq!(out_of_pocket, s.service_totals_cents - s.covered_cents);
        assert_eq!(out_of_pocket, s.out_of_pocket_cents);
    }

    async fn history_of_miles(miles: i32) -> CostSummary {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        insert_service(&db, vid, "2026-11-03", Some(14_000), "self").await;
        insert_reading(&db, vid, "2026-11-01", 63_000).await;
        insert_reading(&db, vid, "2026-11-12", 63_000 + miles).await;
        summary(&db, vid).await.unwrap()
    }

    #[tokio::test]
    async fn the_cost_per_1000_miles_is_withheld_below_1000_recorded_miles() {
        let short = history_of_miles(999).await;
        assert_eq!(short.miles_covered, Some(999));
        assert_eq!(short.history_starts_on.as_deref(), Some("2026-11-01"));
        assert_eq!(short.cost_per_1000_miles_cents, None);

        let enough = history_of_miles(1_000).await;
        assert_eq!(enough.miles_covered, Some(1_000));
        assert_eq!(enough.cost_per_1000_miles_cents, Some(14_000));
    }

    #[tokio::test]
    async fn a_line_marked_self_on_a_covered_invoice_is_out_of_pocket() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let id = insert_service(&db, vid, "2026-11-09", Some(50_000), "manufacturer").await;
        insert_line(&db, id, "Shop supplies", "fee", Some(5_000), Some("self")).await;

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.out_of_pocket_cents, 5_000);
        assert_eq!(s.covered_cents, 45_000);
        assert!(s.covered_work[0].lines.is_empty());
    }

    #[tokio::test]
    async fn the_breakdown_and_monthly_rows_add_up_to_the_headline() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let a = insert_service(&db, vid, "2026-10-03", Some(12_000), "self").await;
        insert_line(&db, a, "Oil change", "maintenance", Some(12_000), None).await;
        insert_line(&db, a, "Water pump", "repair", None, Some("manufacturer")).await;
        let b = insert_service(&db, vid, "2026-10-20", Some(90_000), "self").await;
        insert_line(
            &db,
            b,
            "Bodywork",
            "repair",
            Some(80_000),
            Some("insurance"),
        )
        .await;
        insert_line(&db, b, "Repair", "repair", Some(10_000), None).await;
        let c = insert_service(&db, vid, "2026-11-09", None, "manufacturer").await;
        insert_line(&db, c, "Recall", "recall", None, None).await;
        let d = insert_service(&db, vid, "2026-11-10", Some(50_000), "manufacturer").await;
        insert_line(&db, d, "Fee", "fee", Some(5_000), Some("self")).await;
        insert_part(&db, vid, "self", 3_000, Some("2026-10-05"), None).await;

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.total_cost_cents, 12_000 + 90_000 + 50_000 + 3_000);
        assert_eq!(s.covered_cents, 80_000 + 45_000);
        assert_eq!(s.out_of_pocket_cents, 12_000 + 10_000 + 5_000 + 3_000);
        assert_eq!(s.out_of_pocket_cents + s.covered_cents, s.total_cost_cents);
        let months_total: i64 = s.monthly_costs.iter().map(|m| m.total_cents).sum();
        let months_covered: i64 = s.monthly_costs.iter().map(|m| m.covered_cents).sum();
        let months_oop: i64 = s.monthly_costs.iter().map(|m| m.out_of_pocket_cents).sum();
        assert_eq!(months_total + s.unbucketed_parts_cents, s.total_cost_cents);
        assert_eq!(months_covered, s.covered_cents);
        assert_eq!(months_oop, s.out_of_pocket_cents);
        assert_eq!(s.covered_work_count, 4);
    }
}
