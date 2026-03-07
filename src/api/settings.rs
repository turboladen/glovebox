use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use sea_orm::*;
use serde::Deserialize;

use crate::entities::settings;
use crate::AppState;

use super::error::ApiError;

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct UpsertSetting {
    pub value: String,
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<settings::Model>>> {
    let items = settings::Entity::find().all(&state.db).await?;
    Ok(Json(items))
}

async fn get_one(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<settings::Model>> {
    settings::Entity::find_by_id(&key)
        .one(&state.db)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("Setting '{key}' not found")))
}

async fn upsert(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(input): Json<UpsertSetting>,
) -> Result<Json<settings::Model>> {
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let txn = state.db.begin().await?;
    let existing = settings::Entity::find_by_id(&key).one(&txn).await?;

    let result = if let Some(existing) = existing {
        let mut active: settings::ActiveModel = existing.into();
        active.value = Set(input.value);
        active.updated_at = Set(now);
        active.update(&txn).await?
    } else {
        let model = settings::ActiveModel {
            key: Set(key),
            value: Set(input.value),
            created_at: Set(now.clone()),
            updated_at: Set(now),
        };
        model.insert(&txn).await?
    };

    txn.commit().await?;
    Ok(Json(result))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/{key}", get(get_one).put(upsert))
}
