use sea_orm::*;

use crate::{
    entities::platform,
    error::{DomainError, DomainResult},
    inputs::platform::{NewPlatform, UpdatePlatform},
};

pub async fn list(db: &impl ConnectionTrait) -> DomainResult<Vec<platform::Model>> {
    Ok(platform::Entity::find().all(db).await?)
}

pub async fn get(db: &impl ConnectionTrait, id: i32) -> DomainResult<platform::Model> {
    platform::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Platform {id} not found")))
}

pub async fn create(
    db: &impl ConnectionTrait,
    input: NewPlatform,
) -> DomainResult<platform::Model> {
    if input.name.trim().is_empty() {
        return Err(DomainError::invalid("name", "must not be blank"));
    }
    let model = platform::ActiveModel {
        name: Set(input.name),
        website_url: Set(input.website_url),
        api_base_url: Set(input.api_base_url),
        notes: Set(input.notes),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

pub async fn update(
    db: &impl ConnectionTrait,
    id: i32,
    input: UpdatePlatform,
) -> DomainResult<platform::Model> {
    let existing = get(db, id).await?;
    let mut active: platform::ActiveModel = existing.into();
    if let Some(v) = input.name {
        if v.trim().is_empty() {
            return Err(DomainError::invalid("name", "must not be blank"));
        }
        active.name = Set(v);
    }
    if let Some(v) = input.website_url {
        active.website_url = Set(v);
    }
    if let Some(v) = input.api_base_url {
        active.api_base_url = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }
    // SeaORM ActiveModel::update() does not auto-set updated_at, and the DB
    // default only fires on INSERT — stamp it explicitly (CLAUDE.md convention).
    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
    Ok(active.update(db).await?)
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
            NewPlatform {
                name: "RockAuto".into(),
                website_url: Some("https://rockauto.com".into()),
                api_base_url: None,
                notes: None,
            },
        )
        .await
        .unwrap();
        let fetched = get(&db, created.id).await.unwrap();
        assert_eq!(fetched.name, "RockAuto");
        assert_eq!(fetched.website_url.as_deref(), Some("https://rockauto.com"));
    }

    #[tokio::test]
    async fn create_rejects_blank_name() {
        let db = test_db().await;
        let err = create(
            &db,
            NewPlatform {
                name: "   ".into(),
                website_url: None,
                api_base_url: None,
                notes: None,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { .. }));
    }

    #[tokio::test]
    async fn update_sets_name_and_clears_notes() {
        let db = test_db().await;
        let p = create(
            &db,
            NewPlatform {
                name: "A".into(),
                website_url: None,
                api_base_url: None,
                notes: Some("x".into()),
            },
        )
        .await
        .unwrap();
        let updated = update(
            &db,
            p.id,
            UpdatePlatform {
                name: Some("B".into()),
                notes: Some(None), // explicit null clears it
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
