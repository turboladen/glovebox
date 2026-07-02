use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::{
    entities::shop,
    inputs::shop::{NewShop, UpdateShop as UpdateShopInput},
    services::shop as svc,
};

use super::{error::ApiError, serde_helpers::deserialize_optional};

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
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub address: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub phone: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub website: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub specialty: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub notes: Option<Option<String>>,
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<shop::Model>>> {
    Ok(Json(svc::list(&state.db).await?))
}

async fn get_one(State(state): State<AppState>, Path(id): Path<i32>) -> Result<Json<shop::Model>> {
    Ok(Json(svc::get(&state.db, id).await?))
}

async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateShop>,
) -> Result<Json<shop::Model>> {
    let created = svc::create(
        &state.db,
        NewShop {
            name: input.name,
            address: input.address,
            phone: input.phone,
            website: input.website,
            specialty: input.specialty,
            notes: input.notes,
        },
    )
    .await?;
    Ok(Json(created))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(input): Json<UpdateShop>,
) -> Result<Json<shop::Model>> {
    let updated = svc::update(
        &state.db,
        id,
        UpdateShopInput {
            name: input.name,
            address: input.address,
            phone: input.phone,
            website: input.website,
            specialty: input.specialty,
            notes: input.notes,
        },
    )
    .await?;
    Ok(Json(updated))
}

async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>> {
    let deleted = svc::delete(&state.db, id).await?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).put(update).delete(delete))
}
