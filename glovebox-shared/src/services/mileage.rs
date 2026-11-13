use sea_orm::*;

use crate::{
    entities::mileage_log,
    error::{DomainError, DomainResult},
    inputs::mileage::NewMileageEntry,
};

/// The vehicle's odometer readings, newest first. A reading a service logged
/// carries that service's `service_record_id`.
pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<Vec<mileage_log::Model>> {
    crate::services::vehicle::require(db, vehicle_id).await?;
    Ok(mileage_log::Entity::find()
        .filter(mileage_log::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(mileage_log::Column::RecordedAt)
        .all(db)
        .await?)
}

pub async fn create(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    input: NewMileageEntry,
) -> DomainResult<mileage_log::Model> {
    super::vehicle::require(db, vehicle_id).await?;
    if let Some(at) = &input.recorded_at {
        super::validate_datetime("recorded_at", at)?;
    }
    let now = super::local_now_stamp();
    let model = mileage_log::ActiveModel {
        vehicle_id: Set(vehicle_id),
        mileage: Set(input.mileage),
        recorded_at: Set(input.recorded_at.unwrap_or(now)),
        source: Set(input.source.or_else(|| Some("manual".to_string()))),
        notes: Set(input.notes),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

/// Deletes one odometer reading. A reading another vehicle owns reads as
/// nonexistent. A reading a service logged follows that service's mileage,
/// so deleting it is a `BadRequest`; correct the service instead.
pub async fn delete(db: &impl ConnectionTrait, vehicle_id: i32, id: i32) -> DomainResult<()> {
    let reading = mileage_log::Entity::find_by_id(id)
        .filter(mileage_log::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Mileage reading {id} not found")))?;
    if let Some(service_id) = reading.service_record_id {
        return Err(DomainError::BadRequest(format!(
            "Mileage reading {id} was logged by service {service_id}. Change or clear that \
             service's mileage with correct_service instead."
        )));
    }
    reading.delete(db).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{VehicleFixture, test_db};

    async fn seed_vehicle(db: &impl ConnectionTrait) -> i32 {
        VehicleFixture::new().insert_id(db).await
    }

    #[tokio::test]
    async fn create_then_list_round_trips() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = create(
            &db,
            vid,
            NewMileageEntry {
                mileage: 42000,
                recorded_at: None,
                source: None,
                notes: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(created.mileage, 42000);
        // default source applied
        assert_eq!(created.source.as_deref(), Some("manual"));

        let entries = list(&db, vid).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].mileage, 42000);
    }

    #[tokio::test]
    async fn list_scoped_to_vehicle() {
        let db = test_db().await;
        let v1 = seed_vehicle(&db).await;
        let v2 = seed_vehicle(&db).await;
        create(
            &db,
            v1,
            NewMileageEntry {
                mileage: 100,
                recorded_at: Some("2024-01-01 00:00:00".into()),
                source: Some("import".into()),
                notes: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(list(&db, v1).await.unwrap().len(), 1);
        assert_eq!(list(&db, v2).await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn delete_removes_a_manual_reading_and_refuses_a_service_one() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let manual = create(
            &db,
            vid,
            NewMileageEntry {
                mileage: 480_000,
                recorded_at: None,
                source: None,
                notes: None,
            },
        )
        .await
        .unwrap();
        let service = crate::services::service_record::create(
            &db,
            vid,
            crate::inputs::service_record::NewServiceRecord {
                service_date: "2026-05-02".into(),
                mileage: Some(48_100),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let logged = list(&db, vid)
            .await
            .unwrap()
            .into_iter()
            .find(|r| r.service_record_id == Some(service.record.id))
            .unwrap();

        assert!(matches!(
            delete(&db, vid, logged.id).await.unwrap_err(),
            DomainError::BadRequest(_)
        ));
        let foreign = delete(&db, other, manual.id).await.unwrap_err();
        let missing = delete(&db, other, 99_999).await.unwrap_err();
        assert_eq!(
            foreign.to_string().replace(&manual.id.to_string(), "N"),
            missing.to_string().replace("99999", "N")
        );

        delete(&db, vid, manual.id).await.unwrap();
        let left = list(&db, vid).await.unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, logged.id);
    }
}
