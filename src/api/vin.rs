use axum::extract::{Path, State};
use axum::Json;
use sea_orm::{EntityTrait, TransactionTrait, QueryFilter, ColumnTrait, Set, Iden, ActiveModelTrait};
use serde::Serialize;

use crate::entities::vehicle_attribute;
use crate::services::vin_decode;
use crate::AppState;

use super::error::ApiError;

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Serialize)]
pub struct VinDecodeResponse {
    pub vin: String,
    #[serde(flatten)]
    pub decoded: vin_decode::VinDecodeResult,
}

/// Decode a VIN without creating a vehicle — useful for the setup wizard preview step
pub async fn decode(Path(vin): Path<String>) -> Result<Json<VinDecodeResponse>> {
    let decoded = vin_decode::decode_vin(&vin)
        .await
        .map_err(ApiError::BadRequest)?;

    Ok(Json(VinDecodeResponse { vin, decoded }))
}

/// Decode a VIN and store the attributes on an existing vehicle
pub async fn decode_and_store(
    State(state): State<AppState>,
    Path((vehicle_id, vin)): Path<(i32, String)>,
) -> Result<Json<VinDecodeResponse>> {
    use crate::entities::vehicle;

    // Verify vehicle exists
    vehicle::Entity::find_by_id(vehicle_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Vehicle {vehicle_id} not found")))?;

    let decoded = vin_decode::decode_vin(&vin)
        .await
        .map_err(ApiError::BadRequest)?;

    let txn = state.db.begin().await?;

    // Delete any existing vin_decode attributes for this vehicle, then re-insert
    vehicle_attribute::Entity::delete_many()
        .filter(vehicle_attribute::Column::VehicleId.eq(vehicle_id))
        .filter(vehicle_attribute::Column::Source.eq("vin_decode"))
        .exec(&txn)
        .await?;

    for (key, value) in &decoded.all_attributes {
        let attr = vehicle_attribute::ActiveModel {
            vehicle_id: Set(vehicle_id),
            key: Set(key.clone()),
            value: Set(value.clone()),
            source: Set(Some("vin_decode".to_string())),
            ..Default::default()
        };
        attr.insert(&txn).await?;
    }

    txn.commit().await?;

    Ok(Json(VinDecodeResponse { vin, decoded }))
}
