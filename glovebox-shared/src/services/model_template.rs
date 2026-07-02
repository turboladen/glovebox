use sea_orm::*;

use crate::{
    entities::model_template,
    error::{DomainError, DomainResult},
    inputs::model_template::{NewModelTemplate, UpdateModelTemplate},
};

pub async fn list(db: &impl ConnectionTrait) -> DomainResult<Vec<model_template::Model>> {
    Ok(model_template::Entity::find().all(db).await?)
}

pub async fn get(db: &impl ConnectionTrait, id: i32) -> DomainResult<model_template::Model> {
    model_template::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Model template {id} not found")))
}

pub async fn create(
    db: &impl ConnectionTrait,
    input: NewModelTemplate,
) -> DomainResult<model_template::Model> {
    let model = model_template::ActiveModel {
        platform_id: Set(input.platform_id),
        platform_ref: Set(input.platform_ref),
        year: Set(input.year),
        make: Set(input.make),
        model: Set(input.model),
        trim_level: Set(input.trim_level),
        body_style: Set(input.body_style),
        engine: Set(input.engine),
        transmission: Set(input.transmission),
        drivetrain: Set(input.drivetrain),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

pub async fn update(
    db: &impl ConnectionTrait,
    id: i32,
    input: UpdateModelTemplate,
) -> DomainResult<model_template::Model> {
    let existing = get(db, id).await?;
    let mut active: model_template::ActiveModel = existing.into();

    if let Some(v) = input.platform_id {
        active.platform_id = Set(v);
    }
    if let Some(v) = input.platform_ref {
        active.platform_ref = Set(v);
    }
    if let Some(v) = input.year {
        active.year = Set(v);
    }
    if let Some(v) = input.make {
        active.make = Set(v);
    }
    if let Some(v) = input.model {
        active.model = Set(v);
    }
    if let Some(v) = input.trim_level {
        active.trim_level = Set(v);
    }
    if let Some(v) = input.body_style {
        active.body_style = Set(v);
    }
    if let Some(v) = input.engine {
        active.engine = Set(v);
    }
    if let Some(v) = input.transmission {
        active.transmission = Set(v);
    }
    if let Some(v) = input.drivetrain {
        active.drivetrain = Set(v);
    }

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
            NewModelTemplate {
                platform_id: None,
                platform_ref: None,
                year: Some(2015),
                make: Some("Subaru".into()),
                model: Some("WRX".into()),
                trim_level: None,
                body_style: None,
                engine: None,
                transmission: None,
                drivetrain: None,
            },
        )
        .await
        .unwrap();
        let fetched = get(&db, created.id).await.unwrap();
        assert_eq!(fetched.make.as_deref(), Some("Subaru"));
        assert_eq!(fetched.year, Some(2015));
    }

    #[tokio::test]
    async fn update_sets_make_and_clears_model() {
        let db = test_db().await;
        let mt = create(
            &db,
            NewModelTemplate {
                platform_id: None,
                platform_ref: None,
                year: None,
                make: Some("A".into()),
                model: Some("X".into()),
                trim_level: None,
                body_style: None,
                engine: None,
                transmission: None,
                drivetrain: None,
            },
        )
        .await
        .unwrap();
        let updated = update(
            &db,
            mt.id,
            UpdateModelTemplate {
                make: Some(Some("B".into())),
                model: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.make.as_deref(), Some("B"));
        assert_eq!(updated.model, None);
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
