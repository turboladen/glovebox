//! Made-up data for a test environment, so the web page can be developed and
//! shown without the owner's real records.

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{
        self,
        incident::IncidentCategory,
        job_need::Approach,
        need_option::Fitment,
        part::Supplier,
        service_record::{Payer, Performer},
        service_record_line_item::WorkKind,
    },
    error::{DomainError, DomainResult},
    inputs::{
        appointment::NewAppointment,
        incident::NewIncident,
        mileage::NewMileageEntry,
        need::{NewNeed, NewOption, OrderDetails, ReturnInput, UpdateOption},
        part::{NewPartContent, UpdatePart},
        schedule::NewScheduleItem,
        service_record::{NewLineItem, NewServiceRecord},
        shop::{NewShop, ShopChoice},
        todo::NewTodo,
        vehicle::NewVehicle,
        work_item::NewWorkItem,
    },
    services::{
        appointment, incident, mileage, model_template, need, nhtsa::RecallInfo, part, research,
        schedule, service_record, shop, todo, vehicle, work_item,
    },
};

/// How many rows of each kind [`demo`] created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SeedSummary {
    pub vehicles: u64,
    pub appointments: u64,
    pub jobs: u64,
    pub needs: u64,
    pub options: u64,
    pub parts: u64,
    pub todos: u64,
    pub services: u64,
    /// Counts every odometer reading, the ones the services logged included.
    pub readings: u64,
    /// Counts the incidents that are not notes.
    pub incidents: u64,
    pub notes: u64,
    pub recalls: u64,
}

/// Fills an empty non-live database with made-up data: one vehicle with a
/// year of odometer readings, one appointment with four jobs, two
/// unscheduled jobs, needs in all six stages, two services, incidents that
/// are open and resolved, a note and an open recall. Every date counts from
/// `today`, and no part of the demo reads the clock, so two runs given the
/// same day write the same records. Everything is written in one transaction
/// through the same services the tools use, so a failure leaves the database
/// empty. The caller is responsible for never pointing this at the live
/// database.
///
/// # Errors
///
/// `BadRequest` when any vehicle exists. `Invalid` when `today` is after the
/// current day, because an odometer reading dated ahead is refused.
pub async fn demo<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    today: chrono::NaiveDate,
) -> DomainResult<SeedSummary> {
    let txn = crate::services::begin_write(db).await?;
    if entities::vehicle::Entity::find().count(&txn).await? > 0 {
        return Err(DomainError::BadRequest(
            "this database already has a vehicle; seed an empty environment".into(),
        ));
    }
    build(&txn, today).await?;
    let notes = entities::incident::Entity::find()
        .filter(entities::incident::Column::Category.eq(IncidentCategory::Note))
        .count(&txn)
        .await?;
    let summary = SeedSummary {
        vehicles: entities::vehicle::Entity::find().count(&txn).await?,
        appointments: entities::appointment::Entity::find().count(&txn).await?,
        jobs: entities::work_item::Entity::find().count(&txn).await?,
        needs: entities::job_need::Entity::find().count(&txn).await?,
        options: entities::need_option::Entity::find().count(&txn).await?,
        parts: entities::part::Entity::find().count(&txn).await?,
        todos: entities::todo::Entity::find().count(&txn).await?,
        services: entities::service_record::Entity::find().count(&txn).await?,
        readings: entities::mileage_log::Entity::find().count(&txn).await?,
        incidents: entities::incident::Entity::find().count(&txn).await? - notes,
        notes,
        recalls: entities::research_finding::Entity::find()
            .filter(
                entities::research_finding::Column::Category
                    .eq(entities::research_finding::FindingCategory::Recall),
            )
            .count(&txn)
            .await?,
    };
    txn.commit().await?;
    Ok(summary)
}

// One researched candidate with the fields the sentences are built from.
fn option(description: &str, manufacturer: &str, part_number: &str, price_cents: i32) -> NewOption {
    NewOption {
        description: description.into(),
        manufacturer: Some(manufacturer.into()),
        part_number: Some(part_number.into()),
        seller: Some("FCP Euro".into()),
        price_cents: Some(price_cents),
        fitment: Some(Fitment::Confirmed),
        ..Default::default()
    }
}

async fn job(
    db: &(impl ConnectionTrait + TransactionTrait),
    vehicle_id: i32,
    input: NewWorkItem,
) -> DomainResult<i32> {
    Ok(work_item::create(db, vehicle_id, input).await?.id)
}

async fn self_need(
    db: &(impl ConnectionTrait + TransactionTrait),
    vehicle_id: i32,
    job_id: i32,
    name: &str,
) -> DomainResult<i32> {
    let created = need::add(
        db,
        vehicle_id,
        job_id,
        NewNeed {
            name: name.into(),
            supplied_by: Some(Supplier::Owner),
            approach: Some(Approach::Replacement),
            ..Default::default()
        },
    )
    .await?;
    Ok(created.id)
}

