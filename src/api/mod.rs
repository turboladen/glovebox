pub mod accidents;
pub mod ai;
pub mod conversations;
pub mod costs;
pub mod documents;
pub mod error;
pub mod export;
pub mod health;
pub mod mileage;
pub mod model_templates;
pub mod observations;
pub mod part_slots;
pub mod parts;
pub mod platforms;
pub mod reminders;
pub mod research;
pub mod schedules;
pub mod serde_helpers;
pub mod services;
pub mod shops;
pub mod vehicles;
pub mod vin;

use crate::entities::vehicle;
use error::ApiError;
use sea_orm::{DatabaseConnection, EntityTrait};

/// Shared helper to verify a vehicle exists, returning the model or a 404 error.
pub async fn require_vehicle(
    db: &DatabaseConnection,
    vehicle_id: i32,
) -> Result<vehicle::Model, ApiError> {
    vehicle::Entity::find_by_id(vehicle_id)
        .one(db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Vehicle {vehicle_id} not found")))
}
