use sea_orm::*;

use crate::{
    entities::{
        appointment, incident, model_template, part, research_finding, research_report,
        service_record, todo, vehicle, work_item,
    },
    error::{DomainError, DomainResult},
    inputs::vehicle::{NewVehicle, UpdateVehicle, VehicleSale},
    services::vin_decode::{self, VinDecodeResult},
};

/// Verify a vehicle exists, returning the model or a `NotFound` error.
///
/// This is the shared ownership check for all vehicle sub-resources.
pub async fn require(db: &impl ConnectionTrait, vehicle_id: i32) -> DomainResult<vehicle::Model> {
    vehicle::Entity::find_by_id(vehicle_id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Vehicle {vehicle_id} not found")))
}

/// The car's year, make, model and trim in that order, such as
/// `2017 Volkswagen Golf GTI`, with each one the vehicle lacks left out. It is
/// none when the vehicle records none of them.
#[must_use]
pub fn description(v: &vehicle::Model) -> Option<String> {
    let year = v.year.map(|y| y.to_string());
    let words: Vec<String> = [&year, &v.make, &v.model, &v.trim_level]
        .into_iter()
        .filter_map(|word| word.as_deref().and_then(super::trim_to_none))
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// Verifies a vehicle exists and is in the garage, for a write that adds
/// open work (a planned job, an appointment or a to-do).
///
/// # Errors
/// `NotFound` for a missing vehicle; `BadRequest` for an archived one, since
/// no garage list would show the work.
pub(crate) async fn require_in_garage(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<vehicle::Model> {
    let v = require(db, vehicle_id).await?;
    if v.archived_at.is_some() {
        return Err(DomainError::BadRequest(format!(
            "Vehicle {vehicle_id} is archived, so it takes no new planned work, appointments or \
             to-dos. Unarchive it first if it is back in the garage."
        )));
    }
    Ok(v)
}

/// The vehicles in id order, leaving out the archived ones unless
/// `include_archived` is set.
pub async fn list(
    db: &impl ConnectionTrait,
    include_archived: bool,
) -> DomainResult<Vec<vehicle::Model>> {
    let mut query = vehicle::Entity::find();
    if !include_archived {
        query = query.filter(vehicle::Column::ArchivedAt.is_null());
    }
    Ok(query.order_by_asc(vehicle::Column::Id).all(db).await?)
}

pub async fn get(db: &impl ConnectionTrait, id: i32) -> DomainResult<vehicle::Model> {
    require(db, id).await
}

/// Loads the template when an id is given. Fails with a `NotFound` that
/// points at `list_model_templates` when the id names no template.
async fn require_template(
    db: &impl ConnectionTrait,
    id: Option<i32>,
) -> DomainResult<Option<model_template::Model>> {
    let Some(id) = id else { return Ok(None) };
    model_template::Entity::find_by_id(id)
        .one(db)
        .await?
        .map(Some)
        .ok_or_else(|| {
            DomainError::NotFound(format!(
                "Model template {id} not found; list_model_templates shows the valid ids"
            ))
        })
}

/// Fills each detail the input leaves empty from its model template, when it
/// names one.
async fn fill_new_from_template(
    db: &impl ConnectionTrait,
    input: &mut NewVehicle,
) -> DomainResult<()> {
    if let Some(t) = require_template(db, input.model_template_id).await? {
        input.year = input.year.or(t.year);
        input.make = input.make.take().or(t.make);
        input.model = input.model.take().or(t.model);
        input.trim_level = input.trim_level.take().or(t.trim_level);
        input.body_style = input.body_style.take().or(t.body_style);
        input.engine = input.engine.take().or(t.engine);
        input.transmission = input.transmission.take().or(t.transmission);
        input.drivetrain = input.drivetrain.take().or(t.drivetrain);
    }
    Ok(())
}

/// Creates a vehicle. When `model_template_id` is set, every one of year, make,
/// model, trim, body style, engine, transmission and drivetrain that the input
/// leaves empty takes the template's value; a supplied value always wins.
pub async fn create<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    mut input: NewVehicle,
) -> DomainResult<vehicle::Model> {
    validate_purchase_mileage(input.purchase_mileage)?;
    let txn = super::begin_write(db).await?;
    let db = &txn;
    fill_new_from_template(db, &mut input).await?;
    let model = vehicle::ActiveModel {
        name: Set(input.name),
        model_template_id: Set(input.model_template_id),
        year: Set(input.year),
        make: Set(input.make),
        model: Set(input.model),
        trim_level: Set(input.trim_level),
        body_style: Set(input.body_style),
        engine: Set(input.engine),
        transmission: Set(input.transmission),
        drivetrain: Set(input.drivetrain),
        vin: Set(input.vin),
        license_plate: Set(input.license_plate),
        color: Set(input.color),
        purchase_date: Set(input.purchase_date),
        purchase_price_cents: Set(input.purchase_price_cents),
        purchase_mileage: Set(input.purchase_mileage),
        notes: Set(input.notes),
        warranty_expires_on: Set(input.warranty_expires_on),
        warranty_expires_miles: Set(input.warranty_expires_miles),
        ..Default::default()
    };
    let out = model.insert(db).await?;
    txn.commit().await?;
    Ok(out)
}

/// A created vehicle and what its VIN lookup did.
#[derive(Debug, serde::Serialize)]
pub struct CreatedVehicle {
    #[serde(flatten)]
    pub vehicle: vehicle::Model,
    /// Why the lookup failed. It is `None` when the input had no VIN or the
    /// lookup succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vin_decode_error: Option<String>,
}

/// Creates a vehicle, decoding its VIN on NHTSA (a live web request) when the
/// input carries one.
///
/// Year, make, model, trim, body style, engine, transmission and drivetrain
/// each take the first value available from the caller, then the model
/// template, then the VIN decode, and the decoded attributes are stored. A
/// failed lookup is not an error: the vehicle is created without the decode
/// and the result carries `vin_decode_error`.
///
/// # Errors
/// `Invalid` for a negative purchase mileage and `NotFound` for an unknown
/// model template, both before any request is made.
pub async fn create_with_vin<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    input: NewVehicle,
) -> DomainResult<CreatedVehicle> {
    create_with_vin_lookup(db, input, |vin| async move {
        vin_decode::decode_vin(&vin).await
    })
    .await
}

/// [`create_with_vin`] with the VIN lookup supplied by the caller, so a test
/// can stand in for the network.
pub(crate) async fn create_with_vin_lookup<C, F, Fut>(
    db: &C,
    input: NewVehicle,
    lookup: F,
) -> DomainResult<CreatedVehicle>
where
    C: ConnectionTrait + TransactionTrait,
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<VinDecodeResult, String>>,
{
    // Each refusal `create` can give comes before the request, so a bad input
    // costs no lookup. The request runs outside any transaction, and `create`
    // checks again inside its own.
    validate_purchase_mileage(input.purchase_mileage)?;
    require_template(db, input.model_template_id).await?;
    let mut input = input;
    input.vin = input
        .vin
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let (decoded, vin_decode_error) = match input.vin.clone() {
        Some(vin) => match lookup(vin).await {
            Ok(d) => (Some(d), None),
            Err(e) => (None, Some(e)),
        },
        None => (None, None),
    };
    let vehicle = create_with_vin_decode(db, input, decoded.as_ref()).await?;
    Ok(CreatedVehicle {
        vehicle,
        vin_decode_error,
    })
}

/// Creates a vehicle from an input and an optional finished VIN decode,
/// filling the fields the caller left empty and storing the decoded
/// attributes. With `decoded` of `None` this behaves like [`create`].
async fn create_with_vin_decode<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    mut input: NewVehicle,
    decoded: Option<&VinDecodeResult>,
) -> DomainResult<vehicle::Model> {
    let txn = super::begin_write(db).await?;
    // The template goes first: a chosen template names one trim and engine,
    // where the decode often returns every option the model line offers.
    fill_new_from_template(&txn, &mut input).await?;
    if let Some(d) = decoded {
        input.year = input.year.or(d.year);
        input.make = input.make.or_else(|| d.make.clone());
        input.model = input.model.or_else(|| d.model.clone());
        input.trim_level = input.trim_level.or_else(|| d.trim.clone());
        input.body_style = input.body_style.or_else(|| d.body_style.clone());
        input.engine = input.engine.or_else(|| d.engine.clone());
        input.transmission = input.transmission.or_else(|| d.transmission.clone());
        input.drivetrain = input.drivetrain.or_else(|| d.drivetrain.clone());
    }
    let created = create(&txn, input).await?;
    if let Some(d) = decoded {
        vin_decode::store_attributes(&txn, created.id, d).await?;
    }
    txn.commit().await?;
    Ok(created)
}

