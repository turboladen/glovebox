//! Wear readings of brake pads, brake rotors and tire tread, and the outlook
//! that projects from them when each will reach its limit.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use sea_orm::{sea_query::Expr, *};
use serde::Serialize;

use crate::{
    entities::{
        part::{self, PartStatus},
        service_record,
        wear_reading::{self, Axle, LengthUnit, WearComponent},
    },
    error::{DomainError, DomainResult},
    inputs::wear::{NewWearReading, UpdateWearReading, WearFilter, WearTag},
    services::{
        reminders::{self, DueStatus, RemindersResponse},
        schedule::{DEFAULT_WARNING_DAYS, DEFAULT_WARNING_MILES},
    },
    units::{Measure, millimeters},
};

/// One wear reading as a reply shows it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WearReading {
    pub id: i32,
    pub vehicle_id: i32,
    /// The service that took the reading, if any.
    pub service_id: Option<i32>,
    pub component: WearComponent,
    pub axle: Axle,
    pub reading: Measure,
    /// The minimum the shop stated beside the reading.
    pub limit: Option<Measure>,
    pub measured_on: NaiveDate,
    pub odometer: Option<i32>,
    pub notes: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<wear_reading::Model> for WearReading {
    fn from(m: wear_reading::Model) -> Self {
        Self {
            id: m.id,
            vehicle_id: m.vehicle_id,
            service_id: m.service_id,
            component: m.component,
            axle: m.axle,
            reading: m.reading(),
            limit: m.limit(),
            measured_on: m.measured_on,
            odometer: m.odometer,
            notes: m.notes,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

fn not_found(id: i32) -> DomainError {
    DomainError::NotFound(format!("Wear reading {id} not found"))
}

fn validate_measure(field: &str, measure: Measure) -> DomainResult<()> {
    if measure.value.is_finite() && measure.value >= 0.0 {
        Ok(())
    } else {
        Err(DomainError::invalid(
            field,
            "must be a length of zero or more",
        ))
    }
}

fn validate_day(measured_on: NaiveDate) -> DomainResult<()> {
    // A reading dated ahead would be the newest of its series and every
    // projection would start from it.
    if measured_on > super::today() {
        return Err(DomainError::invalid(
            "measured_on",
            "must not be after today",
        ));
    }
    Ok(())
}

fn validate_odometer(odometer: Option<i32>) -> DomainResult<()> {
    if odometer.is_some_and(|o| o < 0) {
        return Err(DomainError::invalid("odometer", "must not be negative"));
    }
    Ok(())
}

/// Records a wear reading.
///
/// # Errors
/// `NotFound` for a vehicle that does not exist and for a service that is not
/// the vehicle's; `Invalid` for a negative or non-finite length, a negative
/// odometer, a day after today, and a reading with neither a day nor a
/// service to take one from.
pub async fn create<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    input: NewWearReading,
) -> DomainResult<WearReading> {
    let txn = super::begin_write(db).await?;
    super::vehicle::require(&txn, vehicle_id).await?;
    let service = match input.service_id {
        Some(id) => Some(super::service_record::require(&txn, vehicle_id, id).await?),
        None => None,
    };
    let out = insert(&txn, vehicle_id, service.as_ref(), input).await?;
    txn.commit().await?;
    Ok(out)
}

/// Inserts the readings a new service took, each dated by the service and
/// carrying its odometer unless the reading gives its own. Runs inside the
/// caller's transaction.
pub(crate) async fn insert_for_service(
    db: &impl ConnectionTrait,
    record: &service_record::Model,
    readings: Vec<NewWearReading>,
) -> DomainResult<Vec<WearReading>> {
    let mut out = Vec::with_capacity(readings.len());
    for reading in readings {
        out.push(insert(db, record.vehicle_id, Some(record), reading).await?);
    }
    Ok(out)
}

async fn insert(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    service: Option<&service_record::Model>,
    input: NewWearReading,
) -> DomainResult<WearReading> {
    validate_measure("reading", input.reading)?;
    if let Some(limit) = input.limit {
        validate_measure("limit", limit)?;
    }
    validate_odometer(input.odometer)?;
    // A day taken from the service follows the service's own date rule.
    if let Some(day) = input.measured_on {
        validate_day(day)?;
    }
    let measured_on = input
        .measured_on
        .or(service.map(|s| s.service_date))
        .ok_or_else(|| {
            DomainError::invalid("measured_on", "is needed for a reading no service took")
        })?;
    let mut model = wear_reading::ActiveModel {
        vehicle_id: Set(vehicle_id),
        service_id: Set(service.map(|s| s.id)),
        component: Set(input.component),
        axle: Set(input.axle),
        measured_on: Set(measured_on),
        odometer: Set(input.odometer.or(service.and_then(|s| s.mileage))),
        notes: Set(input.notes.as_deref().and_then(super::trim_to_none)),
        ..Default::default()
    };
    write_reading(&mut model, input.reading);
    write_limit(&mut model, input.limit);
    Ok(model.insert(db).await?.into())
}

// A measure's two columns are written only by these two, so a value never
// goes in without its unit.
fn write_reading(active: &mut wear_reading::ActiveModel, reading: Measure) {
    active.value = Set(reading.value);
    active.unit = Set(reading.unit);
}

fn write_limit(active: &mut wear_reading::ActiveModel, limit: Option<Measure>) {
    active.limit_value = Set(limit.map(|l| l.value));
    active.limit_unit = Set(limit.map(|l| l.unit));
}

/// Moves the readings a service took that carry its old day or odometer to
/// the corrected ones, as its mileage log follows it. A reading that gave its
/// own day or odometer keeps it. Runs inside the caller's transaction.
pub(crate) async fn follow_service(
    db: &impl ConnectionTrait,
    before: (NaiveDate, Option<i32>),
    record: &service_record::Model,
) -> DomainResult<()> {
    let (day, mileage) = before;
    if day != record.service_date {
        wear_reading::Entity::update_many()
            .col_expr(
                wear_reading::Column::MeasuredOn,
                Expr::value(record.service_date),
            )
            .col_expr(wear_reading::Column::UpdatedAt, Expr::value(super::now()))
            .filter(wear_reading::Column::ServiceId.eq(record.id))
            .filter(wear_reading::Column::MeasuredOn.eq(day))
            .exec(db)
            .await?;
    }
    if mileage != record.mileage {
        let mut query = wear_reading::Entity::update_many()
            .col_expr(wear_reading::Column::Odometer, Expr::value(record.mileage))
            .col_expr(wear_reading::Column::UpdatedAt, Expr::value(super::now()))
            .filter(wear_reading::Column::ServiceId.eq(record.id));
        query = match mileage {
            Some(miles) => query.filter(wear_reading::Column::Odometer.eq(miles)),
            None => query.filter(wear_reading::Column::Odometer.is_null()),
        };
        query.exec(db).await?;
    }
    Ok(())
}

async fn require(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<wear_reading::Model> {
    wear_reading::Entity::find_by_id(id)
        .filter(wear_reading::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| not_found(id))
}

/// Corrects a wear reading. A reading of another vehicle reads as one that
/// does not exist.
///
/// # Errors
/// `NotFound` for a reading or a service that is not the vehicle's;
/// `Invalid` as [`create`] gives it.
pub async fn update<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    input: UpdateWearReading,
) -> DomainResult<WearReading> {
    let txn = super::begin_write(db).await?;
    let existing = require(&txn, vehicle_id, id).await?;
    if let Some(Some(service_id)) = input.service_id {
        super::service_record::require(&txn, vehicle_id, service_id).await?;
    }
    let mut active: wear_reading::ActiveModel = existing.into();
    if let Some(v) = input.component {
        active.component = Set(v);
    }
    if let Some(v) = input.axle {
        active.axle = Set(v);
    }
    if let Some(v) = input.reading {
        validate_measure("reading", v)?;
        write_reading(&mut active, v);
    }
    if let Some(v) = input.limit {
        if let Some(limit) = v {
            validate_measure("limit", limit)?;
        }
        write_limit(&mut active, v);
    }
    if let Some(v) = input.measured_on {
        validate_day(v)?;
        active.measured_on = Set(v);
    }
    if let Some(v) = input.odometer {
        validate_odometer(v)?;
        active.odometer = Set(v);
    }
    if let Some(v) = input.service_id {
        active.service_id = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v.as_deref().and_then(super::trim_to_none));
    }
    active.updated_at = Set(super::now());
    let out = active.update(&txn).await?;
    txn.commit().await?;
    Ok(out.into())
}

/// Deletes a wear reading. A reading of another vehicle reads as one that
/// does not exist.
pub async fn delete<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<()> {
    let txn = super::begin_write(db).await?;
    require(&txn, vehicle_id, id).await?.delete(&txn).await?;
    txn.commit().await?;
    Ok(())
}

/// The vehicle's wear readings, newest first (by day, then id).
///
/// # Errors
/// `NotFound` for a vehicle that does not exist and for a `service_id` that is
/// not the vehicle's; `Invalid` for a limit of zero.
pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    filter: WearFilter,
) -> DomainResult<Vec<WearReading>> {
    super::vehicle::require(db, vehicle_id).await?;
    super::validate_limit(filter.limit)?;
    let mut query =
        wear_reading::Entity::find().filter(wear_reading::Column::VehicleId.eq(vehicle_id));
    if let Some(component) = filter.component {
        query = query.filter(wear_reading::Column::Component.eq(component));
    }
    if let Some(axle) = filter.axle {
        query = query.filter(wear_reading::Column::Axle.eq(axle));
    }
    if let Some(service_id) = filter.service_id {
        super::service_record::require(db, vehicle_id, service_id).await?;
        query = query.filter(wear_reading::Column::ServiceId.eq(service_id));
    }
    query = query
        .order_by_desc(wear_reading::Column::MeasuredOn)
        .order_by_desc(wear_reading::Column::Id);
    if let Some(limit) = filter.limit {
        query = query.limit(u64::from(limit));
    }
    Ok(query.all(db).await?.into_iter().map(Into::into).collect())
}

