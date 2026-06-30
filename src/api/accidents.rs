use axum::{
    Json,
    extract::{Path, State},
};
use sea_orm::*;
use serde::{Deserialize, Serialize};

use crate::{
    AppState,
    entities::{accident, accident_correspondence, accident_service_link},
};

use super::{error::ApiError, require_vehicle, serde_helpers::deserialize_optional};

type Result<T> = std::result::Result<T, ApiError>;

// --- DTOs ---

#[derive(Deserialize)]
pub struct CreateAccident {
    pub occurred_at: String,
    pub odometer: Option<i32>,
    pub description: String,
    pub fault: Option<String>,
    pub other_party_name: Option<String>,
    pub other_party_phone: Option<String>,
    pub other_party_email: Option<String>,
    pub other_party_insurance: Option<String>,
    pub other_party_policy_number: Option<String>,
    pub insurance_claim_number: Option<String>,
    pub insurance_adjuster: Option<String>,
    pub insurance_adjuster_phone: Option<String>,
    pub notes: Option<String>,
    pub service_record_ids: Option<Vec<i32>>,
}

#[derive(Deserialize)]
pub struct UpdateAccident {
    pub occurred_at: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub odometer: Option<Option<i32>>,
    pub description: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub fault: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub other_party_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub other_party_phone: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub other_party_email: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub other_party_insurance: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub other_party_policy_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub insurance_claim_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub insurance_adjuster: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub insurance_adjuster_phone: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub total_repair_cost_cents: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub total_repair_cost_currency: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub deductible_cents: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub deductible_currency: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub insurance_payout_cents: Option<Option<i32>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub insurance_payout_currency: Option<Option<String>>,
    pub resolved: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub notes: Option<Option<String>>,
    pub service_record_ids: Option<Vec<i32>>,
}

#[derive(Serialize)]
pub struct AccidentWithDetails {
    #[serde(flatten)]
    pub accident: accident::Model,
    pub correspondence: Vec<accident_correspondence::Model>,
    pub service_record_ids: Vec<i32>,
}

#[derive(Deserialize)]
pub struct CreateCorrespondence {
    pub occurred_at: String,
    pub contact_method: Option<String>,
    pub contact_with: Option<String>,
    pub summary: String,
    pub notes: Option<String>,
}

// --- Helpers ---

/// Load correspondence and service link IDs for a single accident.
async fn load_accident_details(
    db: &impl ConnectionTrait,
    accident_id: i32,
) -> Result<(Vec<accident_correspondence::Model>, Vec<i32>)> {
    let correspondence = accident_correspondence::Entity::find()
        .filter(accident_correspondence::Column::AccidentId.eq(accident_id))
        .order_by_asc(accident_correspondence::Column::OccurredAt)
        .all(db)
        .await?;

    let links = accident_service_link::Entity::find()
        .filter(accident_service_link::Column::AccidentId.eq(accident_id))
        .all(db)
        .await?;
    let service_record_ids = links.into_iter().map(|l| l.service_record_id).collect();

    Ok((correspondence, service_record_ids))
}

// --- Handlers ---

