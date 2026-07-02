use axum::{
    Json, Router,
    extract::{Multipart, Path, State},
    routing::get,
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::{
    entities::vehicle,
    inputs::vehicle::{NewVehicle, UpdateVehicle as UpdateVehicleInput},
    services::vehicle as svc,
};

use super::{error::ApiError, serde_helpers::deserialize_optional};

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct CreateVehicle {
    pub name: String,
    pub model_template_id: Option<i32>,
    pub year: Option<i32>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub trim_level: Option<String>,
    pub body_style: Option<String>,
    pub engine: Option<String>,
    pub transmission: Option<String>,
    pub drivetrain: Option<String>,
    pub vin: Option<String>,
    pub license_plate: Option<String>,
    pub color: Option<String>,
    pub purchase_date: Option<String>,
    pub purchase_price_cents: Option<i32>,
    pub purchase_price_currency: Option<String>,
    pub purchase_mileage: Option<i32>,
    pub photo_path: Option<String>,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateVehicle {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub model_template_id: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub year: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub make: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub model: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub trim_level: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub body_style: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub engine: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub transmission: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub drivetrain: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub vin: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub license_plate: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub color: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub purchase_date: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub purchase_price_cents: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub purchase_price_currency: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub purchase_mileage: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub sold_date: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub sold_price_cents: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub sold_price_currency: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub sold_mileage: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub photo_path: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub notes: Option<Option<String>>,
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<vehicle::Model>>> {
    Ok(Json(svc::list(&state.db).await?))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<vehicle::Model>> {
    Ok(Json(svc::get(&state.db, id).await?))
}

async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateVehicle>,
) -> Result<Json<vehicle::Model>> {
    let created = svc::create(
        &state.db,
        NewVehicle {
            name: input.name,
            model_template_id: input.model_template_id,
            year: input.year,
            make: input.make,
            model: input.model,
            trim_level: input.trim_level,
            body_style: input.body_style,
            engine: input.engine,
            transmission: input.transmission,
            drivetrain: input.drivetrain,
            vin: input.vin,
            license_plate: input.license_plate,
            color: input.color,
            purchase_date: input.purchase_date,
            purchase_price_cents: input.purchase_price_cents,
            purchase_price_currency: input.purchase_price_currency,
            purchase_mileage: input.purchase_mileage,
            photo_path: input.photo_path,
            notes: input.notes,
        },
    )
    .await?;
    Ok(Json(created))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(input): Json<UpdateVehicle>,
) -> Result<Json<vehicle::Model>> {
    let updated = svc::update(
        &state.db,
        id,
        UpdateVehicleInput {
            name: input.name,
            model_template_id: input.model_template_id,
            year: input.year,
            make: input.make,
            model: input.model,
            trim_level: input.trim_level,
            body_style: input.body_style,
            engine: input.engine,
            transmission: input.transmission,
            drivetrain: input.drivetrain,
            vin: input.vin,
            license_plate: input.license_plate,
            color: input.color,
            purchase_date: input.purchase_date,
            purchase_price_cents: input.purchase_price_cents,
            purchase_price_currency: input.purchase_price_currency,
            purchase_mileage: input.purchase_mileage,
            sold_date: input.sold_date,
            sold_price_cents: input.sold_price_cents,
            sold_price_currency: input.sold_price_currency,
            sold_mileage: input.sold_mileage,
            photo_path: input.photo_path,
            notes: input.notes,
        },
    )
    .await?;
    Ok(Json(updated))
}

async fn upload_photo(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    mut multipart: Multipart,
) -> Result<Json<vehicle::Model>> {
    // Verify the vehicle exists before touching the filesystem.
    svc::require(&state.db, id).await?;

    // Extract file from multipart
    let field = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::BadRequest(format!("Multipart error: {e}")))?
        .ok_or_else(|| ApiError::BadRequest("No file provided".into()))?;

    let file_name = field
        .file_name()
        .map_or_else(|| "photo.jpg".into(), std::string::ToString::to_string);
    let data = field
        .bytes()
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    // Save to {files_dir}/{vehicle_id}/photos/
    let dir: std::path::PathBuf = [&state.config.files_dir, &id.to_string(), "photos"]
        .iter()
        .collect();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let safe_name = super::documents::sanitize_filename(&file_name);
    let stored_name = format!("{timestamp}_{safe_name}");
    let full_path = dir.join(&stored_name);

    tokio::fs::write(&full_path, &data)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let relative_path = format!("{id}/photos/{stored_name}");

    Ok(Json(
        svc::set_photo_path(&state.db, id, relative_path).await?,
    ))
}

async fn archive(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<vehicle::Model>> {
    Ok(Json(svc::archive(&state.db, id).await?))
}

async fn unarchive(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<vehicle::Model>> {
    Ok(Json(svc::unarchive(&state.db, id).await?))
}

async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>> {
    svc::delete(&state.db, id).await?;

    // Remove vehicle's file directory from disk
    if let Ok(files_dir) = std::path::Path::new(&state.config.files_dir).canonicalize() {
        let vehicle_dir = files_dir.join(id.to_string());
        if vehicle_dir.exists()
            && let Ok(canonical) = vehicle_dir.canonicalize()
            && canonical.starts_with(&files_dir)
            && let Err(e) = tokio::fs::remove_dir_all(&canonical).await
        {
            tracing::warn!(
                "Failed to remove vehicle files at {}: {e}",
                canonical.display()
            );
        }
    }

    Ok(Json(serde_json::json!({ "deleted": id })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).put(update).delete(delete))
        .route("/{id}/archive", axum::routing::post(archive))
        .route("/{id}/unarchive", axum::routing::post(unarchive))
        .route("/{id}/photo", axum::routing::post(upload_photo))
}
