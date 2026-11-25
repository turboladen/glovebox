use std::collections::HashMap;

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{
        mileage_log,
        part::{self, PartStatus, Supplier},
        service_record::{self, Payer},
        service_record_line_item::{self, WorkKind},
    },
    error::{DomainError, DomainResult},
    services::{
        recall::{self, RecallRef},
        service_record::effective_payer,
    },
};

/// The span of dates a cost summary counts, inclusive at both ends. An
/// empty bound leaves that end open.
#[derive(Debug, Default, Clone, Copy)]
pub struct CostRange {
    pub from: Option<chrono::NaiveDate>,
    pub to: Option<chrono::NaiveDate>,
}

impl CostRange {
    fn is_open(self) -> bool {
        self.from.is_none() && self.to.is_none()
    }

    fn holds(self, date: chrono::NaiveDate) -> bool {
        self.from.is_none_or(|from| date >= from) && self.to.is_none_or(|to| date <= to)
    }
}

#[derive(Debug, Serialize)]
pub struct CostSummary {
    pub vehicle_id: i32,
    /// The first date counted, or `None` when the summary has no start.
    pub from: Option<chrono::NaiveDate>,
    /// The last date counted, or `None` when the summary has no end.
    pub to: Option<chrono::NaiveDate>,
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
    /// The lines of the counted services by kind of work, in the order of
    /// the `WorkKind` list. A kind with no line is left out. The figures are
    /// the lines' own amounts, so they add up to the service totals only when
    /// every invoice's lines do.
    pub by_work_kind: Vec<WorkKindCost>,
    pub service_count: usize,
    pub part_count: usize,
    /// The odometer miles the records cover: the highest odometer reading on
    /// record minus the mileage of the earliest-dated service or reading.
    /// `None` when no such reading exists or the difference is not positive.
    /// With a date range, only services and readings dated inside it count.
    pub miles_covered: Option<i64>,
    /// The date of the earliest-dated service or odometer
    /// reading that starts `miles_covered`, or `None` when there is none.
    pub history_starts_on: Option<chrono::NaiveDate>,
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
#[derive(Debug, Serialize)]
pub struct CoveredWork {
    pub service_id: i32,
    pub service_date: chrono::NaiveDate,
    pub description: Option<String>,
    /// Who paid the invoice overall.
    pub paid_by: Payer,
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

#[derive(Debug, Serialize)]
pub struct CoveredLine {
    pub description: String,
    pub work_kind: WorkKind,
    /// The line's own payer, or the service's when the line has none.
    pub paid_by: Payer,
    pub cost_cents: Option<i32>,
}

/// The lines of one kind of work across the counted services.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct WorkKindCost {
    pub work_kind: WorkKind,
    pub line_count: usize,
    /// The sum of the lines' amounts. A line with no amount adds nothing.
    pub cost_cents: i64,
    /// The part of `cost_cents` on lines the owner paid, where a line with no
    /// payer of its own follows its service.
    pub out_of_pocket_cents: i64,
    /// The part of `cost_cents` on lines someone else paid.
    pub covered_cents: i64,
}

#[derive(Debug, Serialize)]
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

/// The `YYYY-MM` label of the month `date` falls in.
fn month_of(date: chrono::NaiveDate) -> String {
    date.format("%Y-%m").to_string()
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
                .filter(|l| effective_payer(l.paid_by, svc.paid_by).is_covered() == covered)
                .map(|l| i64::from(l.cost_cents.unwrap_or(0)))
                .sum()
        };
        let out_of_pocket = if svc.paid_by.is_covered() {
            Ord::min(line_cents(false), total)
        } else {
            Ord::max(total - line_cents(true), 0)
        };
        out_of_pocket_by_service.insert(svc.id, out_of_pocket);
    }
    out_of_pocket_by_service
}

