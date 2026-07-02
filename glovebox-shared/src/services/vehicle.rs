use sea_orm::*;

use crate::{
    entities::vehicle,
    error::{DomainError, DomainResult},
    inputs::vehicle::{NewVehicle, UpdateVehicle},
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

pub async fn create(db: &impl ConnectionTrait, input: NewVehicle) -> DomainResult<vehicle::Model> {
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
        photo_path: Set(input.photo_path),
        notes: Set(input.notes),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

#[allow(clippy::too_many_lines)]
pub async fn update(
    db: &impl ConnectionTrait,
    id: i32,
    input: UpdateVehicle,
) -> DomainResult<vehicle::Model> {
    let existing = require(db, id).await?;
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
    if let Some(v) = input.photo_path {
        active.photo_path = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }
    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

    Ok(active.update(db).await?)
}

/// Set the vehicle's photo path (after the file has been written to disk by the caller).
pub async fn set_photo_path(
    db: &impl ConnectionTrait,
    id: i32,
    relative_path: String,
) -> DomainResult<vehicle::Model> {
    let existing = require(db, id).await?;
    let mut active: vehicle::ActiveModel = existing.into();
    active.photo_path = Set(Some(relative_path));
    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
    Ok(active.update(db).await?)
}

pub async fn archive(db: &impl ConnectionTrait, id: i32) -> DomainResult<vehicle::Model> {
    let existing = require(db, id).await?;
    let mut active: vehicle::ActiveModel = existing.into();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    active.archived_at = Set(Some(now));
    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
    Ok(active.update(db).await?)
}

pub async fn unarchive(db: &impl ConnectionTrait, id: i32) -> DomainResult<vehicle::Model> {
    let existing = require(db, id).await?;
    let mut active: vehicle::ActiveModel = existing.into();
    active.archived_at = Set(None);
    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
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
                photo_path: None,
                notes: None,
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
}
