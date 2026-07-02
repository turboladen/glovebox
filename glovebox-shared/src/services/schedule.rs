use std::collections::HashMap;

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{maintenance_schedule_item, model_template, vehicle},
    error::{DomainError, DomainResult},
    inputs::schedule::{NewScheduleItem, ScheduleFilter, UpdateScheduleItem},
};

#[derive(Serialize)]
pub struct ResolvedScheduleItem {
    pub effective_item: maintenance_schedule_item::Model,
    pub inherited_from: Option<String>,
}

pub async fn list(
    db: &impl ConnectionTrait,
    filter: ScheduleFilter,
) -> DomainResult<Vec<maintenance_schedule_item::Model>> {
    let mut select = maintenance_schedule_item::Entity::find();

    if let Some(pid) = filter.platform_id {
        select = select.filter(maintenance_schedule_item::Column::PlatformId.eq(pid));
    }
    if let Some(mtid) = filter.model_template_id {
        select = select.filter(maintenance_schedule_item::Column::ModelTemplateId.eq(mtid));
    }
    if let Some(vid) = filter.vehicle_id {
        select = select.filter(maintenance_schedule_item::Column::VehicleId.eq(vid));
    }

    Ok(select.all(db).await?)
}

pub async fn get(
    db: &impl ConnectionTrait,
    id: i32,
) -> DomainResult<maintenance_schedule_item::Model> {
    maintenance_schedule_item::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Schedule item {id} not found")))
}

