//! Made-up data for a test environment, so the web page can be developed and
//! shown without the owner's real records.

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities,
    error::{DomainError, DomainResult},
    inputs::{
        appointment::NewAppointment,
        need::{NewNeed, NewOption, OrderDetails, ReturnInput, UpdateOption},
        service_record::NewServiceRecord,
        todo::NewTodo,
        vehicle::NewVehicle,
        work_item::NewWorkItem,
    },
    services::{self, appointment, model_template, need, service_record, todo, vehicle, work_item},
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
}

/// Fills an empty non-live database with made-up data: one vehicle, one
/// appointment with three jobs, one unscheduled job, and needs in all six
/// stages. Everything is written in one transaction through the same
/// services the tools use, so a failure leaves the database empty. The
/// caller is responsible for never pointing this at the live database.
///
/// # Errors
///
/// `BadRequest` when any vehicle exists.
pub async fn demo<C: ConnectionTrait + TransactionTrait>(db: &C) -> DomainResult<SeedSummary> {
    let txn = db.begin().await?;
    if entities::vehicle::Entity::find().count(&txn).await? > 0 {
        return Err(DomainError::BadRequest(
            "this database already has a vehicle; seed an empty environment".into(),
        ));
    }
    build(&txn).await?;
    let summary = SeedSummary {
        vehicles: entities::vehicle::Entity::find().count(&txn).await?,
        appointments: entities::appointment::Entity::find().count(&txn).await?,
        jobs: entities::work_item::Entity::find().count(&txn).await?,
        needs: entities::job_need::Entity::find().count(&txn).await?,
        options: entities::need_option::Entity::find().count(&txn).await?,
        parts: entities::part::Entity::find().count(&txn).await?,
        todos: entities::todo::Entity::find().count(&txn).await?,
    };
    txn.commit().await?;
    Ok(summary)
}

fn today() -> String {
    services::today().format("%Y-%m-%d").to_string()
}

// One researched candidate with the fields the sentences are built from.
fn option(description: &str, manufacturer: &str, part_number: &str, price_cents: i32) -> NewOption {
    NewOption {
        description: description.into(),
        manufacturer: Some(manufacturer.into()),
        part_number: Some(part_number.into()),
        seller: Some("FCP Euro".into()),
        price_cents: Some(price_cents),
        fitment: Some("confirmed".into()),
        ..Default::default()
    }
}

async fn job(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    appointment_id: Option<i32>,
    title: &str,
) -> DomainResult<i32> {
    let created = work_item::create(
        db,
        vehicle_id,
        NewWorkItem {
            title: title.into(),
            appointment_id,
            ..Default::default()
        },
    )
    .await?;
    Ok(created.id)
}

async fn self_need(
    db: &impl ConnectionTrait,
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
            supplied_by: Some("self".into()),
            approach: Some("replacement".into()),
            ..Default::default()
        },
    )
    .await?;
    Ok(created.id)
}

// Adds `candidate` to the need and chooses it.
async fn choose(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    need_id: i32,
    candidate: NewOption,
    reason: &str,
) -> DomainResult<i32> {
    let added = need::add_option(db, vehicle_id, need_id, candidate).await?;
    need::decide(db, vehicle_id, need_id, Some(added.id), Some(reason.into())).await?;
    Ok(added.id)
}

// Chooses `candidate`, orders it from FCP Euro and records its arrival.
async fn order_and_receive(
    db: &(impl ConnectionTrait + TransactionTrait),
    vehicle_id: i32,
    need_id: i32,
    candidate: NewOption,
    order_ref: &str,
) -> DomainResult<()> {
    choose(
        db,
        vehicle_id,
        need_id,
        candidate,
        "Matches the original part.",
    )
    .await?;
    order(db, vehicle_id, need_id, order_ref).await?;
    need::mark_received(db, vehicle_id, need_id, None).await?;
    Ok(())
}

async fn order(
    db: &(impl ConnectionTrait + TransactionTrait),
    vehicle_id: i32,
    need_id: i32,
    order_ref: &str,
) -> DomainResult<()> {
    let details = OrderDetails {
        order_ref: Some(order_ref.into()),
        ..Default::default()
    };
    need::mark_ordered(db, vehicle_id, need_id, details).await?;
    Ok(())
}

struct Visit {
    vehicle: i32,
    appointment: i32,
}

async fn build(db: &(impl ConnectionTrait + TransactionTrait)) -> DomainResult<()> {
    let template = model_template::list(db)
        .await?
        .into_iter()
        .find(|t| t.model.as_deref() == Some("Golf GTI"))
        .ok_or_else(|| DomainError::Internal("the Golf GTI template is not seeded".into()))?;
    let car = vehicle::create(
        db,
        NewVehicle {
            name: "Demo GTI".into(),
            model_template_id: Some(template.id),
            color: Some("Demo gray".into()),
            purchase_mileage: Some(52_000),
            notes: Some("Made-up vehicle for trying out the checklist.".into()),
            ..Default::default()
        },
    )
    .await?;
    let v = car.id;
    let visit = appointment::create(
        db,
        v,
        NewAppointment {
            purpose: "60k service + loose ends".into(),
            shop_name: Some("Demo Auto Works".into()),
            planned_date: Some(
                (services::today() + chrono::Duration::days(14))
                    .format("%Y-%m-%d")
                    .to_string(),
            ),
            ..Default::default()
        },
    )
    .await?;
    let at = Visit {
        vehicle: v,
        appointment: visit.id,
    };
    brakes(db, &at).await?;
    oil_change(db, &at).await?;
    small_jobs(db, &at).await?;
    todos(db, &at).await?;
    unscheduled(db, v).await
}