/// Per service, the owner's share of its total, read as
/// `out_of_pocket_per_service` reads it. A service with no total is left out.
pub(crate) async fn out_of_pocket(
    db: &impl ConnectionTrait,
    services: &[service_record::Model],
) -> DomainResult<HashMap<i32, i64>> {
    let lines_by_service = load_lines(db, services).await?;
    Ok(out_of_pocket_per_service(services, &lines_by_service))
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
            s.paid_by.is_covered()
                || lines_by_service.get(&s.id).is_some_and(|lines| {
                    lines
                        .iter()
                        .any(|l| effective_payer(l.paid_by, s.paid_by).is_covered())
                })
        })
        .collect();
    covered.sort_by_key(|s| std::cmp::Reverse((s.service_date, s.id)));
    let ids: Vec<i32> = covered.iter().map(|s| s.id).collect();
    let mut recalls_by_service = recall::load_performed(db, &ids).await?;
    Ok(covered
        .into_iter()
        .map(|s| {
            let out_of_pocket = out_of_pocket_by_service.get(&s.id).copied().unwrap_or(0);
            CoveredWork {
                service_id: s.id,
                service_date: s.service_date,
                description: s.description.clone(),
                paid_by: s.paid_by,
                payer_note: s.payer_note.clone(),
                covered_by_others_cents: i64::from(s.total_cost_cents.unwrap_or(0)) - out_of_pocket,
                out_of_pocket_cents: out_of_pocket,
                recalls: recalls_by_service.remove(&s.id).unwrap_or_default(),
                lines: lines_by_service
                    .get(&s.id)
                    .into_iter()
                    .flatten()
                    .filter_map(|l| {
                        let payer = effective_payer(l.paid_by, s.paid_by);
                        payer.is_covered().then(|| CoveredLine {
                            description: l.description.clone(),
                            work_kind: l.work_kind,
                            paid_by: payer,
                            cost_cents: l.cost_cents,
                        })
                    })
                    .collect(),
            }
        })
        .collect())
}

/// The odometer miles the records cover and the date they start, counting
/// the given services and the odometer readings dated inside `range`. The
/// purchase reading is left out on purpose: the owner may have bought the car
/// years before the records begin, and the costs cover only the miles driven
/// since the first record.
async fn recorded_miles(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    services: &[service_record::Model],
    range: CostRange,
) -> DomainResult<(Option<i64>, Option<chrono::NaiveDate>)> {
    let readings = mileage_log::Entity::find()
        .filter(mileage_log::Column::VehicleId.eq(vehicle_id))
        .all(db)
        .await?;
    let points: Vec<(chrono::NaiveDate, i32)> = services
        .iter()
        .filter_map(|s| Some((s.service_date, s.mileage?)))
        .chain(readings.iter().map(|r| (r.recorded_at.day(), r.mileage)))
        .filter(|(date, _)| range.holds(*date))
        .collect();
    let start = points.iter().min();
    let end = points.iter().map(|(_, m)| *m).max();
    let miles = start
        .zip(end)
        .map(|((_, from), to)| i64::from(to) - i64::from(*from))
        .filter(|miles| *miles > 0);
    Ok((miles, start.map(|(date, _)| *date)))
}

/// The lines of the given services by kind of work. Each line is judged by
/// its own payer, or its service's when it has none.
fn by_work_kind(
    services: &[service_record::Model],
    lines_by_service: &LinesByService,
) -> Vec<WorkKindCost> {
    use crate::vocab::Vocabulary;
    let mut kinds: Vec<WorkKindCost> = WorkKind::ALL
        .iter()
        .map(|kind| WorkKindCost {
            work_kind: *kind,
            line_count: 0,
            cost_cents: 0,
            out_of_pocket_cents: 0,
            covered_cents: 0,
        })
        .collect();
    for svc in services {
        for line in lines_by_service.get(&svc.id).into_iter().flatten() {
            let Some(kind) = kinds.iter_mut().find(|k| k.work_kind == line.work_kind) else {
                continue;
            };
            let cents = i64::from(line.cost_cents.unwrap_or(0));
            kind.line_count += 1;
            kind.cost_cents += cents;
            if effective_payer(line.paid_by, svc.paid_by).is_covered() {
                kind.covered_cents += cents;
            } else {
                kind.out_of_pocket_cents += cents;
            }
        }
    }
    kinds.retain(|k| k.line_count > 0);
    kinds
}

