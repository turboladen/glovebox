use sea_orm::*;

use crate::{
    entities::{part, service_record},
    error::{DomainError, DomainResult},
    inputs::part::{NewPart, PartFilter, UpdatePart},
};

/// Verify a referenced service record belongs to the vehicle. A cross-vehicle
/// service must be indistinguishable from a nonexistent one.
async fn require_service_record_owned(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    service_id: i32,
) -> DomainResult<()> {
    service_record::Entity::find_by_id(service_id)
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Service record {service_id} not found")))?;
    Ok(())
}

pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    filter: PartFilter,
) -> DomainResult<Vec<part::Model>> {
    let mut query = part::Entity::find().filter(part::Column::VehicleId.eq(vehicle_id));

    if let Some(status) = filter.status {
        query = query.filter(part::Column::Status.eq(status));
    }

    Ok(query.order_by_desc(part::Column::CreatedAt).all(db).await?)
}

pub async fn get(db: &impl ConnectionTrait, vehicle_id: i32, id: i32) -> DomainResult<part::Model> {
    part::Entity::find_by_id(id)
        .filter(part::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Part {id} not found")))
}

pub async fn create(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    input: NewPart,
) -> DomainResult<part::Model> {
    if let Some(service_id) = input.installed_service_id {
        require_service_record_owned(db, vehicle_id, service_id).await?;
    }
    if let Some(build_id) = input.build_id {
        crate::services::build::require_owned(db, vehicle_id, build_id).await?;
    }

    let model = part::ActiveModel {
        vehicle_id: Set(vehicle_id),
        name: Set(input.name),
        manufacturer: Set(input.manufacturer),
        part_number: Set(input.part_number),
        oe_part_number_replaced: Set(input.oe_part_number_replaced),
        seller: Set(input.seller),
        purchase_date: Set(input.purchase_date),
        cost_cents: Set(input.cost_cents),
        cost_currency: Set(input.cost_currency),
        invoice_url: Set(input.invoice_url),
        manufacturer_url: Set(input.manufacturer_url),
        retailer_url: Set(input.retailer_url),
        status: Set(input.status.unwrap_or_else(|| "purchased".to_string())),
        installed_date: Set(input.installed_date),
        installed_odometer: Set(input.installed_odometer),
        installed_service_id: Set(input.installed_service_id),
        notes: Set(input.notes),
        build_id: Set(input.build_id),
        location: Set(input.location),
        warranty_expires_on: Set(input.warranty_expires_on),
        warranty_expires_miles: Set(input.warranty_expires_miles),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

pub async fn update(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
    input: UpdatePart,
) -> DomainResult<part::Model> {
    let existing = get(db, vehicle_id, id).await?;

    if let Some(Some(service_id)) = input.installed_service_id {
        require_service_record_owned(db, vehicle_id, service_id).await?;
    }
    if let Some(Some(build_id)) = input.build_id {
        crate::services::build::require_owned(db, vehicle_id, build_id).await?;
    }

    let mut active: part::ActiveModel = existing.into();

    if let Some(v) = input.name {
        active.name = Set(v);
    }
    if let Some(v) = input.manufacturer {
        active.manufacturer = Set(v);
    }
    if let Some(v) = input.part_number {
        active.part_number = Set(v);
    }
    if let Some(v) = input.oe_part_number_replaced {
        active.oe_part_number_replaced = Set(v);
    }
    if let Some(v) = input.seller {
        active.seller = Set(v);
    }
    if let Some(v) = input.purchase_date {
        active.purchase_date = Set(v);
    }
    if let Some(v) = input.cost_cents {
        active.cost_cents = Set(v);
    }
    if let Some(v) = input.cost_currency {
        active.cost_currency = Set(v);
    }
    if let Some(v) = input.invoice_url {
        active.invoice_url = Set(v);
    }
    if let Some(v) = input.manufacturer_url {
        active.manufacturer_url = Set(v);
    }
    if let Some(v) = input.retailer_url {
        active.retailer_url = Set(v);
    }
    if let Some(v) = input.status {
        active.status = Set(v);
    }
    if let Some(v) = input.installed_date {
        active.installed_date = Set(v);
    }
    if let Some(v) = input.installed_odometer {
        active.installed_odometer = Set(v);
    }
    if let Some(v) = input.installed_service_id {
        active.installed_service_id = Set(v);
    }
    if let Some(v) = input.replaced_date {
        active.replaced_date = Set(v);
    }
    if let Some(v) = input.replaced_odometer {
        active.replaced_odometer = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }
    if let Some(v) = input.build_id {
        active.build_id = Set(v);
    }
    if let Some(v) = input.location {
        active.location = Set(v);
    }
    if let Some(v) = input.warranty_expires_on {
        active.warranty_expires_on = Set(v);
    }
    if let Some(v) = input.warranty_expires_miles {
        active.warranty_expires_miles = Set(v);
    }

    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
    Ok(active.update(db).await?)
}

pub async fn delete(db: &impl ConnectionTrait, vehicle_id: i32, id: i32) -> DomainResult<()> {
    let existing = get(db, vehicle_id, id).await?;
    existing.delete(db).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_db;

    async fn seed_vehicle(db: &impl ConnectionTrait) -> i32 {
        use crate::entities::vehicle;
        vehicle::ActiveModel {
            name: Set("Car".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn seed_service(db: &impl ConnectionTrait, vehicle_id: i32) -> i32 {
        use crate::entities::service_record;
        service_record::ActiveModel {
            vehicle_id: Set(vehicle_id),
            service_date: Set("2024-01-01".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    fn minimal_part(name: &str, installed_service_id: Option<i32>) -> NewPart {
        NewPart {
            name: name.into(),
            manufacturer: None,
            part_number: None,
            oe_part_number_replaced: None,
            seller: None,
            purchase_date: None,
            cost_cents: None,
            cost_currency: None,
            invoice_url: None,
            manufacturer_url: None,
            retailer_url: None,
            status: None,
            installed_date: None,
            installed_odometer: None,
            installed_service_id,
            notes: None,
            build_id: None,
            location: None,
            warranty_expires_on: None,
            warranty_expires_miles: None,
        }
    }

    #[tokio::test]
    async fn create_defaults_status_and_get_round_trips() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = create(
            &db,
            vid,
            NewPart {
                cost_cents: Some(1_599),
                ..minimal_part("Oil filter", None)
            },
        )
        .await
        .unwrap();
        assert_eq!(created.status, "purchased");
        let fetched = get(&db, vid, created.id).await.unwrap();
        assert_eq!(fetched.name, "Oil filter");
        assert_eq!(fetched.cost_cents, Some(1_599));
    }

    #[tokio::test]
    async fn location_round_trips_and_clears() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = create(
            &db,
            vid,
            NewPart {
                location: Some("Front brakes".into()),
                ..minimal_part("Brake pads", None)
            },
        )
        .await
        .unwrap();
        assert_eq!(created.location, Some("Front brakes".into()));

        // Omitted location survives an unrelated update.
        let updated = update(
            &db,
            vid,
            created.id,
            UpdatePart {
                status: Some("installed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.location, Some("Front brakes".into()));

        // Explicit null clears it (double-option convention).
        let cleared = update(
            &db,
            vid,
            created.id,
            UpdatePart {
                location: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.location, None);
    }

    #[tokio::test]
    async fn update_preserves_installed_service_link() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc_id = seed_service(&db, vid).await;
        let p = create(&db, vid, minimal_part("Brake pads", Some(svc_id)))
            .await
            .unwrap();
        assert_eq!(p.installed_service_id, Some(svc_id));

        // Update an unrelated field; the installed_service_id link must survive.
        let updated = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                status: Some("installed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.status, "installed");
        assert_eq!(updated.installed_service_id, Some(svc_id));

        // Explicitly clear the link.
        let cleared = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                installed_service_id: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.installed_service_id, None);
    }

    #[tokio::test]
    async fn create_rejects_other_vehicles_service() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign_svc = seed_service(&db, other).await;

        // Referencing another vehicle's service must 404 and create nothing.
        assert!(matches!(
            create(&db, owner, minimal_part("Part", Some(foreign_svc)))
                .await
                .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(
            list(&db, owner, PartFilter::default())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn update_rejects_other_vehicles_service() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign_svc = seed_service(&db, other).await;
        let p = create(&db, owner, minimal_part("Part", None))
            .await
            .unwrap();

        assert!(matches!(
            update(
                &db,
                owner,
                p.id,
                UpdatePart {
                    installed_service_id: Some(Some(foreign_svc)),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        // The part is untouched by the rejected update.
        let fetched = get(&db, owner, p.id).await.unwrap();
        assert_eq!(fetched.installed_service_id, None);
    }

    #[tokio::test]
    async fn list_filters_by_status() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        create(
            &db,
            vid,
            NewPart {
                status: Some("installed".into()),
                ..minimal_part("A", None)
            },
        )
        .await
        .unwrap();
        create(
            &db,
            vid,
            NewPart {
                status: Some("purchased".into()),
                ..minimal_part("B", None)
            },
        )
        .await
        .unwrap();

        assert_eq!(
            list(&db, vid, PartFilter::default()).await.unwrap().len(),
            2
        );
        assert_eq!(
            list(
                &db,
                vid,
                PartFilter {
                    status: Some("purchased".into())
                }
            )
            .await
            .unwrap()
            .len(),
            1
        );
    }
}