/// The readings each named service took, oldest first, keyed by service.
pub(crate) async fn by_service(
    db: &impl ConnectionTrait,
    service_ids: impl IntoIterator<Item = i32>,
) -> DomainResult<BTreeMap<i32, Vec<WearReading>>> {
    let ids: Vec<i32> = service_ids.into_iter().collect();
    let mut out: BTreeMap<i32, Vec<WearReading>> = BTreeMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let rows = wear_reading::Entity::find()
        .filter(wear_reading::Column::ServiceId.is_in(ids))
        .order_by_asc(wear_reading::Column::Id)
        .all(db)
        .await?;
    for row in rows {
        if let Some(service_id) = row.service_id {
            out.entry(service_id).or_default().push(row.into());
        }
    }
    Ok(out)
}

/// Deletes the readings a service took. Runs inside the caller's
/// transaction, before the service row goes.
pub(crate) async fn delete_for_service(
    db: &impl ConnectionTrait,
    service_id: i32,
) -> DomainResult<()> {
    wear_reading::Entity::delete_many()
        .filter(wear_reading::Column::ServiceId.eq(service_id))
        .exec(db)
        .await?;
    Ok(())
}

/// The ids of the readings a service took, oldest first.
pub(crate) async fn ids_of_service(
    db: &impl ConnectionTrait,
    service_id: i32,
) -> DomainResult<Vec<i32>> {
    Ok(wear_reading::Entity::find()
        .select_only()
        .column(wear_reading::Column::Id)
        .filter(wear_reading::Column::ServiceId.eq(service_id))
        .order_by_asc(wear_reading::Column::Id)
        .into_tuple()
        .all(db)
        .await?)
}