/// Sets each detail that is empty on the car and not set by this update to the
/// template's value.
fn fill_empty_from_template(
    input: &mut UpdateVehicle,
    existing: &vehicle::Model,
    t: model_template::Model,
) {
    fn fill<T>(slot: &mut Option<Option<T>>, current: Option<&T>, from: Option<T>) {
        if slot.is_none() && current.is_none() {
            *slot = Some(from);
        }
    }
    fill(&mut input.year, existing.year.as_ref(), t.year);
    fill(&mut input.make, existing.make.as_ref(), t.make);
    fill(&mut input.model, existing.model.as_ref(), t.model);
    fill(
        &mut input.trim_level,
        existing.trim_level.as_ref(),
        t.trim_level,
    );
    fill(
        &mut input.body_style,
        existing.body_style.as_ref(),
        t.body_style,
    );
    fill(&mut input.engine, existing.engine.as_ref(), t.engine);
    fill(
        &mut input.transmission,
        existing.transmission.as_ref(),
        t.transmission,
    );
    fill(
        &mut input.drivetrain,
        existing.drivetrain.as_ref(),
        t.drivetrain,
    );
}

pub async fn update<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    id: i32,
    input: UpdateVehicle,
) -> DomainResult<vehicle::Model> {
    validate_sale(
        input.sold_price_cents.flatten(),
        input.sold_mileage.flatten(),
    )?;
    validate_purchase_mileage(input.purchase_mileage.flatten())?;
    let txn = super::begin_write(db).await?;
    let db = &txn;
    let existing = require(db, id).await?;
    let template_before = existing.model_template_id;
    let template = require_template(db, input.model_template_id.flatten()).await?;
    let mut input = input;
    if let Some(t) = template {
        fill_empty_from_template(&mut input, &existing, t);
    }
    let mut active: vehicle::ActiveModel = existing.into();

    if let Some(v) = input.name {
        active.name = Set(v);
    }
    if let Some(v) = input.model_template_id {
        active.model_template_id = Set(v);
    }
    if let Some(v) = input.year {
        active.year = Set(v);
    }
    if let Some(v) = input.make {
        active.make = Set(v);
    }
    if let Some(v) = input.model {
        active.model = Set(v);
    }
    if let Some(v) = input.trim_level {
        active.trim_level = Set(v);
    }
    if let Some(v) = input.body_style {
        active.body_style = Set(v);
    }
    if let Some(v) = input.engine {
        active.engine = Set(v);
    }
    if let Some(v) = input.transmission {
        active.transmission = Set(v);
    }
    if let Some(v) = input.drivetrain {
        active.drivetrain = Set(v);
    }
    if let Some(v) = input.vin {
        active.vin = Set(v);
    }
    if let Some(v) = input.license_plate {
        active.license_plate = Set(v);
    }
    if let Some(v) = input.color {
        active.color = Set(v);
    }
    if let Some(v) = input.purchase_date {
        active.purchase_date = Set(v);
    }
    if let Some(v) = input.purchase_price_cents {
        active.purchase_price_cents = Set(v);
    }
    if let Some(v) = input.purchase_mileage {
        active.purchase_mileage = Set(v);
    }
    if let Some(v) = input.sold_date {
        active.sold_date = Set(v);
    }
    if let Some(v) = input.sold_price_cents {
        active.sold_price_cents = Set(v);
    }
    if let Some(v) = input.sold_mileage {
        active.sold_mileage = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }
    if let Some(v) = input.warranty_expires_on {
        active.warranty_expires_on = Set(v);
    }
    if let Some(v) = input.warranty_expires_miles {
        active.warranty_expires_miles = Set(v);
    }
    active.updated_at = Set(super::now());

    let out = active.update(db).await?;
    if out.model_template_id != template_before {
        // Another template changes which schedule item each name stands for.
        super::schedule::drop_links_the_vehicles_jobs_bring(db, out.id).await?;
    }
    txn.commit().await?;
    Ok(out)
}