/// The services and parts a summary over `range` counts: the services dated
/// inside it, oldest first, and the parts that were not returned. With a
/// bound set, a part counts by its purchase date, or its install date when it
/// has no purchase date, and a part with neither is left out.
async fn counted_records(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    range: CostRange,
) -> DomainResult<(Vec<service_record::Model>, Vec<part::Model>)> {
    let mut query =
        service_record::Entity::find().filter(service_record::Column::VehicleId.eq(vehicle_id));
    if let Some(from) = range.from {
        query = query.filter(service_record::Column::ServiceDate.gte(from));
    }
    if let Some(to) = range.to {
        query = query.filter(service_record::Column::ServiceDate.lte(to));
    }
    let services = query
        .order_by_asc(service_record::Column::ServiceDate)
        .all(db)
        .await?;
    // A returned part went back to the seller, so it is no spend at all.
    let mut parts = part::Entity::find()
        .filter(part::Column::VehicleId.eq(vehicle_id))
        .filter(part::Column::Status.ne(PartStatus::Returned))
        .all(db)
        .await?;
    if !range.is_open() {
        parts.retain(|p| {
            p.purchase_date
                .or(p.installed_date)
                .is_some_and(|date| range.holds(date))
        });
    }
    Ok((services, parts))
}

/// Aggregate cost totals for a vehicle over all of its records. Currency is
/// always `i32`/`i64` cents; integer arithmetic only (no float division).
pub async fn summary(db: &impl ConnectionTrait, vehicle_id: i32) -> DomainResult<CostSummary> {
    summary_in(db, vehicle_id, CostRange::default()).await
}