// Adds `candidate` to the need and chooses it on `on`.
async fn choose(
    db: &(impl ConnectionTrait + TransactionTrait),
    vehicle_id: i32,
    need_id: i32,
    candidate: NewOption,
    reason: &str,
    on: chrono::NaiveDate,
) -> DomainResult<i32> {
    let added = need::add_option(db, vehicle_id, need_id, candidate).await?;
    need::decide_on(
        db,
        vehicle_id,
        need_id,
        Some(added.id),
        Some(reason.into()),
        on,
    )
    .await?;
    Ok(added.id)
}

// The days of one purchase, in the order they must fall.
#[derive(Clone, Copy)]
struct Bought {
    decided: chrono::NaiveDate,
    ordered: chrono::NaiveDate,
    received: chrono::NaiveDate,
}

// Chooses `candidate`, orders it from FCP Euro and records its arrival, each
// on its day of `days`. The result is the id of the part the order created.
async fn order_and_receive(
    db: &(impl ConnectionTrait + TransactionTrait),
    vehicle_id: i32,
    need_id: i32,
    candidate: NewOption,
    order_ref: &str,
    contents: Vec<NewPartContent>,
    days: Bought,
) -> DomainResult<i32> {
    choose(
        db,
        vehicle_id,
        need_id,
        candidate,
        "Matches the original part.",
        days.decided,
    )
    .await?;
    order(db, vehicle_id, need_id, order_ref, contents, days.ordered).await?;
    let received = need::mark_received(db, vehicle_id, need_id, Some(days.received)).await?;
    Ok(received.part.part.id)
}

async fn order(
    db: &(impl ConnectionTrait + TransactionTrait),
    vehicle_id: i32,
    need_id: i32,
    order_ref: &str,
    contents: Vec<NewPartContent>,
    on: chrono::NaiveDate,
) -> DomainResult<()> {
    let details = OrderDetails {
        order_ref: Some(order_ref.into()),
        purchase_date: Some(on),
        contents,
        ..Default::default()
    };
    need::mark_ordered(db, vehicle_id, need_id, details).await?;
    Ok(())
}

// The demo car, its appointment and the day every date counts from.
struct Visit {
    vehicle: i32,
    appointment: i32,
    today: chrono::NaiveDate,
}

impl Visit {
    // The day `days` before the demo's day.
    fn days_ago(&self, days: i64) -> chrono::NaiveDate {
        self.today - chrono::Duration::days(days)
    }
}

async fn build(
    db: &(impl ConnectionTrait + TransactionTrait),
    today: chrono::NaiveDate,
) -> DomainResult<()> {
    let template = model_template::list(db)
        .await?
        .into_iter()
        .find(|t| t.model.as_deref() == Some("Golf GTI"))
        .ok_or_else(|| DomainError::Internal("the Golf GTI template is not seeded".into()))?;
    // Bought just under two years back, so the 24-month items are due soon,
    // the 12-month items are overdue and the rest are not due. The odometer
    // on the demo's day is about 60,050 miles: the last reading is
    // 59,900 miles twelve days back, at 13 or so miles a day.
    let car = vehicle::create(
        db,
        NewVehicle {
            name: "Demo GTI".into(),
            model_template_id: Some(template.id),
            color: Some("Demo gray".into()),
            purchase_date: Some(today - chrono::Duration::days(710)),
            purchase_mileage: Some(52_000),
            notes: Some("Made-up vehicle for trying out the checklist.".into()),
            ..Default::default()
        },
    )
    .await?;
    let v = car.id;
    let shop = demo_shop(db).await?;
    let visit = appointment::create(
        db,
        v,
        NewAppointment {
            purpose: "60k service + loose ends".into(),
            shop: Some(ShopChoice::Id(shop)),
            planned_date: Some(today + chrono::Duration::days(14)),
            ..Default::default()
        },
    )
    .await?;
    let at = Visit {
        vehicle: v,
        appointment: visit.id,
        today,
    };
    price_schedule(db, v).await?;
    readings(db, &at).await?;
    shop_service(db, &at, shop).await?;
    brakes(db, &at).await?;
    oil_change(db, &at).await?;
    small_jobs(db, &at).await?;
    incidents(db, &at).await?;
    recall(db, v).await?;
    todos(db, &at).await?;
    unscheduled(db, v).await
}

// The shop directory is not per vehicle, so an earlier demo car may have left
// its shop behind.
async fn demo_shop(db: &(impl ConnectionTrait + TransactionTrait)) -> DomainResult<i32> {
    let existing = shop::list(db)
        .await?
        .into_iter()
        .find(|s| s.shop.name == "Demo Auto Works");
    if let Some(listed) = existing {
        return Ok(listed.shop.id);
    }
    let created = shop::create(
        db,
        NewShop {
            name: "Demo Auto Works".into(),
            address: Some("1 Example Way".into()),
            ..Default::default()
        },
    )
    .await?;
    Ok(created.id)
}