fn validate_purchase_mileage(mileage: Option<i32>) -> DomainResult<()> {
    if mileage.is_some_and(|miles| miles < 0) {
        return Err(DomainError::invalid(
            "purchase_mileage",
            "must not be negative",
        ));
    }
    Ok(())
}

// A sale price or odometer reading below zero is a typing mistake.
fn validate_sale(price_cents: Option<i32>, mileage: Option<i32>) -> DomainResult<()> {
    for (field, value) in [("sold_price_cents", price_cents), ("sold_mileage", mileage)] {
        if value.is_some_and(|v| v < 0) {
            return Err(DomainError::invalid(field, "must not be negative"));
        }
    }
    Ok(())
}

// A garage list leaves an archived car out, so work still open on one would
// show nowhere; it is settled before the car leaves.
async fn refuse_open_work(db: &impl ConnectionTrait, id: i32) -> DomainResult<()> {
    let appointments = super::appointment::open_by_vehicle(db, &[id], super::today())
        .await?
        .remove(&id)
        .map_or(0, |a| a.len());
    let jobs = work_item::Entity::find()
        .filter(work_item::Column::VehicleId.eq(id))
        .filter(work_item::Column::Status.eq(work_item::JobStatus::Planned))
        .count(db)
        .await?;
    let todos = todo::Entity::find()
        .filter(todo::Column::VehicleId.eq(id))
        .filter(todo::Column::DoneOn.is_null())
        .count(db)
        .await?;
    let open: Vec<String> = [
        (
            u64::try_from(appointments).unwrap_or(u64::MAX),
            "open appointment",
            "open appointments",
        ),
        (jobs, "planned job", "planned jobs"),
        (todos, "open to-do", "open to-dos"),
    ]
    .into_iter()
    .filter(|(n, _, _)| *n > 0)
    .map(|(n, one, many)| format!("{n} {}", if n == 1 { one } else { many }))
    .collect();
    if open.is_empty() {
        return Ok(());
    }
    Err(DomainError::BadRequest(format!(
        "Vehicle {id} still has {}. Cancel or close each appointment, finish or drop each job, \
         and tick or delete each to-do before archiving it.",
        open.join(", ")
    )))
}

