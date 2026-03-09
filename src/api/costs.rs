use axum::extract::{Path, State};
use axum::Json;
use sea_orm::{QueryOrder, QueryFilter, EntityTrait, ColumnTrait, Iterable, ActiveModelBehavior, Iden, ActiveModelTrait, ModelTrait};
use serde::Serialize;

use crate::entities::{part, service_record};
use crate::AppState;

use super::error::ApiError;
use super::require_vehicle;

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Serialize)]
pub struct CostSummary {
    pub vehicle_id: i32,
    pub total_service_cost_cents: i64,
    pub total_parts_cost_cents: i64,
    pub total_labor_cost_cents: i64,
    pub total_cost_cents: i64,
    pub service_count: usize,
    pub part_count: usize,
    pub cost_per_mile_cents: Option<i64>,
    pub monthly_costs: Vec<MonthlyCost>,
}

#[derive(Serialize)]
pub struct MonthlyCost {
    pub month: String,
    pub service_cost_cents: i64,
    pub parts_cost_cents: i64,
    pub total_cents: i64,
}

pub async fn get_costs(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<CostSummary>> {
    let v = require_vehicle(&state.db, vehicle_id).await?;

    let services = service_record::Entity::find()
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .order_by_asc(service_record::Column::ServiceDate)
        .all(&state.db)
        .await?;

    let parts = part::Entity::find()
        .filter(part::Column::VehicleId.eq(vehicle_id))
        .all(&state.db)
        .await?;

    let total_service_cost_cents: i64 = services
        .iter()
        .filter_map(|s| s.total_cost_cents)
        .map(|c| i64::from(c))
        .sum();

    let total_labor_cost_cents: i64 = services
        .iter()
        .filter_map(|s| s.labor_cost_cents)
        .map(|c| i64::from(c))
        .sum();

    let total_parts_cost_from_services: i64 = services
        .iter()
        .filter_map(|s| s.parts_cost_cents)
        .map(|c| i64::from(c))
        .sum();

    let total_parts_purchased: i64 = parts
        .iter()
        .filter_map(|p| p.cost_cents)
        .map(|c| i64::from(c))
        .sum();

    // Parts cost: use purchased parts total (more accurate than service-reported)
    let total_parts_cost_cents = total_parts_purchased.max(total_parts_cost_from_services);
    // Total: service totals (which include labor + parts on the bill) plus any
    // separately purchased parts not already counted in service records
    let extra_parts_cost = (total_parts_purchased - total_parts_cost_from_services).max(0);
    let total_cost_cents = total_service_cost_cents + extra_parts_cost;

    // Cost per mile: total cost / (current mileage - purchase mileage)
    let cost_per_mile_cents = if let Some(purchase_mi) = v.purchase_mileage {
        // Find the most recent service mileage as a proxy for current mileage
        let latest_mileage = services.iter().rev().find_map(|s| s.mileage);
        latest_mileage.and_then(|current| {
            let miles_driven = i64::from(current) - i64::from(purchase_mi);
            if miles_driven > 0 {
                Some(total_cost_cents / miles_driven)
            } else {
                None
            }
        })
    } else {
        None
    };

    // Monthly cost breakdown
    let mut monthly_map: std::collections::BTreeMap<String, (i64, i64)> =
        std::collections::BTreeMap::new();
    for svc in &services {
        let month = svc
            .service_date
            .get(..7)
            .unwrap_or(&svc.service_date)
            .to_string();
        let entry = monthly_map.entry(month).or_insert((0, 0));
        entry.0 += i64::from(svc.total_cost_cents.unwrap_or(0));
    }
    for p in &parts {
        if let Some(date) = &p.purchase_date {
            let month = date.get(..7).unwrap_or(date).to_string();
            let entry = monthly_map.entry(month).or_insert((0, 0));
            entry.1 += i64::from(p.cost_cents.unwrap_or(0));
        }
    }
    let monthly_costs: Vec<MonthlyCost> = monthly_map
        .into_iter()
        .map(|(month, (svc, prt))| MonthlyCost {
            month,
            service_cost_cents: svc,
            parts_cost_cents: prt,
            total_cents: svc + prt,
        })
        .collect();

    Ok(Json(CostSummary {
        vehicle_id,
        total_service_cost_cents,
        total_parts_cost_cents,
        total_labor_cost_cents,
        total_cost_cents,
        service_count: services.len(),
        part_count: parts.len(),
        cost_per_mile_cents,
        monthly_costs,
    }))
}