crate::vocab::vocabulary! {
    /// Where an outlook's limit came from.
    // No column stores these texts.
    pub enum LimitSource {
        // The newest limit a shop stated in the series.
        Shop => "shop",
        // The default for the component, when no shop stated one.
        Default => "default",
    }
}

/// The limit a component is judged against when no shop has stated one.
/// Rotors have none: a rotor's minimum is stamped on the rotor.
pub const fn default_limit(component: WearComponent) -> Option<Measure> {
    match component {
        WearComponent::BrakePads => Some(Measure::new(3.0, LengthUnit::Millimeter)),
        WearComponent::TireTread => Some(Measure::new(2.0, LengthUnit::ThirtySecondInch)),
        WearComponent::BrakeRotors => None,
    }
}

/// How one series of readings stands: the brake pads or rotors of one axle,
/// or the tires as a set.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WearOutlook {
    pub component: WearComponent,
    /// The axle of a brake series; absent for the tires.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub axle: Option<Axle>,
    /// The newest reading, the lowest of that visit for the tires.
    pub latest: Measure,
    pub latest_on: NaiveDate,
    pub limit: Option<Measure>,
    pub limit_source: Option<LimitSource>,
    /// How many readings the series counts, one per visit for the tires.
    pub readings_in_series: usize,
    /// The day a tagged part was installed, from which the series counts.
    /// Absent when no tagged install is recorded and every reading counts.
    pub series_starts_on: Option<NaiveDate>,
    /// The wear over 10,000 miles, in the latest reading's unit.
    pub wear_per_10k_miles: Option<Measure>,
    /// Miles from the vehicle's estimated mileage until the limit, negative
    /// once it is projected past.
    pub miles_left: Option<i32>,
    /// The day the limit is projected to be reached at the vehicle's daily
    /// rate, a past day once it is projected past.
    pub due_on: Option<NaiveDate>,
    pub status: DueStatus,
    /// Why there is no projection, when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The fewest readings a series needs before it is projected.