/// [`summary`] over the records dated inside `range`, both ends included.
///
/// # Errors
/// `NotFound` for a missing vehicle, and `Invalid` when the range starts
/// after it ends.
pub async fn summary_in(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    range: CostRange,
) -> DomainResult<CostSummary> {
    crate::services::vehicle::require(db, vehicle_id).await?;
    if let (Some(from), Some(to)) = (range.from, range.to)
        && from > to
    {
        return Err(DomainError::Invalid {
            field: "from".into(),
            message: format!("{from} is after the end of the range, {to}"),
        });
    }
    let (services, parts) = counted_records(db, vehicle_id, range).await?;

    let service_totals_cents =
        sum_present(services.iter().map(|s| s.total_cost_cents)).unwrap_or(0);
    // A part the shop supplied is billed on its service's invoice, so its cost
    // is already inside that service's total. Only owner-bought parts are
    // spend beyond the invoices.
    let self_parts: Vec<&part::Model> = parts
        .iter()
        .filter(|p| p.supplied_by != Supplier::Shop)
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
    let (miles_covered, history_starts_on) =
        recorded_miles(db, vehicle_id, &services, range).await?;
    let cost_per_1000_miles_cents = miles_covered
        .filter(|miles| *miles >= MIN_MILES_FOR_COST_PER_1000)
        .map(|miles| (total_cost_cents * 1000 + miles / 2) / miles);

    // Month buckets: (service cents, parts cents, covered cents).
    let mut monthly_map: std::collections::BTreeMap<String, (i64, i64, i64)> =
        std::collections::BTreeMap::new();
    for svc in &services {
        let entry = monthly_map
            .entry(month_of(svc.service_date))
            .or_insert((0, 0, 0));
        entry.0 += i64::from(svc.total_cost_cents.unwrap_or(0));
        entry.2 += covered_by_service.get(&svc.id).copied().unwrap_or(0);
    }
    let mut unbucketed_parts_cents = 0;
    for p in self_parts {
        let cents = i64::from(p.cost_cents.unwrap_or(0));
        match p.purchase_date.or(p.installed_date) {
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
        from: range.from,
        to: range.to,
        service_totals_cents,
        self_supplied_parts_cents,
        total_cost_cents,
        out_of_pocket_cents: total_cost_cents - covered_cents,
        covered_cents,
        covered_work_count: covered_work.len(),
        covered_work,
        labor_subtotals_cents: sum_present(services.iter().map(|s| s.labor_cost_cents)),
        parts_subtotals_cents: sum_present(services.iter().map(|s| s.parts_cost_cents)),
        by_work_kind: by_work_kind(&services, &lines_by_service),
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
        test_support::{VehicleFixture, date, test_db},
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
            service_date: Set(date("2024-01-15")),
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
            service_date: Set(date("2024-02-20")),
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
            status: Set(PartStatus::Purchased),
            cost_cents: Set(Some(20_000)),
            purchase_date: Set(Some(date("2024-01-10"))),
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
        assert_eq!(s.history_starts_on, Some(date("2024-01-15")));
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
            service_date: Set(date("2024-01-15")),
            total_cost_cents: Set(Some(10_000)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        // $150 insurance-paid service in February.
        service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set(date("2024-02-20")),
            total_cost_cents: Set(Some(15_000)),
            paid_by: Set(Payer::Insurance),
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
            status: Set(PartStatus::Purchased),
            cost_cents: Set(Some(2_000)),
            purchase_date: Set(Some(date("2024-01-10"))),
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
            service_date: Set(date("2024-03-01")),
            total_cost_cents: Set(Some(20_000)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        for (name, supplied_by, cents) in [
            ("Plugs", Supplier::Shop, 6_000),
            ("Pads", Supplier::Owner, 9_000),
        ] {
            part::ActiveModel {
                vehicle_id: Set(vid),
                name: Set(name.into()),
                status: Set(PartStatus::Installed),
                supplied_by: Set(supplied_by),
                cost_cents: Set(Some(cents)),
                purchase_date: Set(Some(date("2024-03-01"))),
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
        paid_by: Payer,
    ) -> i32 {
        service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set(crate::test_support::date(date)),
            total_cost_cents: Set(total),
            paid_by: Set(paid_by),
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
        supplied_by: Supplier,
        cents: i32,
        purchase_date: Option<&str>,
        installed_date: Option<&str>,
    ) {
        part::ActiveModel {
            vehicle_id: Set(vid),
            name: Set("Part".into()),
            status: Set(PartStatus::Installed),
            supplied_by: Set(supplied_by),
            cost_cents: Set(Some(cents)),
            purchase_date: Set(purchase_date.map(date)),
            installed_date: Set(installed_date.map(date)),
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
        insert_service(&db, vid, "2026-11-03", Some(56_900), Payer::Owner).await;
        insert_part(&db, vid, Supplier::Shop, 2_200, None, Some("2026-11-03")).await;
        insert_part(&db, vid, Supplier::Owner, 8_900, Some("2026-10-20"), None).await;

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.service_totals_cents, 56_900);
        assert_eq!(s.self_supplied_parts_cents, 8_900);
        assert_eq!(s.total_cost_cents, 65_800);
    }

    #[tokio::test]
    async fn the_headline_is_the_service_totals_plus_the_self_supplied_parts() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        insert_service(&db, vid, "2026-11-03", Some(57_500), Payer::Owner).await;
        insert_service(&db, vid, "2026-11-10", None, Payer::Owner).await;
        insert_service(&db, vid, "2026-11-12", None, Payer::Owner).await;
        insert_part(&db, vid, Supplier::Shop, 2_200, None, Some("2026-11-03")).await;
        // No purchase date, so the install date places it in a month.
        insert_part(&db, vid, Supplier::Owner, 8_900, None, Some("2026-10-30")).await;

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
    async fn cost_summary_leaves_out_returned_parts() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        insert_part(&db, vid, Supplier::Owner, 8_900, Some("2026-10-20"), None).await;
        let before = summary(&db, vid).await.unwrap();
        let returned =
            insert_part_with_status(&db, vid, PartStatus::Returned, 18_000, "2026-10-21").await;
        assert!(returned > 0);
        let after = summary(&db, vid).await.unwrap();
        assert_eq!(after.self_supplied_parts_cents, 8_900);
        assert_eq!(after.total_cost_cents, before.total_cost_cents);
        assert_eq!(after.part_count, before.part_count);
        assert_eq!(after.monthly_costs.len(), before.monthly_costs.len());
        assert_eq!(after.monthly_costs[0].parts_cost_cents, 8_900);
        assert_eq!(after.unbucketed_parts_cents, 0);
    }

    async fn insert_part_with_status(
        db: &impl ConnectionTrait,
        vid: i32,
        status: PartStatus,
        cents: i32,
        purchase_date: &str,
    ) -> i32 {
        part::ActiveModel {
            vehicle_id: Set(vid),
            name: Set("Part".into()),
            status: Set(status),
            supplied_by: Set(Supplier::Owner),
            cost_cents: Set(Some(cents)),
            purchase_date: Set(Some(date(purchase_date))),
            // A returned part records when and why it went back.
            returned_on: Set((status == PartStatus::Returned).then(|| date(purchase_date))),
            return_reason: Set((status == PartStatus::Returned).then(|| "Wrong size".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn a_part_with_no_date_is_reported_as_unbucketed() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        insert_service(&db, vid, "2026-11-03", Some(10_000), Payer::Owner).await;
        insert_part(&db, vid, Supplier::Owner, 3_000, None, None).await;

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
        insert_service(&db, vid, "2026-11-03", Some(10_000), Payer::Owner).await;
        insert_service(&db, vid, "2026-11-04", Some(5_000), Payer::Manufacturer).await;

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
            service_date: Set(date("2026-11-03")),
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
            recorded_at: Set(crate::test_support::date(date)),
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
            let id = insert_service(&db, vid, date, total, Payer::Owner).await;
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
        insert_part(&db, vid, Supplier::Owner, 8_900, None, Some("2026-10-30")).await;

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.total_cost_cents, 66_400);
        assert_eq!(s.miles_covered, Some(1_200));
        assert_eq!(s.history_starts_on, Some(date("2026-10-01")));
        // 66_400 cents over 1200 miles is 55_333 cents per 1000 miles.
        assert_eq!(s.cost_per_1000_miles_cents, Some(55_333));
    }

    async fn insert_line(
        db: &impl ConnectionTrait,
        service_id: i32,
        description: &str,
        work_kind: WorkKind,
        cost: Option<i32>,
        paid_by: Option<Payer>,
    ) {
        service_record_line_item::ActiveModel {
            service_record_id: Set(service_id),
            description: Set(description.into()),
            work_kind: Set(work_kind),
            paid_by: Set(paid_by),
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
        insert_service(&db, vid, "2026-10-01", Some(10_000), Payer::Owner).await;
        let old = insert_service(&db, vid, "2026-10-05", Some(5_000), Payer::Insurance).await;
        let new = insert_service(&db, vid, "2026-11-09", None, Payer::Manufacturer).await;
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
        assert_eq!(first.paid_by, Payer::Manufacturer);
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
        let id = insert_service(&db, vid, "2026-11-03", Some(12_000), Payer::Owner).await;
        insert_line(
            &db,
            id,
            "Oil change",
            WorkKind::Maintenance,
            Some(12_000),
            None,
        )
        .await;
        insert_line(
            &db,
            id,
            "Replace failed water pump under warranty",
            WorkKind::Repair,
            None,
            Some(Payer::Manufacturer),
        )
        .await;

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.out_of_pocket_cents, 12_000);
        assert_eq!(s.covered_cents, 0);
        assert_eq!(s.covered_work_count, 1);
        let work = &s.covered_work[0];
        assert_eq!(work.service_id, id);
        assert_eq!(work.paid_by, Payer::Owner);
        assert_eq!(work.lines.len(), 1);
        assert_eq!(
            work.lines[0].description,
            "Replace failed water pump under warranty"
        );
        assert_eq!(work.lines[0].work_kind, WorkKind::Repair);
        assert_eq!(work.lines[0].paid_by, Payer::Manufacturer);
        assert_eq!(work.lines[0].cost_cents, None);
    }

    #[tokio::test]
    async fn a_line_paid_by_insurance_on_an_owner_paid_invoice_is_covered() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let id = insert_service(&db, vid, "2026-11-03", Some(90_000), Payer::Owner).await;
        insert_line(
            &db,
            id,
            "Bodywork",
            WorkKind::Repair,
            Some(80_000),
            Some(Payer::Insurance),
        )
        .await;
        insert_line(
            &db,
            id,
            "Deductible-side repair",
            WorkKind::Repair,
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
        let id = insert_service(&db, vid, "2026-11-03", Some(10_000), Payer::Owner).await;
        insert_line(
            &db,
            id,
            "Work",
            WorkKind::Repair,
            Some(15_000),
            Some(Payer::Insurance),
        )
        .await;

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.out_of_pocket_cents, 0);
        assert_eq!(s.covered_cents, 10_000);
    }

    #[tokio::test]
    async fn a_manufacturer_paid_recall_with_no_total_costs_nothing_and_is_listed() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let id = insert_service(&db, vid, "2026-11-09", None, Payer::Manufacturer).await;
        insert_line(&db, id, "Fuel pump recall", WorkKind::Recall, None, None).await;

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.out_of_pocket_cents, 0);
        assert_eq!(s.covered_cents, 0);
        assert_eq!(s.covered_work_count, 1);
        assert_eq!(s.covered_work[0].lines[0].paid_by, Payer::Manufacturer);
        assert_eq!(s.covered_work[0].lines[0].cost_cents, None);
    }

    #[tokio::test]
    async fn covered_work_figures_add_up_to_the_summary_figures() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        // The worked example: $450 warranty repair, $129 oil change and $11
        // in fees on one invoice with a grand total of $590.
        let a = insert_service(&db, vid, "2026-11-03", Some(59_000), Payer::Owner).await;
        insert_line(
            &db,
            a,
            "Water pump",
            WorkKind::Repair,
            Some(45_000),
            Some(Payer::Manufacturer),
        )
        .await;
        insert_line(
            &db,
            a,
            "Oil change",
            WorkKind::Maintenance,
            Some(12_900),
            None,
        )
        .await;
        insert_line(&db, a, "Shop fees", WorkKind::Fee, Some(1_100), None).await;
        let b = insert_service(&db, vid, "2026-11-05", Some(30_000), Payer::Insurance).await;
        insert_line(&db, b, "Bodywork", WorkKind::Repair, Some(30_000), None).await;
        let c = insert_service(&db, vid, "2026-11-06", Some(8_000), Payer::Manufacturer).await;
        insert_line(&db, c, "Tax", WorkKind::Fee, Some(500), Some(Payer::Owner)).await;

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
        insert_service(&db, vid, "2026-11-03", Some(14_000), Payer::Owner).await;
        insert_reading(&db, vid, "2026-11-01", 63_000).await;
        insert_reading(&db, vid, "2026-11-12", 63_000 + miles).await;
        summary(&db, vid).await.unwrap()
    }

    #[tokio::test]
    async fn the_cost_per_1000_miles_is_withheld_below_1000_recorded_miles() {
        let short = history_of_miles(999).await;
        assert_eq!(short.miles_covered, Some(999));
        assert_eq!(short.history_starts_on, Some(date("2026-11-01")));
        assert_eq!(short.cost_per_1000_miles_cents, None);

        let enough = history_of_miles(1_000).await;
        assert_eq!(enough.miles_covered, Some(1_000));
        assert_eq!(enough.cost_per_1000_miles_cents, Some(14_000));
    }

    #[tokio::test]
    async fn a_line_marked_self_on_a_covered_invoice_is_out_of_pocket() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let id = insert_service(&db, vid, "2026-11-09", Some(50_000), Payer::Manufacturer).await;
        insert_line(
            &db,
            id,
            "Shop supplies",
            WorkKind::Fee,
            Some(5_000),
            Some(Payer::Owner),
        )
        .await;

        let s = summary(&db, vid).await.unwrap();
        assert_eq!(s.out_of_pocket_cents, 5_000);
        assert_eq!(s.covered_cents, 45_000);
        assert!(s.covered_work[0].lines.is_empty());
    }

    #[tokio::test]
    async fn the_breakdown_and_monthly_rows_add_up_to_the_headline() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let a = insert_service(&db, vid, "2026-10-03", Some(12_000), Payer::Owner).await;
        insert_line(
            &db,
            a,
            "Oil change",
            WorkKind::Maintenance,
            Some(12_000),
            None,
        )
        .await;
        insert_line(
            &db,
            a,
            "Water pump",
            WorkKind::Repair,
            None,
            Some(Payer::Manufacturer),
        )
        .await;
        let b = insert_service(&db, vid, "2026-10-20", Some(90_000), Payer::Owner).await;
        insert_line(
            &db,
            b,
            "Bodywork",
            WorkKind::Repair,
            Some(80_000),
            Some(Payer::Insurance),
        )
        .await;
        insert_line(&db, b, "Repair", WorkKind::Repair, Some(10_000), None).await;
        let c = insert_service(&db, vid, "2026-11-09", None, Payer::Manufacturer).await;
        insert_line(&db, c, "Recall", WorkKind::Recall, None, None).await;
        let d = insert_service(&db, vid, "2026-11-10", Some(50_000), Payer::Manufacturer).await;
        insert_line(
            &db,
            d,
            "Fee",
            WorkKind::Fee,
            Some(5_000),
            Some(Payer::Owner),
        )
        .await;
        insert_part(&db, vid, Supplier::Owner, 3_000, Some("2026-10-05"), None).await;

        let totals = summary(&db, vid).await.unwrap();
        assert_eq!(totals.total_cost_cents, 12_000 + 90_000 + 50_000 + 3_000);
        assert_eq!(totals.covered_cents, 80_000 + 45_000);
        assert_eq!(totals.out_of_pocket_cents, 12_000 + 10_000 + 5_000 + 3_000);
        assert_eq!(
            totals.out_of_pocket_cents + totals.covered_cents,
            totals.total_cost_cents
        );
        let months_total: i64 = totals.monthly_costs.iter().map(|m| m.total_cents).sum();
        let months_covered: i64 = totals.monthly_costs.iter().map(|m| m.covered_cents).sum();
        let months_oop: i64 = totals
            .monthly_costs
            .iter()
            .map(|m| m.out_of_pocket_cents)
            .sum();
        assert_eq!(
            months_total + totals.unbucketed_parts_cents,
            totals.total_cost_cents
        );
        assert_eq!(months_covered, totals.covered_cents);
        assert_eq!(months_oop, totals.out_of_pocket_cents);
        assert_eq!(totals.covered_work_count, 4);
    }

    async fn priced_service(
        db: &DatabaseConnection,
        vehicle_id: i32,
        day: &str,
        mileage: i32,
        total_cents: i32,
        paid_by: Payer,
    ) -> i32 {
        service_record::ActiveModel {
            vehicle_id: Set(vehicle_id),
            service_date: Set(date(day)),
            mileage: Set(Some(mileage)),
            total_cost_cents: Set(Some(total_cents)),
            paid_by: Set(paid_by),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn priced_line(
        db: &DatabaseConnection,
        service_id: i32,
        kind: WorkKind,
        cents: Option<i32>,
        paid_by: Option<Payer>,
    ) {
        service_record_line_item::ActiveModel {
            service_record_id: Set(service_id),
            description: Set("line".into()),
            work_kind: Set(kind),
            cost_cents: Set(cents),
            paid_by: Set(paid_by),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn dated_part(
        db: &DatabaseConnection,
        vehicle_id: i32,
        cents: i32,
        bought: Option<&str>,
    ) {
        part::ActiveModel {
            vehicle_id: Set(vehicle_id),
            name: Set("Part".into()),
            status: Set(PartStatus::Purchased),
            cost_cents: Set(Some(cents)),
            purchase_date: Set(bought.map(date)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    fn range(from: Option<&str>, to: Option<&str>) -> CostRange {
        CostRange {
            from: from.map(date),
            to: to.map(date),
        }
    }

    // Three services 2,000 miles apart and three parts, one of them undated.
    async fn seed_three_years(db: &DatabaseConnection) -> i32 {
        let vid = seed_vehicle(db, None).await;
        priced_service(db, vid, "2024-06-01", 10_000, 10_000, Payer::Owner).await;
        priced_service(db, vid, "2025-06-01", 12_000, 20_000, Payer::Owner).await;
        priced_service(db, vid, "2026-06-01", 14_000, 40_000, Payer::Owner).await;
        dated_part(db, vid, 1_000, Some("2024-07-01")).await;
        dated_part(db, vid, 2_000, Some("2025-07-01")).await;
        dated_part(db, vid, 4_000, None).await;
        vid
    }

    #[tokio::test]
    async fn a_range_counts_the_services_and_parts_dated_inside_it() {
        let db = test_db().await;
        let vid = seed_three_years(&db).await;

        let all = summary(&db, vid).await.unwrap();
        assert_eq!(all.from, None);
        assert_eq!(all.to, None);
        assert_eq!(all.total_cost_cents, 77_000);
        assert_eq!(all.unbucketed_parts_cents, 4_000);

        // Both ends are inside the range.
        let s = summary_in(&db, vid, range(Some("2025-06-01"), Some("2026-06-01")))
            .await
            .unwrap();
        assert_eq!(s.from, Some(date("2025-06-01")));
        assert_eq!(s.to, Some(date("2026-06-01")));
        assert_eq!(s.service_count, 2);
        assert_eq!(s.service_totals_cents, 60_000);
        assert_eq!(s.part_count, 1);
        assert_eq!(s.self_supplied_parts_cents, 2_000);
        assert_eq!(s.total_cost_cents, 62_000);
        assert_eq!(s.out_of_pocket_cents, 62_000);
        assert_eq!(s.unbucketed_parts_cents, 0);
        let months: Vec<&str> = s.monthly_costs.iter().map(|m| m.month.as_str()).collect();
        assert_eq!(months, ["2025-06", "2025-07", "2026-06"]);

        let until = summary_in(&db, vid, range(None, Some("2024-12-31")))
            .await
            .unwrap();
        assert_eq!(until.total_cost_cents, 11_000);
        let since = summary_in(&db, vid, range(Some("2026-01-01"), None))
            .await
            .unwrap();
        assert_eq!(since.total_cost_cents, 40_000);
        assert_eq!(since.part_count, 0);
    }

    #[tokio::test]
    async fn a_range_measures_the_miles_driven_inside_it() {
        let db = test_db().await;
        let vid = seed_three_years(&db).await;
        mileage_log::ActiveModel {
            vehicle_id: Set(vid),
            mileage: Set(15_000),
            recorded_at: Set(date("2026-09-01")),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let all = summary(&db, vid).await.unwrap();
        assert_eq!(all.miles_covered, Some(5_000));
        assert_eq!(all.history_starts_on, Some(date("2024-06-01")));

        // 62,000 cents over the 2,000 miles from the 2025 service to the 2026
        // one. The reading of September 2026 is outside the range.
        let s = summary_in(&db, vid, range(Some("2025-01-01"), Some("2026-06-30")))
            .await
            .unwrap();
        assert_eq!(s.miles_covered, Some(2_000));
        assert_eq!(s.history_starts_on, Some(date("2025-06-01")));
        assert_eq!(s.cost_per_1000_miles_cents, Some(31_000));

        // One service in the range covers no miles, so the rate is withheld.
        let one = summary_in(&db, vid, range(Some("2025-01-01"), Some("2025-12-31")))
            .await
            .unwrap();
        assert_eq!(one.total_cost_cents, 22_000);
        assert_eq!(one.miles_covered, None);
        assert_eq!(one.cost_per_1000_miles_cents, None);
    }

    #[tokio::test]
    async fn a_range_that_starts_after_it_ends_is_invalid() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let err = summary_in(&db, vid, range(Some("2026-02-01"), Some("2026-01-31")))
            .await
            .err()
            .unwrap();
        assert!(
            matches!(&err, DomainError::Invalid { field, .. } if field == "from"),
            "{err}"
        );
        assert!(
            summary_in(&db, vid, range(Some("2026-01-31"), Some("2026-01-31")))
                .await
                .is_ok()
        );
        let missing = summary_in(&db, 999, range(Some("2026-02-01"), Some("2026-01-31")))
            .await
            .err()
            .unwrap();
        assert!(matches!(missing, DomainError::NotFound(_)));
    }

    #[tokio::test]
    async fn work_kinds_split_the_lines_by_who_paid_each() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let mixed = priced_service(&db, vid, "2026-03-01", 10_000, 59_000, Payer::Owner).await;
        priced_line(
            &db,
            mixed,
            WorkKind::Repair,
            Some(45_000),
            Some(Payer::Manufacturer),
        )
        .await;
        priced_line(&db, mixed, WorkKind::Maintenance, Some(12_900), None).await;
        priced_line(&db, mixed, WorkKind::Fee, Some(1_100), None).await;
        let insured =
            priced_service(&db, vid, "2026-04-01", 11_000, 80_000, Payer::Insurance).await;
        priced_line(&db, insured, WorkKind::Repair, Some(70_000), None).await;
        priced_line(
            &db,
            insured,
            WorkKind::Repair,
            Some(10_000),
            Some(Payer::Owner),
        )
        .await;
        priced_line(&db, insured, WorkKind::Inspection, None, None).await;
        let earlier = priced_service(&db, vid, "2025-04-01", 9_000, 5_000, Payer::Owner).await;
        priced_line(&db, earlier, WorkKind::Mod, Some(5_000), None).await;

        let kind =
            |work_kind, line_count, cost_cents, out_of_pocket_cents, covered_cents| WorkKindCost {
                work_kind,
                line_count,
                cost_cents,
                out_of_pocket_cents,
                covered_cents,
            };
        let all = summary(&db, vid).await.unwrap();
        assert_eq!(
            all.by_work_kind,
            [
                kind(WorkKind::Maintenance, 1, 12_900, 12_900, 0),
                kind(WorkKind::Mod, 1, 5_000, 5_000, 0),
                kind(WorkKind::Repair, 3, 125_000, 10_000, 115_000),
                kind(WorkKind::Inspection, 1, 0, 0, 0),
                kind(WorkKind::Fee, 1, 1_100, 1_100, 0),
            ]
        );

        let this_year = summary_in(&db, vid, range(Some("2026-01-01"), None))
            .await
            .unwrap();
        let kinds: Vec<WorkKind> = this_year.by_work_kind.iter().map(|k| k.work_kind).collect();
        assert_eq!(
            kinds,
            [
                WorkKind::Maintenance,
                WorkKind::Repair,
                WorkKind::Inspection,
                WorkKind::Fee
            ]
        );
    }
}
