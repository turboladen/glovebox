use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{part, service_record},
    error::DomainResult,
    services::vehicle,
};

#[derive(Debug, Serialize)]
pub struct ExportRecord {
    pub date: String,
    pub mileage: Option<i32>,
    pub description: Option<String>,
    pub total_cost: Option<String>,
    pub shop: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExportPart {
    pub name: String,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub installed_date: Option<String>,
    pub installed_odometer: Option<i32>,
    pub cost: Option<String>,
}

#[derive(Debug, Serialize)]
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

/// Assemble the full service/parts export for a vehicle.
pub async fn vehicle_history(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<VehicleExport> {
    let v = vehicle::require(db, vehicle_id).await?;

    let services = service_record::Entity::find()
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .order_by_asc(service_record::Column::ServiceDate)
        .all(db)
        .await?;

    let parts = part::Entity::find()
        .filter(part::Column::VehicleId.eq(vehicle_id))
        .filter(part::Column::Status.eq("installed"))
        .order_by_asc(part::Column::InstalledDate)
        .all(db)
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

    Ok(VehicleExport {
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{VehicleFixture, test_db};

    #[tokio::test]
    async fn export_totals_and_counts() {
        let db = test_db().await;
        let vid = VehicleFixture::new().make("Honda").insert_id(&db).await;

        service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set("2024-01-01".into()),
            total_cost_cents: Set(Some(5_000)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        part::ActiveModel {
            vehicle_id: Set(vid),
            name: Set("Filter".into()),
            status: Set("installed".into()),
            cost_cents: Set(Some(2_500)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        // A non-installed part is excluded from the export
        part::ActiveModel {
            vehicle_id: Set(vid),
            name: Set("Spare".into()),
            status: Set("purchased".into()),
            cost_cents: Set(Some(9_999)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let export = vehicle_history(&db, vid).await.unwrap();
        assert_eq!(export.vehicle_name, "Car");
        assert_eq!(export.make.as_deref(), Some("Honda"));
        assert_eq!(export.record_count, 1);
        assert_eq!(export.installed_parts.len(), 1);
        assert_eq!(export.total_service_cost, "$50.00");
        assert_eq!(export.total_parts_cost, "$25.00");
        assert_eq!(export.total_cost, "$75.00");
    }

    #[tokio::test]
    async fn export_missing_vehicle_is_not_found() {
        let db = test_db().await;
        assert!(matches!(
            vehicle_history(&db, 999).await.unwrap_err(),
            crate::error::DomainError::NotFound(_)
        ));
    }
}