const MIN_READINGS_FOR_RATE: usize = 2;

/// The outlook of each series of the vehicle's readings, most urgent first,
/// as of `today` and the estimated mileage `reminders` holds. A series whose
/// every reading predates the install of a part tagged for it is left out:
/// the part it measured is off the car.
pub async fn outlook(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    reminders: &RemindersResponse,
    today: NaiveDate,
) -> DomainResult<Vec<WearOutlook>> {
    let readings: Vec<wear_reading::Model> = wear_reading::Entity::find()
        .filter(wear_reading::Column::VehicleId.eq(vehicle_id))
        .order_by_asc(wear_reading::Column::MeasuredOn)
        .order_by_asc(wear_reading::Column::Id)
        .all(db)
        .await?;
    if readings.is_empty() {
        return Ok(Vec::new());
    }
    let installs = tagged_installs(db, vehicle_id).await?;
    // A reading a service took carries its mileage, so the log is read only
    // for one that has none.
    let odometers = if readings.iter().any(|r| r.odometer.is_none()) {
        let vehicle = super::vehicle::require(db, vehicle_id).await?;
        reminders::dated_readings(db, &vehicle, today).await?
    } else {
        Vec::new()
    };
    Ok(outlook_from(
        readings,
        &installs,
        &odometers,
        reminders.estimated_mileage,
        reminders.avg_daily_miles,
        today,
    ))
}

/// Where a series starts: the install of a tagged part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Install {
    day: NaiveDate,
    /// The service that installed the part. Its own readings measured the
    /// part that came off.
    service_id: Option<i32>,
}

