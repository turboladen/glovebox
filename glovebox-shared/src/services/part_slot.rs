use sea_orm::*;

use crate::{
    entities::part_slot,
    error::{DomainError, DomainResult},
    inputs::part_slot::{NewPartSlot, UpdatePartSlot},
};

pub async fn list(db: &impl ConnectionTrait, vehicle_id: i32) -> DomainResult<Vec<part_slot::Model>> {
    Ok(part_slot::Entity::find()
        .filter(part_slot::Column::VehicleId.eq(vehicle_id))
        .order_by_asc(part_slot::Column::Category)
        .order_by_asc(part_slot::Column::Name)
        .all(db)
        .await?)
}

pub async fn get(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<part_slot::Model> {
    part_slot::Entity::find_by_id(id)
        .filter(part_slot::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Part slot {id} not found")))
}

pub async fn create(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    input: NewPartSlot,
) -> DomainResult<part_slot::Model> {
    let model = part_slot::ActiveModel {
        vehicle_id: Set(vehicle_id),
        name: Set(input.name),
        category: Set(input.category),
        oe_spec: Set(input.oe_spec),
        oe_part_number: Set(input.oe_part_number),
        notes: Set(input.notes),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

pub async fn update(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
    input: UpdatePartSlot,
) -> DomainResult<part_slot::Model> {
    let existing = get(db, vehicle_id, id).await?;
    let mut active: part_slot::ActiveModel = existing.into();

    if let Some(v) = input.name {
        active.name = Set(v);
    }
    if let Some(v) = input.category {
        active.category = Set(v);
    }
    if let Some(v) = input.oe_spec {
        active.oe_spec = Set(v);
    }
    if let Some(v) = input.oe_part_number {
        active.oe_part_number = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }

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
    async fn create_then_get_round_trips() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = create(
            &db,
            vid,
            NewPartSlot {
                name: "Front brake pads".into(),
                category: Some("brakes".into()),
                oe_spec: None,
                oe_part_number: None,
                notes: None,
            },
        )
        .await
        .unwrap();
        let fetched = get(&db, vid, created.id).await.unwrap();
        assert_eq!(fetched.name, "Front brake pads");
    }

    #[tokio::test]
    async fn update_and_delete() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let s = create(
            &db,
            vid,
            NewPartSlot {
                name: "A".into(),
                category: None,
                oe_spec: None,
                oe_part_number: None,
                notes: Some("x".into()),
            },
        )
        .await
        .unwrap();
        let updated = update(
            &db,
            vid,
            s.id,
            UpdatePartSlot {
                name: Some("B".into()),
                notes: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.name, "B");
        assert_eq!(updated.notes, None);

        delete(&db, vid, s.id).await.unwrap();
        assert!(matches!(
            get(&db, vid, s.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn get_missing_is_not_found() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        assert!(matches!(
            get(&db, vid, 999).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }
}
