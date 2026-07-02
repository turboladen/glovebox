use sea_orm::*;

use crate::{
    entities::shop,
    error::{DomainError, DomainResult},
    inputs::shop::{NewShop, UpdateShop},
};

pub async fn list(db: &impl ConnectionTrait) -> DomainResult<Vec<shop::Model>> {
    Ok(shop::Entity::find()
        .order_by_asc(shop::Column::Name)
        .all(db)
        .await?)
}

pub async fn get(db: &impl ConnectionTrait, id: i32) -> DomainResult<shop::Model> {
    shop::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Shop {id} not found")))
}

pub async fn create(db: &impl ConnectionTrait, input: NewShop) -> DomainResult<shop::Model> {
    let model = shop::ActiveModel {
        name: Set(input.name),
        address: Set(input.address),
        phone: Set(input.phone),
        website: Set(input.website),
        specialty: Set(input.specialty),
        notes: Set(input.notes),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

pub async fn update(
    db: &impl ConnectionTrait,
    id: i32,
    input: UpdateShop,
) -> DomainResult<shop::Model> {
    let existing = get(db, id).await?;
    let mut active: shop::ActiveModel = existing.into();

    if let Some(v) = input.name {
        active.name = Set(v);
    }
    if let Some(v) = input.address {
        active.address = Set(v);
    }
    if let Some(v) = input.phone {
        active.phone = Set(v);
    }
    if let Some(v) = input.website {
        active.website = Set(v);
    }
    if let Some(v) = input.specialty {
        active.specialty = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }

    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
    Ok(active.update(db).await?)
}

pub async fn delete(db: &impl ConnectionTrait, id: i32) -> DomainResult<u64> {
    let result = shop::Entity::delete_by_id(id).exec(db).await?;
    Ok(result.rows_affected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_db;

    #[tokio::test]
    async fn create_then_get_round_trips() {
        let db = test_db().await;
        let created = create(
            &db,
            NewShop {
                name: "Joe's Garage".into(),
                address: Some("123 Main St".into()),
                phone: None,
                website: None,
                specialty: None,
                notes: None,
            },
        )
        .await
        .unwrap();
        let fetched = get(&db, created.id).await.unwrap();
        assert_eq!(fetched.name, "Joe's Garage");
        assert_eq!(fetched.address.as_deref(), Some("123 Main St"));
    }

    #[tokio::test]
    async fn update_sets_name_and_clears_notes() {
        let db = test_db().await;
        let s = create(
            &db,
            NewShop {
                name: "A".into(),
                address: None,
                phone: None,
                website: None,
                specialty: None,
                notes: Some("x".into()),
            },
        )
        .await
        .unwrap();
        let updated = update(
            &db,
            s.id,
            UpdateShop {
                name: Some("B".into()),
                notes: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.name, "B");
        assert_eq!(updated.notes, None);
    }

    #[tokio::test]
    async fn get_missing_is_not_found() {
        let db = test_db().await;
        assert!(matches!(
            get(&db, 999).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }
}