/// Takes a vehicle out of the garage, as for a sold car, and records `sale`.
/// An archived vehicle keeps all its records and can still be read by id;
/// garage lists leave it out. Archiving one already archived keeps its first
/// stamp and records the sale.
///
/// # Errors
/// `NotFound` for a missing vehicle; `Invalid` for a negative sale price or
/// mileage; `BadRequest` while the vehicle has an open appointment, a planned
/// job or an open to-do, which no garage list would show once it is archived.
pub async fn archive<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    id: i32,
    sale: VehicleSale,
) -> DomainResult<vehicle::Model> {
    validate_sale(sale.sold_price_cents, sale.sold_mileage)?;
    let txn = super::begin_write(db).await?;
    let db = &txn;
    let existing = require(db, id).await?;
    refuse_open_work(db, id).await?;
    let now = super::now();
    let archived_at = existing.archived_at.unwrap_or(now);
    let mut active: vehicle::ActiveModel = existing.into();
    active.archived_at = Set(Some(archived_at));
    if let Some(v) = sale.sold_date {
        active.sold_date = Set(Some(v));
    }
    if let Some(v) = sale.sold_price_cents {
        active.sold_price_cents = Set(Some(v));
    }
    if let Some(v) = sale.sold_mileage {
        active.sold_mileage = Set(Some(v));
    }
    active.updated_at = Set(now);
    let out = active.update(db).await?;
    txn.commit().await?;
    Ok(out)
}

/// Puts an archived vehicle back in the garage. Its sale details stay.
///
/// # Errors
/// `NotFound` for a missing vehicle.
pub async fn unarchive<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    id: i32,
) -> DomainResult<vehicle::Model> {
    let txn = super::begin_write(db).await?;
    let db = &txn;
    let existing = require(db, id).await?;
    let mut active: vehicle::ActiveModel = existing.into();
    active.archived_at = Set(None);
    active.updated_at = Set(super::now());
    let out = active.update(db).await?;
    txn.commit().await?;
    Ok(out)
}

