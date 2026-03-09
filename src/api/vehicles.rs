use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use sea_orm::*;
use serde::Deserialize;

use crate::entities::vehicle;
use crate::AppState;

use super::error::ApiError;

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
    pub model_template_id: Option<Option<i32>>,
    pub year: Option<Option<i32>>,
    pub make: Option<Option<String>>,
    pub model: Option<Option<String>>,
    pub trim_level: Option<Option<String>>,
    pub body_style: Option<Option<String>>,
    pub engine: Option<Option<String>>,
    pub transmission: Option<Option<String>>,
    pub drivetrain: Option<Option<String>>,
    pub vin: Option<Option<String>>,
    pub license_plate: Option<Option<String>>,
    pub color: Option<Option<String>>,
    pub purchase_date: Option<Option<String>>,
    pub purchase_price_cents: Option<Option<i32>>,
    pub purchase_price_currency: Option<Option<String>>,
    pub purchase_mileage: Option<Option<i32>>,
    pub sold_date: Option<Option<String>>,
    pub sold_price_cents: Option<Option<i32>>,
    pub sold_price_currency: Option<Option<String>>,
    pub sold_mileage: Option<Option<i32>>,
    pub photo_path: Option<Option<String>>,
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

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).put(update))
}
