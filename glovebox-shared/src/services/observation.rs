use sea_orm::*;

use crate::{
    entities::observation,
    error::{DomainError, DomainResult},
    inputs::observation::{NewObservation, UpdateObservation},
};

pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<Vec<observation::Model>> {
    Ok(observation::Entity::find()
        .filter(observation::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(observation::Column::ObservedAt)
        .all(db)
        .await?)
}

pub async fn get(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<observation::Model> {
    observation::Entity::find_by_id(id)
        .filter(observation::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Observation {id} not found")))
}

pub async fn create(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    input: NewObservation,
) -> DomainResult<observation::Model> {
    let mut model = observation::ActiveModel {
        vehicle_id: Set(vehicle_id),
        category: Set(input.category),
        title: Set(input.title),
        description: Set(input.description),
        odometer: Set(input.odometer),
        obd_codes: Set(input.obd_codes),
        notes: Set(input.notes),
        ..Default::default()
    };

    if let Some(observed_at) = input.observed_at {
        model.observed_at = Set(observed_at);
    }

    Ok(model.insert(db).await?)
}

pub async fn update(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
    input: UpdateObservation,
) -> DomainResult<observation::Model> {
    let existing = get(db, vehicle_id, id).await?;
    let mut active: observation::ActiveModel = existing.into();

    if let Some(v) = input.category {
        active.category = Set(v);
    }
    if let Some(v) = input.title {
        active.title = Set(v);
    }
    if let Some(v) = input.description {
        active.description = Set(v);
    }
    if let Some(v) = input.odometer {
        active.odometer = Set(v);
    }
    if let Some(v) = input.observed_at {
        active.observed_at = Set(v);
    }
    if let Some(v) = input.obd_codes {
        active.obd_codes = Set(v);
    }
    if let Some(v) = input.resolved {
        active.resolved = Set(v);
    }
    if let Some(v) = input.resolved_service_id {
        active.resolved_service_id = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }

    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
    Ok(active.update(db).await?)
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
            NewObservation {
                category: "noise".into(),
                title: "Squeaky brakes".into(),
                description: Some("front left".into()),
                odometer: Some(50_000),
                observed_at: None,
                obd_codes: None,
                notes: None,
            },
        )
        .await
        .unwrap();
        let fetched = get(&db, vid, created.id).await.unwrap();
        assert_eq!(fetched.title, "Squeaky brakes");
        assert!(!fetched.resolved);
    }

    #[tokio::test]
    async fn update_marks_resolved_and_clears_description() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let o = create(
            &db,
            vid,
            NewObservation {
                category: "noise".into(),
                title: "T".into(),
                description: Some("d".into()),
                odometer: None,
                observed_at: None,
                obd_codes: None,
                notes: None,
            },
        )
        .await
        .unwrap();
        let updated = update(
            &db,
            vid,
            o.id,
            UpdateObservation {
                resolved: Some(true),
                description: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(updated.resolved);
        assert_eq!(updated.description, None);
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

    #[tokio::test]
    async fn get_wrong_vehicle_is_not_found() {
        let db = test_db().await;
        let v1 = seed_vehicle(&db).await;
        let v2 = seed_vehicle(&db).await;
        let o = create(
            &db,
            v1,
            NewObservation {
                category: "c".into(),
                title: "t".into(),
                description: None,
                odometer: None,
                observed_at: None,
                obd_codes: None,
                notes: None,
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            get(&db, v2, o.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }
}
