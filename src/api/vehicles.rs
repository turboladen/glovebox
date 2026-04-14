use axum::extract::{Multipart, Path, State};
use axum::routing::get;
use axum::{Json, Router};
use sea_orm::*;
use serde::Deserialize;

use crate::entities::vehicle;
use crate::AppState;

use super::error::ApiError;
use super::serde_helpers::deserialize_optional;

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
    let vehicles = vehicle::Entity::find().all(&state.db).await?;
    Ok(Json(vehicles))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<vehicle::Model>> {
    vehicle::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("Vehicle {id} not found")))
}

async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateVehicle>,
) -> Result<Json<vehicle::Model>> {
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
    let result = model.insert(&state.db).await?;
    Ok(Json(result))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(input): Json<UpdateVehicle>,
) -> Result<Json<vehicle::Model>> {
    let existing = vehicle::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Vehicle {id} not found")))?;

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

    let result = active.update(&state.db).await?;
    Ok(Json(result))
}

async fn upload_photo(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    mut multipart: Multipart,
) -> Result<Json<vehicle::Model>> {
    let existing = vehicle::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Vehicle {id} not found")))?;

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

    // Update vehicle photo_path
    let mut active: vehicle::ActiveModel = existing.into();
    active.photo_path = Set(Some(relative_path));
    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
    let result = active.update(&state.db).await?;

    Ok(Json(result))
}

async fn archive(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<vehicle::Model>> {
    let existing = vehicle::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Vehicle {id} not found")))?;

    let mut active: vehicle::ActiveModel = existing.into();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    active.archived_at = Set(Some(now));
    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
    let result = active.update(&state.db).await?;
    Ok(Json(result))
}

async fn unarchive(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<vehicle::Model>> {
    let existing = vehicle::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Vehicle {id} not found")))?;

    let mut active: vehicle::ActiveModel = existing.into();
    active.archived_at = Set(None);
    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
    let result = active.update(&state.db).await?;
    Ok(Json(result))
}

async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>> {
    let existing = vehicle::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Vehicle {id} not found")))?;

    if existing.archived_at.is_none() {
        return Err(ApiError::BadRequest(
            "Vehicle must be archived before it can be deleted".into(),
        ));
    }

    vehicle::Entity::delete_by_id(id).exec(&state.db).await?;

    // Remove vehicle's file directory from disk
    if let Ok(files_dir) = std::path::Path::new(&state.config.files_dir).canonicalize() {
        let vehicle_dir = files_dir.join(id.to_string());
        if vehicle_dir.exists() {
            if let Ok(canonical) = vehicle_dir.canonicalize() {
                if canonical.starts_with(&files_dir) {
                    if let Err(e) = tokio::fs::remove_dir_all(&canonical).await {
                        tracing::warn!("Failed to remove vehicle files at {}: {e}", canonical.display());
                    }
                }
            }
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
