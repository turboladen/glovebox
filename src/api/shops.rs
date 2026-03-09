use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use sea_orm::*;
use serde::Deserialize;

use crate::entities::shop;
use crate::AppState;

use super::error::ApiError;

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct CreateShop {
    pub name: String,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub website: Option<String>,
    pub specialty: Option<String>,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateShop {
    pub name: Option<String>,
    pub address: Option<Option<String>>,
    pub phone: Option<Option<String>>,
    pub website: Option<Option<String>>,
    pub specialty: Option<Option<String>>,
    pub notes: Option<Option<String>>,
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<shop::Model>>> {
    let shops = shop::Entity::find()
        .order_by_asc(shop::Column::Name)
        .all(&state.db)
        .await?;
    Ok(Json(shops))
}

async fn get_one(State(state): State<AppState>, Path(id): Path<i32>) -> Result<Json<shop::Model>> {
    shop::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("Shop {id} not found")))
}

async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateShop>,
) -> Result<Json<shop::Model>> {
    let model = shop::ActiveModel {
        name: Set(input.name),
        address: Set(input.address),
        phone: Set(input.phone),
        website: Set(input.website),
        specialty: Set(input.specialty),
        notes: Set(input.notes),
        ..Default::default()
    };
    let result = model.insert(&state.db).await?;
    Ok(Json(result))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(input): Json<UpdateShop>,
) -> Result<Json<shop::Model>> {
    let existing = shop::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Shop {id} not found")))?;

    let mut active: shop::ActiveModel = existing.into();

    if let Some(v) = input.name {
        active.name = Set(v);
    }
    if let Some(v) = input.address {
        active.address = Set(v);
    }
    if let Some(v) = input.phone {
        active.phone = Set(v);
    }
    if let Some(v) = input.website {
        active.website = Set(v);
    }
    if let Some(v) = input.specialty {
        active.specialty = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }

    let result = active.update(&state.db).await?;
    Ok(Json(result))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).put(update))
}
