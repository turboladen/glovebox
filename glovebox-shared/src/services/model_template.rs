use sea_orm::*;

use crate::{entities::model_template, error::DomainResult};

pub async fn list(db: &impl ConnectionTrait) -> DomainResult<Vec<model_template::Model>> {
    Ok(model_template::Entity::find().all(db).await?)
}

/// A model template together with the name of its platform.
#[derive(serde::Serialize)]
pub struct ModelTemplateWithPlatform {
    #[serde(flatten)]
    pub template: model_template::Model,
    pub platform_name: Option<String>,
}

/// Every model template with its platform name, for choosing a template when
/// adding a vehicle. Loads the platforms in one query.
pub async fn list_with_platform(
    db: &impl ConnectionTrait,
) -> DomainResult<Vec<ModelTemplateWithPlatform>> {
    let templates = list(db).await?;
    let platforms: std::collections::HashMap<i32, String> =
        crate::entities::platform::Entity::find()
            .all(db)
            .await?
            .into_iter()
            .map(|p| (p.id, p.name))
            .collect();
    Ok(templates
        .into_iter()
        .map(|template| ModelTemplateWithPlatform {
            platform_name: template
                .platform_id
                .and_then(|id| platforms.get(&id).cloned()),
            template,
        })
        .collect())
}