pub async fn create(
    db: &impl ConnectionTrait,
    input: NewScheduleItem,
) -> DomainResult<maintenance_schedule_item::Model> {
    // Validate exactly one owner is set
    let owner_count = [
        input.platform_id.is_some(),
        input.model_template_id.is_some(),
        input.vehicle_id.is_some(),
    ]
    .iter()
    .filter(|&&b| b)
    .count();

    if owner_count != 1 {
        return Err(DomainError::invalid(
            "owner",
            "Exactly one of platform_id, model_template_id, or vehicle_id must be set",
        ));
    }

    let model = maintenance_schedule_item::ActiveModel {
        platform_id: Set(input.platform_id),
        model_template_id: Set(input.model_template_id),
        vehicle_id: Set(input.vehicle_id),
        overrides_item_id: Set(input.overrides_item_id),
        name: Set(input.name),
        description: Set(input.description),
        interval_miles: Set(input.interval_miles),
        interval_months: Set(input.interval_months),
        warning_miles: Set(input.warning_miles),
        warning_days: Set(input.warning_days),
        enabled: Set(input.enabled.unwrap_or(true)),
        source: Set(input.source),
        notes: Set(input.notes),
        is_factory_recommended: Set(input.is_factory_recommended),
        labor_categories: Set(input.labor_categories),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

pub async fn update(
    db: &impl ConnectionTrait,
    id: i32,
    input: UpdateScheduleItem,
) -> DomainResult<maintenance_schedule_item::Model> {
    let existing = get(db, id).await?;
    let mut active: maintenance_schedule_item::ActiveModel = existing.into();

    if let Some(v) = input.name {
        active.name = Set(v);
    }
    if let Some(v) = input.description {
        active.description = Set(v);
    }
    if let Some(v) = input.interval_miles {
        active.interval_miles = Set(v);
    }
    if let Some(v) = input.interval_months {
        active.interval_months = Set(v);
    }
    if let Some(v) = input.warning_miles {
        active.warning_miles = Set(v);
    }
    if let Some(v) = input.warning_days {
        active.warning_days = Set(v);
    }
    if let Some(v) = input.enabled {
        active.enabled = Set(v);
    }
    if let Some(v) = input.source {
        active.source = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }
    if let Some(v) = input.is_factory_recommended {
        active.is_factory_recommended = Set(v);
    }
    if let Some(v) = input.labor_categories {
        active.labor_categories = Set(v);
    }

    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
    Ok(active.update(db).await?)
}

pub async fn delete(db: &impl ConnectionTrait, id: i32) -> DomainResult<()> {
    let result = maintenance_schedule_item::Entity::delete_by_id(id)
        .exec(db)
        .await?;
    if result.rows_affected == 0 {
        return Err(DomainError::NotFound(format!("Schedule item {id} not found")));
    }
    Ok(())
}

/// Resolve the effective maintenance schedule for a vehicle.
/// Implements the 3-level inheritance: Platform → Model Template → Vehicle.
pub async fn resolve(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<Vec<ResolvedScheduleItem>> {
    let v = vehicle::Entity::find_by_id(vehicle_id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Vehicle {vehicle_id} not found")))?;

    let mut schedule: HashMap<String, ResolvedScheduleItem> = HashMap::new();

    if let Some(mt_id) = v.model_template_id {
        let mt = model_template::Entity::find_by_id(mt_id).one(db).await?;

        if let Some(mt) = &mt {
            // Layer 1: Platform items (via model template → platform)
            if let Some(platform_id) = mt.platform_id {
                let platform_items = maintenance_schedule_item::Entity::find()
                    .filter(maintenance_schedule_item::Column::PlatformId.eq(platform_id))
                    .all(db)
                    .await?;

                for item in platform_items {
                    let name = item.name.clone();
                    schedule.insert(
                        name,
                        ResolvedScheduleItem {
                            effective_item: item,
                            inherited_from: Some("platform".to_string()),
                        },
                    );
                }
            }

            // Layer 2: Model template items override platform items by name
            let template_items = maintenance_schedule_item::Entity::find()
                .filter(maintenance_schedule_item::Column::ModelTemplateId.eq(mt_id))
                .all(db)
                .await?;

            for item in template_items {
                let name = item.name.clone();
                schedule.insert(
                    name,
                    ResolvedScheduleItem {
                        effective_item: item,
                        inherited_from: Some("model_template".to_string()),
                    },
                );
            }
        }
    }

    // Layer 3: Vehicle-level items override everything
    let vehicle_items = maintenance_schedule_item::Entity::find()
        .filter(maintenance_schedule_item::Column::VehicleId.eq(vehicle_id))
        .all(db)
        .await?;

    for item in vehicle_items {
        let name = item.name.clone();
        schedule.insert(
            name,
            ResolvedScheduleItem {
                effective_item: item,
                inherited_from: None,
            },
        );
    }

    // Filter out disabled items and sort by name for stable output
    let mut result: Vec<ResolvedScheduleItem> = schedule
        .into_values()
        .filter(|r| r.effective_item.enabled)
        .collect();
    result.sort_by(|a, b| a.effective_item.name.cmp(&b.effective_item.name));

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_db;

    async fn seed_vehicle(db: &impl ConnectionTrait, model_template_id: Option<i32>) -> i32 {
        vehicle::ActiveModel {
            name: Set("Car".into()),
            model_template_id: Set(model_template_id),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn create_requires_exactly_one_owner() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let err = create(
            &db,
            NewScheduleItem {
                platform_id: Some(1),
                model_template_id: None,
                vehicle_id: Some(vid),
                overrides_item_id: None,
                name: "Oil change".into(),
                description: None,
                interval_miles: Some(5000),
                interval_months: None,
                warning_miles: None,
                warning_days: None,
                enabled: None,
                source: None,
                notes: None,
                is_factory_recommended: None,
                labor_categories: None,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { .. }));
    }

    #[tokio::test]
    async fn create_then_get_and_update() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        let created = create(
            &db,
            NewScheduleItem {
                platform_id: None,
                model_template_id: None,
                vehicle_id: Some(vid),
                overrides_item_id: None,
                name: "Oil change".into(),
                description: None,
                interval_miles: Some(5000),
                interval_months: None,
                warning_miles: None,
                warning_days: None,
                enabled: None,
                source: None,
                notes: None,
                is_factory_recommended: None,
                labor_categories: None,
            },
        )
        .await
        .unwrap();
        // enabled defaults to true
        assert!(created.enabled);
        assert_eq!(get(&db, created.id).await.unwrap().name, "Oil change");

        let updated = update(
            &db,
            created.id,
            UpdateScheduleItem {
                interval_miles: Some(Some(7500)),
                enabled: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.interval_miles, Some(7500));
        assert!(!updated.enabled);
    }

    #[tokio::test]
    async fn resolve_applies_vehicle_layer_and_hides_disabled() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, None).await;
        create(
            &db,
            NewScheduleItem {
                platform_id: None,
                model_template_id: None,
                vehicle_id: Some(vid),
                overrides_item_id: None,
                name: "Tire rotation".into(),
                description: None,
                interval_miles: Some(6000),
                interval_months: None,
                warning_miles: None,
                warning_days: None,
                enabled: Some(true),
                source: None,
                notes: None,
                is_factory_recommended: None,
                labor_categories: None,
            },
        )
        .await
        .unwrap();
        create(
            &db,
            NewScheduleItem {
                platform_id: None,
                model_template_id: None,
                vehicle_id: Some(vid),
                overrides_item_id: None,
                name: "Disabled item".into(),
                description: None,
                interval_miles: None,
                interval_months: None,
                warning_miles: None,
                warning_days: None,
                enabled: Some(false),
                source: None,
                notes: None,
                is_factory_recommended: None,
                labor_categories: None,
            },
        )
        .await
        .unwrap();

        let resolved = resolve(&db, vid).await.unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].effective_item.name, "Tire rotation");
        assert_eq!(resolved[0].inherited_from, None);
    }

    #[tokio::test]
    async fn delete_missing_is_not_found() {
        let db = test_db().await;
        assert!(matches!(
            delete(&db, 999).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }
}