/// The newest install of each tag among the vehicle's parts that are or were
/// on the car.
async fn tagged_installs(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<BTreeMap<WearTag, Install>> {
    let parts = part::Entity::find()
        .filter(part::Column::VehicleId.eq(vehicle_id))
        .filter(part::Column::WearsComponent.is_not_null())
        .filter(part::Column::Status.is_in([PartStatus::Installed, PartStatus::Replaced]))
        .all(db)
        .await?;
    let mut out: BTreeMap<WearTag, Install> = BTreeMap::new();
    for p in parts {
        let (Some(tag), Some(day)) = (
            WearTag::of_columns(p.wears_component, p.wears_axle),
            p.installed_date,
        ) else {
            continue;
        };
        let install = Install {
            day,
            service_id: p.installed_service_id,
        };
        let newest = out.entry(tag).or_insert(install);
        if install.day > newest.day {
            *newest = install;
        }
    }
    Ok(out)
}

/// One visit's reading in a series: for the tires, the lowest of that day.
struct Point {
    day: NaiveDate,
    odometer: Option<i32>,
    reading: Measure,
    limit: Option<Measure>,
}

/// The outlooks [`outlook`] returns, judged from rows already read.
fn outlook_from(
    readings: Vec<wear_reading::Model>,
    installs: &BTreeMap<WearTag, Install>,
    odometers: &[(NaiveDate, i32)],
    estimated_mileage: i32,
    avg_daily_miles: f64,
    today: NaiveDate,
) -> Vec<WearOutlook> {
    // The car has driven at least as far as its newest reading, whatever the
    // mileage log says.
    let estimated_mileage = readings
        .iter()
        .filter_map(|r| r.odometer)
        .fold(estimated_mileage, Ord::max);
    let mut series: BTreeMap<WearTag, Vec<wear_reading::Model>> = BTreeMap::new();
    for r in readings {
        series
            .entry(WearTag::of_reading(r.component, r.axle))
            .or_default()
            .push(r);
    }
    let mut out: Vec<WearOutlook> = series
        .into_iter()
        .filter_map(|(tag, rows)| {
            let install = installs.get(&tag).copied();
            let points = points_of(rows, install, odometers, avg_daily_miles);
            judge_series(
                tag,
                install.map(|i| i.day),
                &points,
                estimated_mileage,
                avg_daily_miles,
                today,
            )
        })
        .collect();
    out.sort_by_key(|o| o.status.urgency_rank());
    out
}

// Readings before the series starts are history only, and so are the ones
// the installing service took, which measured the part that came off. The
// rows arrive oldest first, so the points do too; the tires keep the lowest
// reading of each day. An odometer is estimated only for a point that still
// has none once its rows are merged.
fn points_of(
    rows: Vec<wear_reading::Model>,
    install: Option<Install>,
    odometers: &[(NaiveDate, i32)],
    avg_daily_miles: f64,
) -> Vec<Point> {
    let mut points: Vec<Point> = Vec::new();
    for r in rows {
        if install.is_some_and(|i| {
            r.measured_on < i.day || (i.service_id.is_some() && r.service_id == i.service_id)
        }) {
            continue;
        }
        let point = Point {
            day: r.measured_on,
            odometer: r.odometer,
            reading: r.reading(),
            limit: r.limit(),
        };
        match points.last_mut() {
            Some(last) if r.component == WearComponent::TireTread && last.day == point.day => {
                if point.reading.length() < last.reading.length() {
                    last.reading = point.reading;
                }
                last.limit = point.limit.or(last.limit);
                last.odometer = last.odometer.or(point.odometer);
            }
            _ => points.push(point),
        }
    }
    for p in &mut points {
        if p.odometer.is_none() {
            p.odometer = reminders::odometer_on(p.day, odometers, avg_daily_miles);
        }
    }
    points
}

/// The least-squares slope of millimeters over miles, or `None` when the
/// points span no miles.
#[expect(
    clippy::cast_precision_loss,
    reason = "a series holds a handful of readings, far below f64's exact range"
)]
fn slope_mm_per_mile(points: &[(f64, f64)]) -> Option<f64> {
    let n = points.len() as f64;
    let mean_x = points.iter().map(|p| p.0).sum::<f64>() / n;
    let mean_y = points.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = points.iter().map(|p| (p.0 - mean_x).powi(2)).sum();
    let sxy: f64 = points.iter().map(|p| (p.0 - mean_x) * (p.1 - mean_y)).sum();
    (sxx > 0.0).then(|| sxy / sxx)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "miles left is bounded by the f64 range check before the cast"
)]
fn judge_series(
    tag: WearTag,
    starts_on: Option<NaiveDate>,
    points: &[Point],
    estimated_mileage: i32,
    avg_daily_miles: f64,
    today: NaiveDate,
) -> Option<WearOutlook> {
    let latest = points.last()?;
    let component = tag.component();
    let (limit, limit_source) = points
        .iter()
        .rev()
        .find_map(|p| p.limit)
        .map(|l| (l, LimitSource::Shop))
        .or_else(|| default_limit(component).map(|l| (l, LimitSource::Default)))
        .unzip();
    let mut outlook = WearOutlook {
        component,
        axle: tag.axle(),
        latest: latest.reading,
        latest_on: latest.day,
        limit,
        limit_source,
        readings_in_series: points.len(),
        series_starts_on: starts_on,
        wear_per_10k_miles: None,
        miles_left: None,
        due_on: None,
        status: DueStatus::NotDue,
        note: None,
    };
    let Some(limit) = limit else {
        outlook.note = Some(
            "No limit is known: record the minimum thickness stamped on the rotor with a reading"
                .into(),
        );
        return Some(outlook);
    };
    let latest_mm = millimeters(latest.reading.length());
    let limit_mm = millimeters(limit.length());
    if latest_mm <= limit_mm {
        outlook.status = DueStatus::Overdue;
    }

    let fitted: Vec<(f64, f64)> = points
        .iter()
        .filter_map(|p| {
            p.odometer
                .map(|o| (f64::from(o), millimeters(p.reading.length())))
        })
        .collect();
    if points.len() < MIN_READINGS_FOR_RATE {
        outlook.note = Some(match starts_on {
            Some(day) => format!("Only one reading since the part installed on {day}"),
            None => "One reading is not enough to tell a wear rate".into(),
        });
        return Some(outlook);
    }
    if fitted.len() < MIN_READINGS_FOR_RATE {
        outlook.note = Some(
            "The readings carry no odometer and none can be estimated from the mileage log".into(),
        );
        return Some(outlook);
    }
    let Some(wear_per_mile) = slope_mm_per_mile(&fitted).map(|s| -s).filter(|w| *w > 0.0) else {
        outlook.note = Some(
            "The readings do not show wear; a replacement may be missing from the parts".into(),
        );
        return Some(outlook);
    };
    outlook.wear_per_10k_miles = Some(Measure::of_length(
        Measure::new(wear_per_mile * 10_000.0, LengthUnit::Millimeter).length(),
        latest.reading.unit,
    ));

    let from_latest = (latest_mm - limit_mm) / wear_per_mile;
    let Some(latest_odometer) = latest.odometer else {
        outlook.note = Some(
            "The newest reading carries no odometer and none can be estimated from the mileage log"
                .into(),
        );
        return Some(outlook);
    };
    let left = f64::from(latest_odometer) + from_latest - f64::from(estimated_mileage);
    if !left.is_finite() || left.abs() > f64::from(i32::MAX) {
        return Some(outlook);
    }
    let left = left.round() as i32;
    outlook.miles_left = Some(left);
    let by_miles = DueStatus::judge(i64::from(left), i64::from(DEFAULT_WARNING_MILES));
    // A limit projected past was reached that many days before today.
    let days = reminders::days_to_drive(left.abs(), avg_daily_miles)
        .map(|d| if left < 0 { -d } else { d });
    outlook.due_on = days.map(|d| today + chrono::Duration::days(d));
    let by_days = days.map_or(DueStatus::NotDue, |d| {
        DueStatus::judge(d, i64::from(DEFAULT_WARNING_DAYS))
    });
    outlook.status = DueStatus::most_urgent([outlook.status, by_miles, by_days]);
    Some(outlook)
}

#[cfg(test)]
mod tests;
