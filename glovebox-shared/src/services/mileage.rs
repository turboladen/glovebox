use sea_orm::*;

use crate::{entities::mileage_log, error::DomainResult, inputs::mileage::NewMileageEntry};

pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<Vec<mileage_log::Model>> {
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
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let model = mileage_log::ActiveModel {
        vehicle_id: Set(vehicle_id),
        mileage: Set(input.mileage),
        recorded_at: Set(input.recorded_at.unwrap_or(now)),
        source: Set(input.source.or(Some("manual".to_string()))),
        notes: Set(input.notes),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
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
}
