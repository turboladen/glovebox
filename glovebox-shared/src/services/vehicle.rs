use sea_orm::*;

use crate::{
    entities::{
        incident, part, research_finding, research_report, service_record, vehicle, work_item,
    },
    error::{DomainError, DomainResult},
    inputs::vehicle::{NewVehicle, UpdateVehicle},
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

pub async fn list(db: &impl ConnectionTrait) -> DomainResult<Vec<vehicle::Model>> {
    Ok(vehicle::Entity::find().all(db).await?)
}

pub async fn get(db: &impl ConnectionTrait, id: i32) -> DomainResult<vehicle::Model> {
    require(db, id).await
}

/// Fails with a `NotFound` that points at `list_model_templates` when the id
/// names no template.
async fn require_template(db: &impl ConnectionTrait, id: Option<i32>) -> DomainResult<()> {
    if let Some(id) = id
        && crate::entities::model_template::Entity::find_by_id(id)
            .one(db)
            .await?
            .is_none()
    {
        return Err(DomainError::NotFound(format!(
            "Model template {id} not found; list_model_templates shows the valid ids"
        )));
    }
    Ok(())
}

pub async fn create(db: &impl ConnectionTrait, input: NewVehicle) -> DomainResult<vehicle::Model> {
    super::validate_optional_date("purchase_date", input.purchase_date.as_deref())?;
    super::validate_optional_date("warranty_expires_on", input.warranty_expires_on.as_deref())?;
    require_template(db, input.model_template_id).await?;
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
        purchase_price_currency: Set(input.purchase_price_currency),
        purchase_mileage: Set(input.purchase_mileage),
        notes: Set(input.notes),
        warranty_expires_on: Set(input.warranty_expires_on),
        warranty_expires_miles: Set(input.warranty_expires_miles),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

/// Creates a vehicle, filling the fields the caller left empty from a VIN
/// decode and storing the decoded attributes.
///
/// Year, make, model, trim, body style, engine, transmission and drivetrain
/// each take the decoded value only when the input has none; a supplied value
/// always wins. With `decoded` of `None` this behaves like [`create`]. The
/// decode is a parameter so the lookup (a live web request) stays with the
/// caller and this function stays testable offline.
pub async fn create_with_vin_decode<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    mut input: NewVehicle,
    decoded: Option<&VinDecodeResult>,
) -> DomainResult<vehicle::Model> {
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
    let txn = db.begin().await?;
    let created = create(&txn, input).await?;
    if let Some(d) = decoded {
        vin_decode::store_attributes(&txn, created.id, d).await?;
    }
    txn.commit().await?;
    Ok(created)
}

#[allow(clippy::too_many_lines)]
pub async fn update(
    db: &impl ConnectionTrait,
    id: i32,
    input: UpdateVehicle,
) -> DomainResult<vehicle::Model> {
    let existing = require(db, id).await?;
    for (field, value) in [
        ("purchase_date", &input.purchase_date),
        ("sold_date", &input.sold_date),
        ("warranty_expires_on", &input.warranty_expires_on),
    ] {
        super::validate_optional_date(field, value.as_ref().and_then(|v| v.as_deref()))?;
    }
    require_template(db, input.model_template_id.flatten()).await?;
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
    if let Some(v) = input.purchase_price_currency {
        active.purchase_price_currency = Set(v);
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
    if let Some(v) = input.sold_price_currency {
        active.sold_price_currency = Set(v);
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
    active.updated_at = Set(super::now_stamp());

    Ok(active.update(db).await?)
}

pub async fn archive(db: &impl ConnectionTrait, id: i32) -> DomainResult<vehicle::Model> {
    let existing = require(db, id).await?;
    let mut active: vehicle::ActiveModel = existing.into();
    let now = super::now_stamp();
    active.archived_at = Set(Some(now.clone()));
    active.updated_at = Set(now);
    Ok(active.update(db).await?)
}

pub async fn unarchive(db: &impl ConnectionTrait, id: i32) -> DomainResult<vehicle::Model> {
    let existing = require(db, id).await?;
    let mut active: vehicle::ActiveModel = existing.into();
    active.archived_at = Set(None);
    active.updated_at = Set(super::now_stamp());
    Ok(active.update(db).await?)
}

/// Delete a vehicle. It must be archived first. File cleanup is the caller's responsibility.
pub async fn delete(db: &impl ConnectionTrait, id: i32) -> DomainResult<()> {
    let existing = require(db, id).await?;

    if existing.archived_at.is_none() {
        return Err(DomainError::BadRequest(
            "Vehicle must be archived before it can be deleted".to_string(),
        ));
    }

    vehicle::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

/// Deletes a vehicle that has no history yet, undoing a mistaken
/// `create`. A vehicle with any service record, part, incident, work item or
/// research finding is a `BadRequest` naming what it has. Its odometer
/// readings, own schedule items and decoded attributes go with it.
pub async fn delete_unused(db: &impl ConnectionTrait, id: i32) -> DomainResult<()> {
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_db;

    async fn seed(db: &impl ConnectionTrait, name: &str) -> vehicle::Model {
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
                purchase_price_currency: None,
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
    async fn delete_requires_archived() {
        let db = test_db().await;
        let v = seed(&db, "Car").await;
        // Not archived -> rejected
        assert!(matches!(
            delete(&db, v.id).await.unwrap_err(),
            DomainError::BadRequest(_)
        ));
        // Archive then delete succeeds
        archive(&db, v.id).await.unwrap();
        delete(&db, v.id).await.unwrap();
        assert!(matches!(
            require(&db, v.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn archive_then_unarchive_toggles() {
        let db = test_db().await;
        let v = seed(&db, "Car").await;
        assert!(archive(&db, v.id).await.unwrap().archived_at.is_some());
        assert!(unarchive(&db, v.id).await.unwrap().archived_at.is_none());
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
            status: Set("purchased".into()),
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
}
