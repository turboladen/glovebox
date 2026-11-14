//! To-dos: small tasks for a vehicle, optionally tied to an appointment.

use sea_orm::*;

use crate::{entities::todo, error::DomainResult};

/// Inserts a to-do for a vehicle, on an appointment when one is given. The
/// caller has already checked that the appointment belongs to the vehicle.
pub(crate) async fn insert(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    appointment_id: Option<i32>,
    title: String,
) -> DomainResult<todo::Model> {
    let model = todo::ActiveModel {
        vehicle_id: Set(vehicle_id),
        appointment_id: Set(appointment_id),
        title: Set(title),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}