// The template's items carry no cost, so the car gets its own priced copy of
// three of them and the forecast has both priced and unpriced maintenance.
async fn price_schedule(
    db: &(impl ConnectionTrait + TransactionTrait),
    v: i32,
) -> DomainResult<()> {
    for (name, cents) in [
        ("Oil & Filter Change", 9_500),
        ("Tire Rotation", 4_000),
        ("Brake Fluid Flush", 14_000),
    ] {
        schedule::create_for_vehicle(
            db,
            v,
            NewScheduleItem {
                name: name.into(),
                est_cost_cents: Some(cents),
                ..Default::default()
            },
        )
        .await?;
    }
    Ok(())
}

// The id of the schedule item the car tracks under `name`.
async fn schedule_item(db: &impl ConnectionTrait, v: i32, name: &str) -> DomainResult<i32> {
    schedule::resolve(db, v)
        .await?
        .into_iter()
        .find(|r| r.effective_item.name == name)
        .map(|r| r.effective_item.id)
        .ok_or_else(|| DomainError::Internal(format!("the demo car has no {name} item")))
}

// Readings the owner logged by hand. The two services log one each, and
// together they give the daily rate behind the page's mileage estimate.
async fn readings(db: &(impl ConnectionTrait + TransactionTrait), at: &Visit) -> DomainResult<()> {
    for (age, miles, notes) in [
        (330, 55_600, None),
        (240, 56_500, Some("Before the road trip")),
        (90, 58_300, None),
        (35, 59_500, None),
    ] {
        mileage::create(
            db,
            at.vehicle,
            NewMileageEntry {
                mileage: miles,
                recorded_at: Some(at.days_ago(age).into()),
                source: None,
                notes: notes.map(Into::into),
            },
        )
        .await?;
    }
    Ok(())
}