/// Deletes a vehicle that has no history yet, undoing a mistaken
/// `create`. A vehicle with any service record, wear reading, part, incident,
/// work item, appointment, to-do or research finding is a `BadRequest` naming what it
/// has. Its odometer readings, own schedule items and decoded attributes go
/// with it.
pub async fn delete_unused<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    id: i32,
) -> DomainResult<()> {
    // The counts and the delete share the write lock, so history the other
    // program records meanwhile cannot slip past the check.
    let txn = super::begin_write(db).await?;
    let db = &txn;
    require(db, id).await?;
    let report_ids = research_report::Entity::find()
        .select_only()
        .column(research_report::Column::Id)
        .filter(research_report::Column::VehicleId.eq(id))
        .into_query();
    let counts = [
        (
            "service records",
            service_record::Entity::find()
                .filter(service_record::Column::VehicleId.eq(id))
                .count(db)
                .await?,
        ),
        (
            "wear readings",
            crate::entities::wear_reading::Entity::find()
                .filter(crate::entities::wear_reading::Column::VehicleId.eq(id))
                .count(db)
                .await?,
        ),
        (
            "parts",
            part::Entity::find()
                .filter(part::Column::VehicleId.eq(id))
                .count(db)
                .await?,
        ),
        (
            "incidents",
            incident::Entity::find()
                .filter(incident::Column::VehicleId.eq(id))
                .count(db)
                .await?,
        ),
        (
            "planned work",
            work_item::Entity::find()
                .filter(work_item::Column::VehicleId.eq(id))
                .count(db)
                .await?,
        ),
        (
            "appointments",
            appointment::Entity::find()
                .filter(appointment::Column::VehicleId.eq(id))
                .count(db)
                .await?,
        ),
        (
            "to-dos",
            todo::Entity::find()
                .filter(todo::Column::VehicleId.eq(id))
                .count(db)
                .await?,
        ),
        (
            "research findings",
            research_finding::Entity::find()
                .filter(research_finding::Column::ReportId.in_subquery(report_ids))
                .count(db)
                .await?,
        ),
    ];
    let held: Vec<&str> = counts
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(what, _)| *what)
        .collect();
    if !held.is_empty() {
        return Err(DomainError::BadRequest(format!(
            "Vehicle {id} has {} recorded, so it cannot be deleted. Only a vehicle with no \
             history can be deleted.",
            held.join(", ")
        )));
    }
    vehicle::Entity::delete_by_id(id).exec(db).await?;
    txn.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{entities::part::PartStatus, test_support::test_db};

    async fn seed(db: &DatabaseConnection, name: &str) -> vehicle::Model {
        create(
            db,
            NewVehicle {
                name: name.into(),
                model_template_id: None,
                year: None,
                make: None,
                model: None,
                trim_level: None,
                body_style: None,
                engine: None,
                transmission: None,
                drivetrain: None,
                vin: None,
                license_plate: None,
                color: None,
                purchase_date: None,
                purchase_price_cents: None,
                purchase_mileage: None,
                notes: None,
                warranty_expires_on: None,
                warranty_expires_miles: None,
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn require_missing_is_not_found() {
        let db = test_db().await;
        let err = require(&db, 999).await.unwrap_err();
        assert!(matches!(err, DomainError::NotFound(m) if m == "Vehicle 999 not found"));
    }

    #[tokio::test]
    async fn create_then_update_mutates_field() {
        let db = test_db().await;
        let v = seed(&db, "Old").await;
        let updated = update(
            &db,
            v.id,
            UpdateVehicle {
                name: Some("New".into()),
                make: Some(Some("Toyota".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.name, "New");
        assert_eq!(updated.make.as_deref(), Some("Toyota"));
    }

    #[tokio::test]
    async fn archiving_records_the_sale_and_leaves_the_garage_list() {
        let db = test_db().await;
        let sold = seed(&db, "Old GTI").await;
        seed(&db, "Alltrack").await;
        let day = crate::test_support::date("2026-11-10");
        let archived = archive(
            &db,
            sold.id,
            VehicleSale {
                sold_date: Some(day),
                sold_price_cents: Some(1_850_000),
                sold_mileage: Some(98_000),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            (
                archived.sold_date,
                archived.sold_price_cents,
                archived.sold_mileage
            ),
            (Some(day), Some(1_850_000), Some(98_000))
        );
        let first_stamp = archived.archived_at;

        let names = |vs: Vec<vehicle::Model>| vs.into_iter().map(|v| v.name).collect::<Vec<_>>();
        assert_eq!(names(list(&db, false).await.unwrap()), ["Alltrack"]);
        assert_eq!(
            names(list(&db, true).await.unwrap()),
            ["Old GTI", "Alltrack"]
        );
        assert_eq!(
            get(&db, sold.id).await.unwrap().name,
            "Old GTI",
            "still readable"
        );

        // Archiving again records the sale and keeps the first stamp.
        let again = archive(&db, sold.id, VehicleSale::default()).await.unwrap();
        assert_eq!(again.archived_at, first_stamp);
        assert_eq!(again.sold_price_cents, Some(1_850_000));

        let back = unarchive(&db, sold.id).await.unwrap();
        assert_eq!(back.sold_date, Some(day), "the sale details stay");
        assert_eq!(list(&db, false).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn archiving_refuses_a_negative_price() {
        let db = test_db().await;
        let v = seed(&db, "Car").await;
        let err = archive(
            &db,
            v.id,
            VehicleSale {
                sold_price_cents: Some(-1),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { .. }), "{err:?}");
        assert!(get(&db, v.id).await.unwrap().archived_at.is_none());
    }

    #[tokio::test]
    async fn archiving_waits_for_open_work_to_be_settled() {
        use crate::{inputs::work_item::NewWorkItem, services::work_item as job};
        let db = test_db().await;
        let v = seed(&db, "Old GTI").await.id;
        let j = job::create(
            &db,
            v,
            NewWorkItem {
                title: "Wipers".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;

        let err = archive(&db, v, VehicleSale::default()).await.unwrap_err();
        let DomainError::BadRequest(message) = err else {
            panic!("{err:?}");
        };
        assert!(message.contains("1 planned job"), "{message}");
        assert!(get(&db, v).await.unwrap().archived_at.is_none());

        job::delete(&db, v, j).await.unwrap();
        assert!(archive(&db, v, VehicleSale::default()).await.is_ok());
    }

    #[tokio::test]
    async fn an_archived_car_takes_no_new_open_work() {
        use crate::{
            inputs::{appointment::NewAppointment, todo::NewTodo, work_item::NewWorkItem},
            services::{appointment, todo, work_item as job},
        };
        let db = test_db().await;
        let v = seed(&db, "Old GTI").await.id;
        archive(&db, v, VehicleSale::default()).await.unwrap();

        let refused = |r: DomainResult<()>| {
            assert!(
                matches!(&r, Err(DomainError::BadRequest(m)) if m.contains("is archived")),
                "{r:?}"
            );
        };
        refused(
            job::create(
                &db,
                v,
                NewWorkItem {
                    title: "Wipers".into(),
                    ..Default::default()
                },
            )
            .await
            .map(|_| ()),
        );
        refused(
            appointment::create(
                &db,
                v,
                NewAppointment {
                    purpose: "Brakes".into(),
                    ..Default::default()
                },
            )
            .await
            .map(|_| ()),
        );
        refused(
            todo::add(
                &db,
                v,
                NewTodo {
                    title: "Wash".into(),
                    ..Default::default()
                },
            )
            .await
            .map(|_| ()),
        );
    }

    #[tokio::test]
    async fn a_corrected_sale_price_is_not_negative_either() {
        let db = test_db().await;
        let v = seed(&db, "Car").await;
        let err = update(
            &db,
            v.id,
            UpdateVehicle {
                sold_price_cents: Some(Some(-1)),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { .. }), "{err:?}");
    }

    fn decoded() -> VinDecodeResult {
        VinDecodeResult {
            year: Some(2019),
            make: Some("Volkswagen".into()),
            model: Some("Golf R".into()),
            trim: Some("Base".into()),
            body_style: Some("Hatchback".into()),
            engine: Some("2.0L I4 turbo".into()),
            transmission: Some("Manual".into()),
            drivetrain: Some("AWD".into()),
            all_attributes: std::collections::HashMap::from([(
                "Plant City".to_string(),
                "Wolfsburg".to_string(),
            )]),
        }
    }

    async fn attribute_count(db: &impl ConnectionTrait, vehicle_id: i32) -> u64 {
        use crate::entities::vehicle_attribute;
        vehicle_attribute::Entity::find()
            .filter(vehicle_attribute::Column::VehicleId.eq(vehicle_id))
            .count(db)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn create_with_vin_decode_fills_blanks_from_the_decode() {
        let db = test_db().await;
        let made = create_with_vin_decode(
            &db,
            NewVehicle {
                name: "Daily".into(),
                vin: Some("WVWQ12345678901234".into()),
                ..Default::default()
            },
            Some(&decoded()),
        )
        .await
        .unwrap();
        assert_eq!(made.year, Some(2019));
        assert_eq!(made.make.as_deref(), Some("Volkswagen"));
        assert_eq!(made.model.as_deref(), Some("Golf R"));
        assert_eq!(made.trim_level.as_deref(), Some("Base"));
        assert_eq!(made.body_style.as_deref(), Some("Hatchback"));
        assert_eq!(made.engine.as_deref(), Some("2.0L I4 turbo"));
        assert_eq!(made.transmission.as_deref(), Some("Manual"));
        assert_eq!(made.drivetrain.as_deref(), Some("AWD"));
        assert_eq!(attribute_count(&db, made.id).await, 1);
    }

    #[tokio::test]
    async fn create_with_vin_decode_keeps_supplied_values() {
        let db = test_db().await;
        let made = create_with_vin_decode(
            &db,
            NewVehicle {
                name: "Daily".into(),
                make: Some("VW".into()),
                year: Some(2020),
                ..Default::default()
            },
            Some(&decoded()),
        )
        .await
        .unwrap();
        assert_eq!(made.make.as_deref(), Some("VW"));
        assert_eq!(made.year, Some(2020));
        assert_eq!(made.model.as_deref(), Some("Golf R"));
    }

    async fn seed_template(db: &impl ConnectionTrait) -> i32 {
        crate::entities::model_template::ActiveModel {
            year: Set(Some(2018)),
            make: Set(Some("Volkswagen".into())),
            model: Set(Some("Golf".into())),
            trim_level: Set(Some("SE".into())),
            body_style: Set(Some("Wagon".into())),
            engine: Set(Some("2.0L TSI".into())),
            transmission: Set(Some("6MT".into())),
            drivetrain: Set(Some("FWD".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn create_fills_empty_fields_from_the_template() {
        let db = test_db().await;
        let tid = seed_template(&db).await;
        let made = create(
            &db,
            NewVehicle {
                name: "Daily".into(),
                model_template_id: Some(tid),
                make: Some("VW".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(made.make.as_deref(), Some("VW"));
        assert_eq!(made.year, Some(2018));
        assert_eq!(made.model.as_deref(), Some("Golf"));
        assert_eq!(made.trim_level.as_deref(), Some("SE"));
        assert_eq!(made.body_style.as_deref(), Some("Wagon"));
        assert_eq!(made.engine.as_deref(), Some("2.0L TSI"));
        assert_eq!(made.transmission.as_deref(), Some("6MT"));
        assert_eq!(made.drivetrain.as_deref(), Some("FWD"));
    }

    #[tokio::test]
    async fn precedence_is_caller_then_template_then_vin_decode() {
        let db = test_db().await;
        let tid = seed_template(&db).await;
        // The decode carries the vague multi-value trim NHTSA returns for a GTI.
        let mut d = decoded();
        d.trim = Some("S, Sport, SE, Autobahn".into());
        let made = create_with_vin_decode(
            &db,
            NewVehicle {
                name: "Daily".into(),
                model_template_id: Some(tid),
                year: Some(2020),
                ..Default::default()
            },
            Some(&d),
        )
        .await
        .unwrap();
        assert_eq!(made.year, Some(2020), "caller wins");
        assert_eq!(
            made.trim_level.as_deref(),
            Some("SE"),
            "template beats decode"
        );
        assert_eq!(made.engine.as_deref(), Some("2.0L TSI"));
        assert_eq!(made.transmission.as_deref(), Some("6MT"));
        assert_eq!(made.body_style.as_deref(), Some("Wagon"));
        assert_eq!(attribute_count(&db, made.id).await, 1);
    }

    #[tokio::test]
    async fn update_setting_a_template_fills_only_empty_fields() {
        let db = test_db().await;
        let tid = seed_template(&db).await;
        let v = create(
            &db,
            NewVehicle {
                name: "Daily".into(),
                make: Some("VW".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let updated = update(
            &db,
            v.id,
            UpdateVehicle {
                model_template_id: Some(Some(tid)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.make.as_deref(), Some("VW"));
        assert_eq!(updated.transmission.as_deref(), Some("6MT"));
        assert_eq!(updated.year, Some(2018));
    }

    #[tokio::test]
    async fn create_with_vin_decode_without_a_decode_is_plain_create() {
        let db = test_db().await;
        let made = create_with_vin_decode(
            &db,
            NewVehicle {
                name: "Daily".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
        assert_eq!(made.make, None);
        assert_eq!(attribute_count(&db, made.id).await, 0);
    }

    #[tokio::test]
    async fn create_with_vin_decodes_the_vin_and_fills_the_blanks() {
        let db = test_db().await;
        let made = create_with_vin_lookup(
            &db,
            NewVehicle {
                name: "Daily".into(),
                make: Some("VW".into()),
                vin: Some(" WVWZZZAUZKW000001 ".into()),
                ..Default::default()
            },
            |vin| async move {
                assert_eq!(vin, "WVWZZZAUZKW000001");
                Ok(decoded())
            },
        )
        .await
        .unwrap();
        assert_eq!(made.vin_decode_error, None);
        assert_eq!(
            made.vehicle.vin.as_deref(),
            Some("WVWZZZAUZKW000001"),
            "the VIN is stored without its surrounding spaces"
        );
        assert_eq!(made.vehicle.make.as_deref(), Some("VW"));
        assert_eq!(made.vehicle.model.as_deref(), Some("Golf R"));
        assert_eq!(attribute_count(&db, made.vehicle.id).await, 1);
        // The reply is the vehicle's own keys, with no error key on success.
        let json = serde_json::to_value(&made).unwrap();
        assert_eq!(json["name"], "Daily");
        assert_eq!(json["id"], made.vehicle.id);
        assert_eq!(json["vin"], "WVWZZZAUZKW000001");
        assert!(
            json.get("vehicle").is_none(),
            "the vehicle's keys are flattened: {json}"
        );
        assert!(json.get("vin_decode_error").is_none(), "{json}");
    }

    #[tokio::test]
    async fn create_with_vin_still_creates_when_the_lookup_fails() {
        let db = test_db().await;
        let made = create_with_vin_lookup(
            &db,
            NewVehicle {
                name: "Daily".into(),
                vin: Some("WVWZZZAUZKW000001".into()),
                ..Default::default()
            },
            |_| async { Err("NHTSA API request failed: offline".to_string()) },
        )
        .await
        .unwrap();
        assert_eq!(made.vehicle.vin.as_deref(), Some("WVWZZZAUZKW000001"));
        assert_eq!(made.vehicle.make, None);
        assert_eq!(attribute_count(&db, made.vehicle.id).await, 0);
        let json = serde_json::to_value(&made).unwrap();
        assert_eq!(json["id"], made.vehicle.id);
        assert!(json.get("vehicle").is_none(), "{json}");
        assert_eq!(
            json["vin_decode_error"],
            "NHTSA API request failed: offline"
        );
    }

    #[tokio::test]
    async fn create_with_vin_makes_no_lookup_without_a_vin() {
        let db = test_db().await;
        for vin in [None, Some("  ".to_string())] {
            let made = create_with_vin_lookup(
                &db,
                NewVehicle {
                    name: "Daily".into(),
                    vin,
                    ..Default::default()
                },
                |_| async { panic!("no VIN was given, so nothing is looked up") },
            )
            .await
            .unwrap();
            assert_eq!(made.vin_decode_error, None);
            assert_eq!(made.vehicle.vin, None, "a blank VIN is stored as none");
        }
    }

    #[tokio::test]
    async fn create_with_vin_refuses_a_bad_input_before_the_lookup() {
        let db = test_db().await;
        let bad_mileage = NewVehicle {
            name: "Daily".into(),
            vin: Some("WVWZZZAUZKW000001".into()),
            purchase_mileage: Some(-1),
            ..Default::default()
        };
        let bad_template = NewVehicle {
            name: "Daily".into(),
            vin: Some("WVWZZZAUZKW000001".into()),
            model_template_id: Some(99_999),
            ..Default::default()
        };
        for (input, refusal) in [(bad_mileage, "purchase_mileage"), (bad_template, "99999")] {
            let err = create_with_vin_lookup(&db, input, |_| async {
                panic!("a refused input must not reach the lookup")
            })
            .await
            .unwrap_err();
            assert!(err.to_string().contains(refusal), "{err}");
        }
        crate::test_support::assert_empty(&list(&db, true).await.unwrap());
    }

    #[tokio::test]
    async fn an_unknown_model_template_is_not_found_on_create_and_update() {
        let db = test_db().await;
        let err = create_with_vin_decode(
            &db,
            NewVehicle {
                name: "Daily".into(),
                model_template_id: Some(99_999),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&err, DomainError::NotFound(m) if m.contains("list_model_templates")),
            "got: {err}"
        );

        let v = seed(&db, "Car").await;
        let err = update(
            &db,
            v.id,
            UpdateVehicle {
                model_template_id: Some(Some(99_999)),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)), "got: {err}");
    }

    #[tokio::test]
    async fn delete_unused_removes_a_fresh_vehicle_and_refuses_one_with_history() {
        let db = test_db().await;
        let fresh = seed(&db, "Duplicate").await;
        crate::services::mileage::create(
            &db,
            fresh.id,
            crate::inputs::mileage::NewMileageEntry {
                mileage: 10,
                recorded_at: None,
                source: None,
                notes: None,
            },
        )
        .await
        .unwrap();
        delete_unused(&db, fresh.id).await.unwrap();
        assert!(matches!(
            require(&db, fresh.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));

        let used = seed(&db, "Daily").await;
        part::ActiveModel {
            vehicle_id: Set(used.id),
            name: Set("Pads".into()),
            status: Set(PartStatus::Purchased),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let err = delete_unused(&db, used.id).await.unwrap_err();
        assert!(
            matches!(&err, DomainError::BadRequest(m) if m.contains("parts")),
            "got: {err}"
        );
        require(&db, used.id).await.unwrap();
    }

    // The appointments and to-dos tables cascade from the vehicle, so a vehicle
    // that holds either must be refused or the delete would take them with it.
    #[tokio::test]
    async fn delete_unused_refuses_a_vehicle_with_an_appointment_or_a_to_do() {
        let db = test_db().await;
        let planned = seed(&db, "Planned").await;
        crate::services::appointment::create(
            &db,
            planned.id,
            crate::inputs::appointment::NewAppointment {
                purpose: "60k service".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let err = delete_unused(&db, planned.id).await.unwrap_err();
        assert!(
            matches!(&err, DomainError::BadRequest(m) if m.contains("appointments")),
            "got: {err}"
        );
        assert_eq!(
            crate::test_support::row_count::<appointment::Entity>(&db).await,
            1
        );

        let reminded = seed(&db, "Reminded").await;
        crate::services::todo::add(
            &db,
            reminded.id,
            crate::inputs::todo::NewTodo {
                title: "Find the wheel lock key".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let err = delete_unused(&db, reminded.id).await.unwrap_err();
        assert!(
            matches!(&err, DomainError::BadRequest(m) if m.contains("to-dos")),
            "got: {err}"
        );
        assert_eq!(crate::test_support::row_count::<todo::Entity>(&db).await, 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn racing_edits_and_archives_of_one_vehicle_all_succeed() {
        let (_dir, pools) = crate::test_support::race_pools().await;
        let v = seed(&pools[0], "Daily").await.id;
        let template = seed_template(&pools[0]).await;
        let results = crate::test_support::race(&pools, |i, db| async move {
            match i % 4 {
                // A template makes the create read before it writes.
                3 => create(
                    &db,
                    NewVehicle {
                        name: format!("Car {i}"),
                        model_template_id: Some(template),
                        ..Default::default()
                    },
                )
                .await
                .map(|_| ()),
                0 => update(
                    &db,
                    v,
                    UpdateVehicle {
                        name: Some(format!("Daily {i}")),
                        ..Default::default()
                    },
                )
                .await
                .map(|_| ()),
                1 => archive(&db, v, VehicleSale::default()).await.map(|_| ()),
                _ => unarchive(&db, v).await.map(|_| ()),
            }
        })
        .await;
        assert!(results.iter().all(Result::is_ok), "{results:?}");
    }
}