// Three candidates: one chosen, one that does not fit and one rejected.
async fn rear_rotors(
    db: &(impl ConnectionTrait + TransactionTrait),
    v: i32,
    brakes: i32,
) -> DomainResult<()> {
    let rear_rotors = self_need(db, v, brakes, "Rear rotors").await?;
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
            fitment: Some("does_not_fit".into()),
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
    need::decide(
        db,
        v,
        rear_rotors,
        Some(chosen.id),
        Some("Coated hats resist rust.".into()),
    )
    .await?;
    Ok(())
}

async fn brakes(db: &(impl ConnectionTrait + TransactionTrait), at: &Visit) -> DomainResult<()> {
    let (v, a) = (at.vehicle, Some(at.appointment));

    let brakes = job(db, v, a, "Brakes").await?;
    rear_rotors(db, v, brakes).await?;

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
            fitment: Some("unconfirmed".into()),
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
    )
    .await?;

    let front_rotors = self_need(db, v, brakes, "Front rotors").await?;
    order_and_receive(
        db,
        v,
        front_rotors,
        option(
            "Coated front rotor, 312 mm",
            "Zimmermann",
            "100.3367.20",
            16_200,
        ),
        "FCP-10458",
    )
    .await?;

    need::add(
        db,
        v,
        brakes,
        NewNeed {
            name: "Brake fluid".into(),
            supplied_by: Some("shop".into()),
            ..Default::default()
        },
    )
    .await?;

    Ok(())
}

async fn oil_change(
    db: &(impl ConnectionTrait + TransactionTrait),
    at: &Visit,
) -> DomainResult<()> {
    let (v, a) = (at.vehicle, Some(at.appointment));
    let oil = job(db, v, a, "Oil change").await?;
    let filter = self_need(db, v, oil, "Oil filter").await?;
    order_and_receive(
        db,
        v,
        filter,
        option("Oil filter cartridge", "Mann", "HU 7008 z", 1_299),
        "FCP-10459",
    )
    .await?;
    let filter_part = need::require(db, v, filter).await?.part_id;
    service_record::create(
        db,
        v,
        NewServiceRecord {
            service_date: today(),
            performed_by: Some("self".into()),
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
    let small = job(db, v, a, "Cabin filter and wipers").await?;
    let cabin = self_need(db, v, small, "Cabin air filter").await?;
    choose(
        db,
        v,
        cabin,
        option("Pollen cabin air filter", "Mann", "CUK 26 009", 2_150),
        "Matches the original part.",
    )
    .await?;
    order(db, v, cabin, "FCP-10460").await?;
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
    )
    .await?;
    order(db, v, wipers, "AMZ-77120").await?;
    need::return_part(
        db,
        v,
        wipers,
        ReturnInput {
            reason: "The driver-side blade is 2 in. too long.".into(),
            option_was_wrong: true,
        },
    )
    .await?;

    Ok(())
}

async fn todos(db: &impl ConnectionTrait, at: &Visit) -> DomainResult<()> {
    let (v, a) = (at.vehicle, Some(at.appointment));
    for (title, done) in [
        ("Ask the shop about the clunk over bumps", false),
        ("Bring the locking wheel nut key", true),
    ] {
        let created = todo::add(
            db,
            v,
            NewTodo {
                title: title.into(),
                appointment_id: a,
                ..Default::default()
            },
        )
        .await?;
        if done {
            todo::complete(db, v, created.id, None).await?;
        }
    }

    Ok(())
}

async fn unscheduled(db: &impl ConnectionTrait, v: i32) -> DomainResult<()> {
    let washer = job(db, v, None, "Washer pump").await?;
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
        services::{appointment, checklist, need::Stage},
        test_support::{VehicleFixture, test_db},
    };

    #[tokio::test]
    async fn demo_seed_covers_every_stage() {
        let db = test_db().await;
        let summary = demo(&db).await.unwrap();
        assert_eq!(summary.vehicles, 1);
        assert_eq!(summary.appointments, 1);
        assert_eq!(summary.jobs, 4);

        let vehicle = crate::entities::vehicle::Entity::find()
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(vehicle.name, "Demo GTI");
        assert!(vehicle.vin.is_none() && vehicle.license_plate.is_none());
        let appointments = appointment::list(&db, vehicle.id, true).await.unwrap();
        assert_eq!(appointments.len(), 1);
        assert_eq!(
            appointments[0].appointment.purpose,
            "60k service + loose ends"
        );

        let list = checklist::for_appointment(&db, vehicle.id, appointments[0].appointment.id)
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
                .any(|o| o.fitment == "does_not_fit" && o.fitment_note.is_some())
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
        assert_eq!(rotor("5Q0615601E").fitment, "confirmed");
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
        assert_eq!(unscheduled.jobs.len(), 1);
        assert_eq!(unscheduled.jobs[0].needs.len(), 1);
    }

    #[tokio::test]
    async fn demo_seed_refuses_a_database_with_a_vehicle() {
        let db = test_db().await;
        VehicleFixture::new().insert(&db).await;
        let err = demo(&db).await.unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        assert_eq!(
            crate::test_support::row_count::<crate::entities::vehicle::Entity>(&db).await,
            1
        );
    }
}
