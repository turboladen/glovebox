use sea_orm::*;

use crate::{
    entities::part,
    error::{DomainError, DomainResult},
    inputs::part::{NewPart, PartFilter, UpdatePart},
};

pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    filter: PartFilter,
) -> DomainResult<Vec<part::Model>> {
    let mut query = part::Entity::find().filter(part::Column::VehicleId.eq(vehicle_id));

    if let Some(slot_id) = filter.slot_id {
        query = query.filter(part::Column::SlotId.eq(slot_id));
    }
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
    let model = part::ActiveModel {
        vehicle_id: Set(vehicle_id),
        slot_id: Set(input.slot_id),
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
    let mut active: part::ActiveModel = existing.into();

    if let Some(v) = input.slot_id {
        active.slot_id = Set(v);
    }
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

    #[tokio::test]
    async fn create_defaults_status_and_get_round_trips() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = create(
            &db,
            vid,
            NewPart {
                slot_id: None,
                name: "Oil filter".into(),
                manufacturer: None,
                part_number: None,
                oe_part_number_replaced: None,
                seller: None,
                purchase_date: None,
                cost_cents: Some(1_599),
                cost_currency: None,
                invoice_url: None,
                manufacturer_url: None,
                retailer_url: None,
                status: None,
                installed_date: None,
                installed_odometer: None,
                installed_service_id: None,
                notes: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(created.status, "purchased");
        let fetched = get(&db, vid, created.id).await.unwrap();
        assert_eq!(fetched.name, "Oil filter");
        assert_eq!(fetched.cost_cents, Some(1_599));
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

    async fn seed_slot(db: &impl ConnectionTrait, vehicle_id: i32, name: &str) -> i32 {
        use crate::entities::part_slot;
        part_slot::ActiveModel {
            vehicle_id: Set(vehicle_id),
            name: Set(name.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn update_preserves_installed_service_link() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc_id = seed_service(&db, vid).await;
        let p = create(
            &db,
            vid,
            NewPart {
                slot_id: None,
                name: "Brake pads".into(),
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
                installed_service_id: Some(svc_id),
                notes: None,
            },
        )
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
    async fn list_filters_by_slot_and_status() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let slot1 = seed_slot(&db, vid, "slot1").await;
        let slot2 = seed_slot(&db, vid, "slot2").await;
        create(
            &db,
            vid,
            NewPart {
                slot_id: Some(slot1),
                name: "A".into(),
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
                status: Some("installed".into()),
                installed_date: None,
                installed_odometer: None,
                installed_service_id: None,
                notes: None,
            },
        )
        .await
        .unwrap();
        create(
            &db,
            vid,
            NewPart {
                slot_id: Some(slot2),
                name: "B".into(),
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
                status: Some("purchased".into()),
                installed_date: None,
                installed_odometer: None,
                installed_service_id: None,
                notes: None,
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
                    slot_id: Some(slot1),
                    status: None
                }
            )
            .await
            .unwrap()
            .len(),
            1
        );
        assert_eq!(
            list(
                &db,
                vid,
                PartFilter {
                    slot_id: None,
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