// An invoice from the demo shop with lines of three kinds, one of them paid
// by the manufacturer, which resolves an incident reported before it.
async fn shop_service(
    db: &(impl ConnectionTrait + TransactionTrait),
    at: &Visit,
    shop: i32,
) -> DomainResult<()> {
    let v = at.vehicle;
    let smell = incident::create(
        db,
        v,
        NewIncident {
            category: IncidentCategory::Leak,
            title: "Coolant smell after a long drive".into(),
            description: Some("Sweet smell from the engine bay, no puddle under the car.".into()),
            occurred_at: Some(at.days_ago(175).into()),
            ..Default::default()
        },
    )
    .await?;
    let line = |description: &str, work_kind, cost_cents, paid_by| NewLineItem {
        description: description.into(),
        work_kind,
        paid_by,
        quantity: None,
        unit_cost_cents: None,
        cost_cents: Some(cost_cents),
    };
    service_record::create(
        db,
        v,
        NewServiceRecord {
            service_date: at.days_ago(160),
            performed_by: Some(Performer::Shop),
            shop: Some(ShopChoice::Id(shop)),
            mileage: Some(57_400),
            description: Some("Coolant leak repair and state inspection".into()),
            invoice_ref: Some("DAW-2291".into()),
            total_cost_cents: Some(45_500),
            line_items: Some(vec![
                line(
                    "Replace the coolant hose clamp",
                    WorkKind::Repair,
                    18_000,
                    None,
                ),
                line("State inspection", WorkKind::Inspection, 3_500, None),
                line(
                    "Rear wiper motor, replaced under warranty",
                    WorkKind::Repair,
                    24_000,
                    Some(Payer::Manufacturer),
                ),
            ]),
            resolved_incident_ids: Some(vec![smell.incident.id]),
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}

// Two open incidents and a note. The clunk has a job on the appointment, so
// the work sheet prints it as the reported symptom, and the rock chip has no
// job, so the home page lists it.
async fn incidents(db: &(impl ConnectionTrait + TransactionTrait), at: &Visit) -> DomainResult<()> {
    let v = at.vehicle;
    let clunk = incident::create(
        db,
        v,
        NewIncident {
            category: IncidentCategory::Noise,
            title: "Clunk over bumps, front left".into(),
            description: Some(
                "Loudest over speed bumps taken slowly with the wheel turned.".into(),
            ),
            occurred_at: Some(at.days_ago(9).into()),
            ..Default::default()
        },
    )
    .await?;
    job(
        db,
        v,
        NewWorkItem {
            title: "Find the front-end clunk".into(),
            incident_id: Some(clunk.incident.id),
            est_cost_cents: Some(15_000),
            appointment_id: Some(at.appointment),
            ..Default::default()
        },
    )
    .await?;
    for (category, title, age) in [
        (IncidentCategory::Damage, "Rock chip in the windshield", 40),
        (
            IncidentCategory::Note,
            "Winter wheels are in the garage loft",
            60,
        ),
    ] {
        incident::create(
            db,
            v,
            NewIncident {
                category,
                title: title.into(),
                occurred_at: Some(at.days_ago(age).into()),
                ..Default::default()
            },
        )
        .await?;
    }
    Ok(())
}

// A recall stored the way a recall check stores what NHTSA lists. The
// campaign number is made up.
async fn recall(db: &(impl ConnectionTrait + TransactionTrait), v: i32) -> DomainResult<()> {
    let found = RecallInfo {
        campaign_number: "00V000000".into(),
        manufacturer: Some("Demo Motors".into()),
        title: "Fuel System, Gasoline: Delivery: Fuel Pump".into(),
        summary: Some("The fuel pump control module may fail (made-up recall).".into()),
        consequence: Some("The engine may stall.".into()),
        remedy: Some("The dealer replaces the module at no charge.".into()),
        report_date: None,
        component: Some("FUEL SYSTEM, GASOLINE:DELIVERY:FUEL PUMP".into()),
        action_number: None,
    };
    research::record_recalls(db, v, &[found]).await?;
    Ok(())
}

// A pair, with three candidates: one chosen, one that does not fit and one
// rejected.
async fn rear_rotors(
    db: &(impl ConnectionTrait + TransactionTrait),
    at: &Visit,
    brakes: i32,
) -> DomainResult<()> {
    let v = at.vehicle;
    let rear_rotors = need::add(
        db,
        v,
        brakes,
        NewNeed {
            name: "Rear rotors".into(),
            quantity: Some(2),
            supplied_by: Some(Supplier::Owner),
            approach: Some(Approach::Replacement),
        },
    )
    .await?
    .id;
    let chosen = need::add_option(
        db,
        v,
        rear_rotors,
        option(
            "Rear rotor, 310 x 22 mm",
            "Volkswagen",
            "5Q0615601E",
            13_800,
        ),
    )
    .await?;
    need::add_option(
        db,
        v,
        rear_rotors,
        NewOption {
            fitment: Some(Fitment::DoesNotFit),
            fitment_note: Some("272 x 10 mm, for cars without the Performance Pack".into()),
            ..option(
                "Rear rotor, 272 x 10 mm",
                "Zimmermann",
                "600.3241.20",
                11_200,
            )
        },
    )
    .await?;
    let cheap = need::add_option(
        db,
        v,
        rear_rotors,
        option("Uncoated rear rotor, 310 x 22 mm", "Pagid", "55155", 7_900),
    )
    .await?;
    need::update_option(
        db,
        v,
        cheap.id,
        UpdateOption {
            rejected_reason: Some(Some("Uncoated hats rust within a season.".into())),
            ..Default::default()
        },
    )
    .await?;
    need::decide_on(
        db,
        v,
        rear_rotors,
        Some(chosen.id),
        Some("Coated hats resist rust.".into()),
        at.days_ago(2),
    )
    .await?;
    Ok(())
}

async fn brakes(db: &(impl ConnectionTrait + TransactionTrait), at: &Visit) -> DomainResult<()> {
    let (v, a) = (at.vehicle, Some(at.appointment));

    let brakes = job(
        db,
        v,
        NewWorkItem {
            title: "Brakes".into(),
            notes: Some("Check the pad wear sensors while the wheels are off.".into()),
            est_cost_cents: Some(65_000),
            appointment_id: a,
            ..Default::default()
        },
    )
    .await?;
    rear_rotors(db, at, brakes).await?;

    let rear_pads = need::add(
        db,
        v,
        brakes,
        NewNeed {
            name: "Rear pads".into(),
            ..Default::default()
        },
    )
    .await?
    .id;
    need::add_option(
        db,
        v,
        rear_pads,
        NewOption {
            fitment: Some(Fitment::Unconfirmed),
            ..option("Ceramic rear pads", "Akebono", "ACT1760", 5_400)
        },
    )
    .await?;

    let front_pads = self_need(db, v, brakes, "Front pads").await?;
    choose(
        db,
        v,
        front_pads,
        option("Ceramic front pads", "Akebono", "ACT1633", 7_499),
        "Quiet and low dust.",
        at.days_ago(3),
    )
    .await?;

    let front_rotors = self_need(db, v, brakes, "Front rotors").await?;
    // The kit's number is the seller's, so the order lists what is in the box
    // with the numbers printed on each item, and the work sheet prints them.
    let rotor_kit = order_and_receive(
        db,
        v,
        front_rotors,
        option(
            "Front rotor kit, 312 mm, with hardware",
            "FCP Euro",
            "KIT-10458",
            16_200,
        ),
        "FCP-10458",
        rotor_kit_contents(),
        Bought {
            decided: at.days_ago(6),
            ordered: at.days_ago(5),
            received: at.days_ago(2),
        },
    )
    .await?;
    part::update(
        db,
        v,
        rotor_kit,
        UpdatePart {
            notes: Some(Some("Torque the retaining screws to 4 Nm.".into())),
            ..Default::default()
        },
    )
    .await?;

    need::add(
        db,
        v,
        brakes,
        NewNeed {
            name: "Brake fluid".into(),
            supplied_by: Some(Supplier::Shop),
            ..Default::default()
        },
    )
    .await?;

    Ok(())
}

// What is in the box of the front rotor kit.
fn rotor_kit_contents() -> Vec<NewPartContent> {
    vec![
        NewPartContent {
            name: "Front rotor".into(),
            manufacturer: Some("Zimmermann".into()),
            part_number: Some("100.3367.20".into()),
            quantity: Some(2),
        },
        NewPartContent {
            name: "Rotor retaining screw".into(),
            manufacturer: Some("Volkswagen".into()),
            part_number: Some("N 911 568 01".into()),
            quantity: Some(2),
        },
    ]
}

async fn oil_change(
    db: &(impl ConnectionTrait + TransactionTrait),
    at: &Visit,
) -> DomainResult<()> {
    let (v, a) = (at.vehicle, Some(at.appointment));
    // The job names the schedule item, so the service that completes it is
    // the item's last service and the item is not due.
    let oil = job(
        db,
        v,
        NewWorkItem {
            title: "Oil change".into(),
            schedule_item_id: Some(schedule_item(db, v, "Oil & Filter Change").await?),
            appointment_id: a,
            ..Default::default()
        },
    )
    .await?;
    let filter = self_need(db, v, oil, "Oil filter").await?;
    order_and_receive(
        db,
        v,
        filter,
        option("Oil filter cartridge", "Mann", "HU 7008 z", 1_299),
        "FCP-10459",
        Vec::new(),
        // The filter arrived before the service that fitted it.
        Bought {
            decided: at.days_ago(22),
            ordered: at.days_ago(20),
            received: at.days_ago(15),
        },
    )
    .await?;
    let filter_part = need::require(db, v, filter).await?.part_id;
    service_record::create(
        db,
        v,
        NewServiceRecord {
            service_date: at.days_ago(12),
            performed_by: Some(Performer::Owner),
            mileage: Some(59_900),
            description: Some("Oil and filter change".into()),
            part_ids: filter_part.map(|id| vec![id]),
            work_item_ids: Some(vec![oil]),
            ..Default::default()
        },
    )
    .await?;

    Ok(())
}

async fn small_jobs(
    db: &(impl ConnectionTrait + TransactionTrait),
    at: &Visit,
) -> DomainResult<()> {
    let (v, a) = (at.vehicle, Some(at.appointment));
    // The cabin filter is due and its job is on the appointment, so the home
    // page counts the item as booked. The job has no estimate.
    let small = job(
        db,
        v,
        NewWorkItem {
            title: "Cabin filter and wipers".into(),
            schedule_item_id: Some(schedule_item(db, v, "Cabin Air Filter").await?),
            appointment_id: a,
            ..Default::default()
        },
    )
    .await?;
    let cabin = self_need(db, v, small, "Cabin air filter").await?;
    choose(
        db,
        v,
        cabin,
        option("Pollen cabin air filter", "Mann", "CUK 26 009", 2_150),
        "Matches the original part.",
        at.days_ago(4),
    )
    .await?;
    order(db, v, cabin, "FCP-10460", Vec::new(), at.days_ago(3)).await?;
    let wipers = self_need(db, v, small, "Wiper blades").await?;
    choose(
        db,
        v,
        wipers,
        NewOption {
            seller: Some("Amazon".into()),
            ..option(
                "Wiper blade set, 24 in. and 19 in.",
                "Bosch",
                "3397118938",
                3_299,
            )
        },
        "Best reviews for this car.",
        at.days_ago(8),
    )
    .await?;
    order(db, v, wipers, "AMZ-77120", Vec::new(), at.days_ago(7)).await?;
    need::return_part_on(
        db,
        v,
        wipers,
        ReturnInput {
            reason: "The driver-side blade is 2 in. too long.".into(),
            option_was_wrong: true,
        },
        at.days_ago(1),
    )
    .await?;

    Ok(())
}

async fn todos(db: &(impl ConnectionTrait + TransactionTrait), at: &Visit) -> DomainResult<()> {
    let (v, a) = (at.vehicle, Some(at.appointment));
    for (title, appointment_id, done) in [
        ("Ask the shop to save the old rotors", a, false),
        ("Bring the locking wheel nut key", a, true),
        ("Renew the registration", None, false),
    ] {
        let created = todo::add(
            db,
            v,
            NewTodo {
                title: title.into(),
                appointment_id,
                ..Default::default()
            },
        )
        .await?;
        if done {
            todo::complete(db, v, created.id, Some(at.days_ago(1))).await?;
        }
    }

    Ok(())
}

async fn unscheduled(db: &(impl ConnectionTrait + TransactionTrait), v: i32) -> DomainResult<()> {
    job(
        db,
        v,
        NewWorkItem {
            title: "Air filter".into(),
            schedule_item_id: Some(schedule_item(db, v, "Air Filter").await?),
            est_cost_cents: Some(6_000),
            ..Default::default()
        },
    )
    .await?;
    let washer = job(
        db,
        v,
        NewWorkItem {
            title: "Washer pump".into(),
            est_cost_cents: Some(9_000),
            ..Default::default()
        },
    )
    .await?;
    let pump = need::add(
        db,
        v,
        washer,
        NewNeed {
            name: "Washer pump".into(),
            ..Default::default()
        },
    )
    .await?;
    need::add_option(
        db,
        v,
        pump.id,
        option("Windshield washer pump", "VAICO", "V10-0207", 2_800),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        error::DomainError,
        services::{
            appointment, checklist, need::Stage, reminders::DueStatus, service_detail,
            vehicle_detail, waiting,
        },
        test_support::{VehicleFixture, test_db},
    };

    // The day the tests seed for. It is fixed, so the real clock decides
    // nothing the tests assert.
    fn day() -> chrono::NaiveDate {
        crate::test_support::date("2026-11-13")
    }

    async fn seeded_vehicle(db: &DatabaseConnection) -> i32 {
        demo(db, day()).await.unwrap();
        entities::vehicle::Entity::find()
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .id
    }

    // The home page's maintenance list is a mix, and no item reads overdue
    // only because the car has no purchase date.
    #[tokio::test]
    async fn demo_seed_shows_a_mix_of_due_maintenance() {
        let db = test_db().await;
        let v = seeded_vehicle(&db).await;

        let waiting = waiting::waiting(&db, v, day()).await.unwrap();

        let listed: Vec<(&str, DueStatus, bool)> = waiting
            .maintenance
            .iter()
            .map(|m| (m.name.as_str(), m.status, m.job_id.is_some()))
            .collect();
        assert_eq!(
            listed,
            [
                ("Tire Rotation", DueStatus::Overdue, false),
                ("Wiper Blades", DueStatus::Overdue, false),
                ("Air Filter", DueStatus::DueSoon, true),
                ("Brake Fluid Flush", DueStatus::DueSoon, false),
            ]
        );
        // The cabin filter's job is on the appointment.
        assert_eq!(waiting.maintenance_booked, 1);
        assert!(waiting.mileage_is_estimate);
        assert!(
            (60_000..60_200).contains(&waiting.estimated_mileage),
            "{}",
            waiting.estimated_mileage
        );
    }

    #[tokio::test]
    async fn demo_seed_fills_the_recall_incident_and_job_lists() {
        let db = test_db().await;
        let v = seeded_vehicle(&db).await;

        let waiting = waiting::waiting(&db, v, day()).await.unwrap();

        assert_eq!(waiting.recalls.len(), 1);
        assert_eq!(
            waiting.recalls[0].campaign_number.as_deref(),
            Some("00V000000")
        );
        let incidents: Vec<&str> = waiting.incidents.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(incidents, ["Rock chip in the windshield"]);
        assert_eq!(waiting.incidents_booked, 1);
        let jobs: Vec<&str> = waiting.jobs.iter().map(|j| j.title.as_str()).collect();
        assert_eq!(jobs, ["Air filter", "Washer pump"]);
    }

    #[tokio::test]
    async fn demo_seed_gives_the_vehicle_page_an_estimate_and_a_history() {
        let db = test_db().await;
        let v = seeded_vehicle(&db).await;

        let detail = vehicle_detail::get(&db, v, day()).await.unwrap();

        assert_eq!(detail.odometer, Some(59_900));
        assert!(detail.estimated_mileage.is_some(), "{detail:?}");
        let kinds: std::collections::HashSet<&str> = detail
            .recent_activity
            .iter()
            .map(|a| a.kind.as_str())
            .collect();
        assert_eq!(kinds.len(), 3, "{kinds:?}");
    }

    #[tokio::test]
    async fn demo_seed_records_a_shop_invoice_with_a_covered_line() {
        let db = test_db().await;
        let v = seeded_vehicle(&db).await;
        let shop = entities::shop::Entity::find()
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let invoice = entities::service_record::Entity::find()
            .filter(entities::service_record::Column::ShopId.eq(shop.id))
            .one(&db)
            .await
            .unwrap()
            .expect("a service at the demo shop");

        let read = service_detail::get(&db, v, invoice.id).await.unwrap();

        let value = serde_json::to_value(&read).unwrap();
        let text = value.to_string();
        assert!(text.contains("\"manufacturer\""), "{text}");
        assert!(text.contains("\"inspection\""), "{text}");
        assert!(text.contains("Coolant smell after a long drive"), "{text}");
        assert_eq!(invoice.total_cost_cents, Some(45_500));
    }

    #[tokio::test]
    async fn demo_seed_prints_a_reported_symptom_on_the_work_sheet() {
        let db = test_db().await;
        let v = seeded_vehicle(&db).await;
        let appointment = entities::appointment::Entity::find()
            .one(&db)
            .await
            .unwrap()
            .unwrap();

        let sheet = crate::services::work_sheet::for_appointment(&db, v, appointment.id, day())
            .await
            .unwrap();

        let symptoms: Vec<&str> = sheet
            .jobs
            .iter()
            .filter_map(|j| j.symptom.as_deref())
            .collect();
        assert_eq!(symptoms, ["Clunk over bumps, front left"]);
    }

    // No part is ordered before it was chosen, received before it was
    // ordered or fitted before it arrived, and no service fixes an incident
    // reported after it.
    #[tokio::test]
    async fn demo_seed_dates_every_record_in_a_possible_order() {
        let db = test_db().await;
        demo(&db, day()).await.unwrap();

        let parts = entities::part::Entity::find().all(&db).await.unwrap();
        let installed: Vec<_> = parts
            .iter()
            .filter(|p| p.status == entities::part::PartStatus::Installed)
            .collect();
        assert_eq!(installed.len(), 1, "{parts:?}");
        for p in &parts {
            let bought = p
                .purchase_date
                .expect("every demo part has a purchase date");
            assert!(bought <= day(), "{p:?}");
            if let Some(received) = p.received_on {
                assert!(bought <= received && received <= day(), "{p:?}");
            }
            if let Some(fitted) = p.installed_date {
                let received = p.received_on.expect("a fitted demo part was received");
                assert!(received <= fitted && fitted <= day(), "{p:?}");
            }
            if let Some(returned) = p.returned_on {
                assert!(bought <= returned && returned <= day(), "{p:?}");
            }
        }
        for n in entities::job_need::Entity::find().all(&db).await.unwrap() {
            let Some(decided) = n.decided_on else {
                continue;
            };
            assert!(decided <= day(), "{n:?}");
            if let Some(part) = parts.iter().find(|p| Some(p.id) == n.part_id) {
                assert!(Some(decided) <= part.purchase_date, "{n:?} {part:?}");
            }
        }
        let vehicle = entities::vehicle::Entity::find()
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        for service in entities::service_record::Entity::find()
            .all(&db)
            .await
            .unwrap()
        {
            assert!(vehicle.purchase_date <= Some(service.service_date));
            assert!(service.service_date <= day(), "{service:?}");
        }
        let fixed = entities::incident::Entity::find()
            .filter(entities::incident::Column::Resolved.eq(true))
            .one(&db)
            .await
            .unwrap()
            .expect("a resolved incident");
        let shop_service = entities::service_record::Entity::find()
            .filter(entities::service_record::Column::ShopId.is_not_null())
            .one(&db)
            .await
            .unwrap()
            .expect("the shop's service");
        assert!(fixed.occurred_at.day() <= shop_service.service_date);
    }

    // The same day gives the same dates, whatever the real clock says.
    #[tokio::test]
    async fn demo_seed_dates_everything_from_the_day_it_is_given() {
        let db = test_db().await;
        let long_ago = crate::test_support::date::<chrono::NaiveDate>("2020-03-01");
        demo(&db, long_ago).await.unwrap();

        let latest_part_day = entities::part::Entity::find()
            .all(&db)
            .await
            .unwrap()
            .into_iter()
            .flat_map(|p| {
                [
                    p.purchase_date,
                    p.received_on,
                    p.installed_date,
                    p.returned_on,
                ]
            })
            .flatten()
            .max();
        assert!(latest_part_day <= Some(long_ago), "{latest_part_day:?}");
        let needs = entities::job_need::Entity::find().all(&db).await.unwrap();
        assert!(needs.iter().all(|n| n.decided_on <= Some(long_ago)));
        let todos = entities::todo::Entity::find().all(&db).await.unwrap();
        assert!(todos.iter().all(|t| t.done_on <= Some(long_ago)));
        let appointment = entities::appointment::Entity::find()
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            appointment.planned_date,
            Some(long_ago + chrono::Duration::days(14))
        );
    }

    #[tokio::test]
    async fn demo_seed_counts_the_records_beside_the_checklist() {
        let db = test_db().await;
        let summary = demo(&db, day()).await.unwrap();
        assert_eq!(summary.services, 2);
        assert_eq!(summary.readings, 6);
        assert_eq!(summary.incidents, 3);
        assert_eq!(summary.notes, 1);
        assert_eq!(summary.recalls, 1);
        assert_eq!(summary.todos, 4);
    }

    #[tokio::test]
    async fn demo_seed_covers_every_stage() {
        let db = test_db().await;
        let summary = demo(&db, day()).await.unwrap();
        assert_eq!(summary.vehicles, 1);
        assert_eq!(summary.appointments, 1);
        assert_eq!(summary.jobs, 6);

        let vehicle = crate::entities::vehicle::Entity::find()
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(vehicle.name, "Demo GTI");
        assert!(vehicle.vin.is_none() && vehicle.license_plate.is_none());
        let appointments = appointment::list(&db, vehicle.id, true, day())
            .await
            .unwrap();
        assert_eq!(appointments.len(), 1);
        assert_eq!(
            appointments[0].appointment.purpose,
            "60k service + loose ends"
        );

        let list =
            checklist::for_appointment(&db, vehicle.id, appointments[0].appointment.id, day())
                .await
                .unwrap();
        let mut stages: Vec<Stage> = list
            .jobs
            .iter()
            .flat_map(|j| j.needs.iter().map(|n| n.stage))
            .collect();
        let unscheduled = checklist::for_unscheduled(&db, vehicle.id).await.unwrap();
        stages.extend(
            unscheduled
                .jobs
                .iter()
                .flat_map(|j| j.needs.iter().map(|n| n.stage)),
        );
        for wanted in [
            Stage::Undecided,
            Stage::ToOrder,
            Stage::ShopSupplies,
            Stage::Ordered,
            Stage::Received,
            Stage::Installed,
        ] {
            assert!(
                stages.contains(&wanted),
                "no need is {wanted:?}: {stages:?}"
            );
        }

        let options: Vec<_> = list
            .jobs
            .iter()
            .flat_map(|j| &j.needs)
            .flat_map(|n| n.chosen.iter().chain(&n.other_options))
            .collect();
        assert!(
            options
                .iter()
                .any(|o| o.fitment == Fitment::DoesNotFit && o.fitment_note.is_some())
        );
        assert!(options.iter().any(|o| o.rejected_reason.is_some()));
        assert!(
            list.todos
                .iter()
                .any(|t| { t.title == "Return Wiper blade set, 24 in. and 19 in. to Amazon" })
        );
        let rotor = |part_number: &str| {
            options
                .iter()
                .find(|o| o.part_number.as_deref() == Some(part_number))
                .unwrap_or_else(|| panic!("no option {part_number}"))
        };
        assert_eq!(rotor("5Q0615601E").fitment, Fitment::Confirmed);
        assert_eq!(
            rotor("600.3241.20").fitment_note.as_deref(),
            Some("272 x 10 mm, for cars without the Performance Pack")
        );
        assert!(list.todos.iter().any(|t| t.done_on.is_some()));
        assert!(
            list.todos
                .iter()
                .any(|t| t.done_on.is_none() && !t.title.starts_with("Return "))
        );
        // The air filter job has no needs, so the unscheduled checklist
        // leaves it out.
        assert_eq!(unscheduled.jobs.len(), 1);
        assert_eq!(unscheduled.jobs[0].needs.len(), 1);
        // A need for two shows its count on the page.
        let pair = list
            .jobs
            .iter()
            .flat_map(|j| &j.needs)
            .find(|n| n.need.name == "Rear rotors")
            .unwrap();
        assert_eq!(pair.need.quantity, 2);
    }

    #[tokio::test]
    async fn demo_seed_shows_a_kits_contents_on_the_work_sheet() {
        let db = test_db().await;
        demo(&db, day()).await.unwrap();
        let appointment = crate::entities::appointment::Entity::find()
            .one(&db)
            .await
            .unwrap()
            .unwrap();

        let sheet = crate::services::work_sheet::for_appointment(
            &db,
            appointment.vehicle_id,
            appointment.id,
            day(),
        )
        .await
        .unwrap();

        assert!(
            sheet
                .jobs
                .iter()
                .flat_map(|j| &j.owner_supplies)
                .any(|line| line.contents.len() == 2),
            "{sheet:?}"
        );
        assert!(sheet.jobs.iter().any(|j| j.notes.is_some()), "{sheet:?}");
        assert!(
            sheet
                .jobs
                .iter()
                .flat_map(|j| &j.owner_supplies)
                .any(|line| line.notes.is_some()),
            "{sheet:?}"
        );
    }

    #[tokio::test]
    async fn demo_seed_reuses_the_shop_an_earlier_demo_left() {
        let db = test_db().await;
        demo(&db, day()).await.unwrap();
        entities::vehicle::Entity::delete_many()
            .exec(&db)
            .await
            .unwrap();
        demo(&db, day()).await.unwrap();
        assert_eq!(
            crate::test_support::row_count::<entities::shop::Entity>(&db).await,
            1
        );
    }

    #[tokio::test]
    async fn demo_seed_refuses_a_database_with_a_vehicle() {
        let db = test_db().await;
        VehicleFixture::new().insert(&db).await;
        let err = demo(&db, day()).await.unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        assert_eq!(
            crate::test_support::row_count::<crate::entities::vehicle::Entity>(&db).await,
            1
        );
    }
}
