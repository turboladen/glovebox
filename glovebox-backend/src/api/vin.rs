use axum::{
    Json,
    extract::{Path, State},
};
use serde::Serialize;

use crate::AppState;
use glovebox_shared::services::{vehicle as vehicle_svc, vin_decode};

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
    // Verify vehicle exists (404 before attempting the decode)
    vehicle_svc::require(&state.db, vehicle_id).await?;

    let decoded = vin_decode::decode_vin(&vin)
        .await
        .map_err(ApiError::BadRequest)?;

    vin_decode::store_attributes(&state.db, vehicle_id, &decoded).await?;

    Ok(Json(VinDecodeResponse { vin, decoded }))
}
