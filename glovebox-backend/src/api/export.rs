use axum::{
    Json,
    extract::{Path, State},
};
use sea_orm::*;
use serde::Serialize;

use crate::AppState;
use glovebox_shared::entities::{part, service_record};

use super::{error::ApiError, require_vehicle};

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Serialize)]
pub struct ExportRecord {
    pub date: String,
    pub mileage: Option<i32>,
    pub description: Option<String>,
    pub total_cost: Option<String>,
    pub shop: Option<String>,
    pub notes: Option<String>,
}

#[derive(Serialize)]
pub struct ExportPart {
    pub name: String,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub installed_date: Option<String>,
    pub installed_odometer: Option<i32>,
    pub cost: Option<String>,
}

#[derive(Serialize)]
pub struct VehicleExport {
    pub vehicle_name: String,
    pub year: Option<i32>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub vin: Option<String>,
    pub service_records: Vec<ExportRecord>,
    pub installed_parts: Vec<ExportPart>,
    pub total_service_cost: String,
    pub total_parts_cost: String,
    pub total_cost: String,
    pub record_count: usize,
}

fn fmt_cost(cents: i64) -> String {
    let dollars = cents / 100;
    let remainder = (cents % 100).unsigned_abs();
    format!("${dollars}.{remainder:02}")
}

pub async fn export_history(
    State(state): State<AppState>,
    Path(vehicle_id): Path<i32>,
) -> Result<Json<VehicleExport>> {
    let v = require_vehicle(&state.db, vehicle_id).await?;

    let services = service_record::Entity::find()
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .order_by_asc(service_record::Column::ServiceDate)
        .all(&state.db)
        .await?;

    let parts = part::Entity::find()
        .filter(part::Column::VehicleId.eq(vehicle_id))
        .filter(part::Column::Status.eq("installed"))
        .order_by_asc(part::Column::InstalledDate)
        .all(&state.db)
        .await?;

    let total_svc: i64 = services
        .iter()
        .filter_map(|s| s.total_cost_cents)
        .map(i64::from)
        .sum();

    let total_prt: i64 = parts
        .iter()
        .filter_map(|p| p.cost_cents)
        .map(i64::from)
        .sum();

    let records: Vec<ExportRecord> = services
        .iter()
        .map(|s| ExportRecord {
            date: s.service_date.clone(),
            mileage: s.mileage,
            description: s.description.clone(),
            total_cost: s.total_cost_cents.map(|c| fmt_cost(i64::from(c))),
            shop: s.shop_name.clone(),
            notes: s.notes.clone(),
        })
        .collect();

    let installed_parts: Vec<ExportPart> = parts
        .iter()
        .map(|p| ExportPart {
            name: p.name.clone(),
            manufacturer: p.manufacturer.clone(),
            part_number: p.part_number.clone(),
            installed_date: p.installed_date.clone(),
            installed_odometer: p.installed_odometer,
            cost: p.cost_cents.map(|c| fmt_cost(i64::from(c))),
        })
        .collect();

    let record_count = records.len();

    Ok(Json(VehicleExport {
        vehicle_name: v.name,
        year: v.year,
        make: v.make,
        model: v.model,
        vin: v.vin,
        service_records: records,
        installed_parts,
        total_service_cost: fmt_cost(total_svc),
        total_parts_cost: fmt_cost(total_prt),
        total_cost: fmt_cost(total_svc + total_prt),
        record_count,
    }))
}