pub async fn list(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<Vec<AccidentWithDetails>>> {
    require_vehicle(&state.db, vehicle_id).await?;

    let accidents = accident::Entity::find()
        .filter(accident::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(accident::Column::OccurredAt)
        .all(&state.db)
        .await?;

    // Batch-load all correspondence and service links (avoids N+1)
    let accident_ids: Vec<i32> = accidents.iter().map(|a| a.id).collect();

    let all_correspondence = if accident_ids.is_empty() {
        vec![]
    } else {
        accident_correspondence::Entity::find()
            .filter(accident_correspondence::Column::AccidentId.is_in(accident_ids.clone()))
            .order_by_asc(accident_correspondence::Column::OccurredAt)
            .all(&state.db)
            .await?
    };

    let all_links = if accident_ids.is_empty() {
        vec![]
    } else {
        accident_service_link::Entity::find()
            .filter(accident_service_link::Column::AccidentId.is_in(accident_ids))
            .all(&state.db)
            .await?
    };

    let results = accidents
        .into_iter()
        .map(|acc| {
            let correspondence: Vec<_> = all_correspondence
                .iter()
                .filter(|c| c.accident_id == acc.id)
                .cloned()
                .collect();
            let service_record_ids = all_links
                .iter()
                .filter(|l| l.accident_id == acc.id)
                .map(|l| l.service_record_id)
                .collect();
            AccidentWithDetails {
                accident: acc,
                correspondence,
                service_record_ids,
            }
        })
        .collect();

    Ok(Json(results))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
) -> Result<Json<AccidentWithDetails>> {
    require_vehicle(&state.db, vehicle_id).await?;

    let acc = accident::Entity::find_by_id(id)
        .filter(accident::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Accident {id} not found")))?;

    let (correspondence, service_record_ids) = load_accident_details(&state.db, acc.id).await?;

    Ok(Json(AccidentWithDetails {
        accident: acc,
        correspondence,
        service_record_ids,
    }))
}

pub async fn create(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
    Json(input): Json<CreateAccident>,
) -> Result<Json<AccidentWithDetails>> {
    require_vehicle(&state.db, vehicle_id).await?;

    let txn = state.db.begin().await?;

    let model = accident::ActiveModel {
        vehicle_id: Set(vehicle_id),
        occurred_at: Set(input.occurred_at),
        odometer: Set(input.odometer),
        description: Set(input.description),
        fault: Set(input.fault),
        other_party_name: Set(input.other_party_name),
        other_party_phone: Set(input.other_party_phone),
        other_party_email: Set(input.other_party_email),
        other_party_insurance: Set(input.other_party_insurance),
        other_party_policy_number: Set(input.other_party_policy_number),
        insurance_claim_number: Set(input.insurance_claim_number),
        insurance_adjuster: Set(input.insurance_adjuster),
        insurance_adjuster_phone: Set(input.insurance_adjuster_phone),
        notes: Set(input.notes),
        ..Default::default()
    };
    let acc = model.insert(&txn).await?;

    let service_record_ids = input.service_record_ids.unwrap_or_default();
    for sid in &service_record_ids {
        let link = accident_service_link::ActiveModel {
            accident_id: Set(acc.id),
            service_record_id: Set(*sid),
        };
        link.insert(&txn).await?;
    }

    txn.commit().await?;

    Ok(Json(AccidentWithDetails {
        accident: acc,
        correspondence: vec![],
        service_record_ids,
    }))
}

#[allow(clippy::too_many_lines)]
pub async fn update(
    State(state): State<AppState>,
    Path((vehicle_id, id)): Path<(i32, i32)>,
    Json(input): Json<UpdateAccident>,
) -> Result<Json<AccidentWithDetails>> {
    require_vehicle(&state.db, vehicle_id).await?;

    let existing = accident::Entity::find_by_id(id)
        .filter(accident::Column::VehicleId.eq(vehicle_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Accident {id} not found")))?;

    let txn = state.db.begin().await?;

    let mut active: accident::ActiveModel = existing.into();

    if let Some(v) = input.occurred_at {
        active.occurred_at = Set(v);
    }
    if let Some(v) = input.odometer {
        active.odometer = Set(v);
    }
    if let Some(v) = input.description {
        active.description = Set(v);
    }
    if let Some(v) = input.fault {
        active.fault = Set(v);
    }
    if let Some(v) = input.other_party_name {
        active.other_party_name = Set(v);
    }
    if let Some(v) = input.other_party_phone {
        active.other_party_phone = Set(v);
    }
    if let Some(v) = input.other_party_email {
        active.other_party_email = Set(v);
    }
    if let Some(v) = input.other_party_insurance {
        active.other_party_insurance = Set(v);
    }
    if let Some(v) = input.other_party_policy_number {
        active.other_party_policy_number = Set(v);
    }
    if let Some(v) = input.insurance_claim_number {
        active.insurance_claim_number = Set(v);
    }
    if let Some(v) = input.insurance_adjuster {
        active.insurance_adjuster = Set(v);
    }
    if let Some(v) = input.insurance_adjuster_phone {
        active.insurance_adjuster_phone = Set(v);
    }
    if let Some(v) = input.total_repair_cost_cents {
        active.total_repair_cost_cents = Set(v);
    }
    if let Some(v) = input.total_repair_cost_currency {
        active.total_repair_cost_currency = Set(v);
    }
    if let Some(v) = input.deductible_cents {
        active.deductible_cents = Set(v);
    }
    if let Some(v) = input.deductible_currency {
        active.deductible_currency = Set(v);
    }
    if let Some(v) = input.insurance_payout_cents {
        active.insurance_payout_cents = Set(v);
    }
    if let Some(v) = input.insurance_payout_currency {
        active.insurance_payout_currency = Set(v);
    }
    if let Some(v) = input.resolved {
        active.resolved = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }

    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

    let acc = active.update(&txn).await?;

    let service_record_ids = if let Some(sids) = input.service_record_ids {
        accident_service_link::Entity::delete_many()
            .filter(accident_service_link::Column::AccidentId.eq(acc.id))
            .exec(&txn)
            .await?;
        for sid in &sids {
            let link = accident_service_link::ActiveModel {
                accident_id: Set(acc.id),
                service_record_id: Set(*sid),
            };
            link.insert(&txn).await?;
        }
        sids
    } else {
        let links = accident_service_link::Entity::find()
            .filter(accident_service_link::Column::AccidentId.eq(acc.id))
            .all(&txn)
            .await?;
        links.into_iter().map(|l| l.service_record_id).collect()
    };

    let correspondence = accident_correspondence::Entity::find()
        .filter(accident_correspondence::Column::AccidentId.eq(acc.id))
        .order_by_asc(accident_correspondence::Column::OccurredAt)
        .all(&txn)
        .await?;

    txn.commit().await?;

    Ok(Json(AccidentWithDetails {
        accident: acc,
        correspondence,
        service_record_ids,
    }))
}

// --- Correspondence sub-resource ---

pub async fn list_correspondence(
    State(state): State<AppState>,
    Path((_vehicle_id, accident_id)): Path<(i32, i32)>,
) -> Result<Json<Vec<accident_correspondence::Model>>> {
    let items = accident_correspondence::Entity::find()
        .filter(accident_correspondence::Column::AccidentId.eq(accident_id))
        .order_by_asc(accident_correspondence::Column::OccurredAt)
        .all(&state.db)
        .await?;
    Ok(Json(items))
}

pub async fn create_correspondence(
    State(state): State<AppState>,
    Path((_vehicle_id, accident_id)): Path<(i32, i32)>,
    Json(input): Json<CreateCorrespondence>,
) -> Result<Json<accident_correspondence::Model>> {
    accident::Entity::find_by_id(accident_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Accident {accident_id} not found")))?;

    let model = accident_correspondence::ActiveModel {
        accident_id: Set(accident_id),
        occurred_at: Set(input.occurred_at),
        contact_method: Set(input.contact_method),
        contact_with: Set(input.contact_with),
        summary: Set(input.summary),
        notes: Set(input.notes),
        ..Default::default()
    };
    let result = model.insert(&state.db).await?;
    Ok(Json(result))
}
