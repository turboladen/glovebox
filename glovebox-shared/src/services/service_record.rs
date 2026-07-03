use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{
        maintenance_schedule_item, mileage_log, model_template, part, service_record,
        service_record_line_item, service_schedule_link,
    },
    error::{DomainError, DomainResult},
    inputs::service_record::{NewLineItem, NewServiceRecord, UpdateServiceRecord},
};

/// Verify each schedule item is within the vehicle's schedule scope: owned by
/// the vehicle itself, by its model template, or by that template's platform.
/// Anything else must be indistinguishable from a nonexistent item.
async fn require_schedule_items_in_scope(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    schedule_item_ids: &[i32],
) -> DomainResult<()> {
    if schedule_item_ids.is_empty() {
        return Ok(());
    }

    let v = crate::services::vehicle::require(db, vehicle_id).await?;
    let template = match v.model_template_id {
        Some(mt_id) => model_template::Entity::find_by_id(mt_id).one(db).await?,
        None => None,
    };

    let mut owner =
        Condition::any().add(maintenance_schedule_item::Column::VehicleId.eq(vehicle_id));
    if let Some(mt) = &template {
        owner = owner.add(maintenance_schedule_item::Column::ModelTemplateId.eq(mt.id));
        if let Some(platform_id) = mt.platform_id {
            owner = owner.add(maintenance_schedule_item::Column::PlatformId.eq(platform_id));
        }
    }

    let in_scope: std::collections::HashSet<i32> = maintenance_schedule_item::Entity::find()
        .filter(maintenance_schedule_item::Column::Id.is_in(schedule_item_ids.to_vec()))
        .filter(owner)
        .all(db)
        .await?
        .into_iter()
        .map(|i| i.id)
        .collect();

    if let Some(missing) = schedule_item_ids.iter().find(|id| !in_scope.contains(id)) {
        return Err(DomainError::NotFound(format!(
            "Schedule item {missing} not found"
        )));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct ServiceRecordWithLinks {
    #[serde(flatten)]
    pub record: service_record::Model,
    pub schedule_item_ids: Vec<i32>,
    pub part_ids: Vec<i32>,
    pub line_items: Vec<service_record_line_item::Model>,
}

/// Load schedule link IDs, part IDs, and line items for a single service record.
async fn load_service_links(
    db: &impl ConnectionTrait,
    record_id: i32,
) -> DomainResult<(Vec<i32>, Vec<i32>, Vec<service_record_line_item::Model>)> {
    let links = service_schedule_link::Entity::find()
        .filter(service_schedule_link::Column::ServiceRecordId.eq(record_id))
        .all(db)
        .await?;
    let schedule_item_ids = links.into_iter().map(|l| l.schedule_item_id).collect();

    let parts = part::Entity::find()
        .filter(part::Column::InstalledServiceId.eq(record_id))
        .all(db)
        .await?;
    let part_ids = parts.into_iter().map(|p| p.id).collect();

    let line_items = service_record_line_item::Entity::find()
        .filter(service_record_line_item::Column::ServiceRecordId.eq(record_id))
        .order_by_asc(service_record_line_item::Column::Id)
        .all(db)
        .await?;

    Ok((schedule_item_ids, part_ids, line_items))
}

/// Insert line items for a service record within a transaction.
async fn insert_line_items(
    txn: &impl ConnectionTrait,
    record_id: i32,
    items: Vec<NewLineItem>,
) -> DomainResult<()> {
    for item in items {
        let li = service_record_line_item::ActiveModel {
            service_record_id: Set(record_id),
            description: Set(item.description),
            category: Set(item.category),
            quantity: Set(item.quantity),
            unit_cost_cents: Set(item.unit_cost_cents),
            cost_cents: Set(item.cost_cents),
            ..Default::default()
        };
        li.insert(txn).await?;
    }
    Ok(())
}

pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<Vec<ServiceRecordWithLinks>> {
    let records = service_record::Entity::find()
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(service_record::Column::ServiceDate)
        .all(db)
        .await?;

    // Batch-load all schedule links, parts, and line items for these records (avoids N+1)
    let record_ids: Vec<i32> = records.iter().map(|r| r.id).collect();

    let all_links = if record_ids.is_empty() {
        vec![]
    } else {
        service_schedule_link::Entity::find()
            .filter(service_schedule_link::Column::ServiceRecordId.is_in(record_ids.clone()))
            .all(db)
            .await?
    };

    let all_parts = if record_ids.is_empty() {
        vec![]
    } else {
        part::Entity::find()
            .filter(part::Column::InstalledServiceId.is_in(record_ids.clone()))
            .all(db)
            .await?
    };

    let all_line_items = if record_ids.is_empty() {
        vec![]
    } else {
        service_record_line_item::Entity::find()
            .filter(service_record_line_item::Column::ServiceRecordId.is_in(record_ids))
            .order_by_asc(service_record_line_item::Column::Id)
            .all(db)
            .await?
    };

    let results = records
        .into_iter()
        .map(|record| {
            let schedule_item_ids = all_links
                .iter()
                .filter(|l| l.service_record_id == record.id)
                .map(|l| l.schedule_item_id)
                .collect();
            let part_ids = all_parts
                .iter()
                .filter(|p| p.installed_service_id == Some(record.id))
                .map(|p| p.id)
                .collect();
            let line_items = all_line_items
                .iter()
                .filter(|li| li.service_record_id == record.id)
                .cloned()
                .collect();
            ServiceRecordWithLinks {
                record,
                schedule_item_ids,
                part_ids,
                line_items,
            }
        })
        .collect();

    Ok(results)
}

pub async fn get(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<ServiceRecordWithLinks> {
    let record = service_record::Entity::find_by_id(id)
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Service record {id} not found")))?;

    let (schedule_item_ids, part_ids, line_items) = load_service_links(db, record.id).await?;

    Ok(ServiceRecordWithLinks {
        record,
        schedule_item_ids,
        part_ids,
        line_items,
    })
}

pub async fn create<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    input: NewServiceRecord,
) -> DomainResult<ServiceRecordWithLinks> {
    let schedule_item_ids = input.schedule_item_ids.unwrap_or_default();
    require_schedule_items_in_scope(db, vehicle_id, &schedule_item_ids).await?;
    if let Some(build_id) = input.build_id {
        crate::services::build::require_owned(db, vehicle_id, build_id).await?;
    }

    let txn = db.begin().await?;

    let record = service_record::ActiveModel {
        vehicle_id: Set(vehicle_id),
        service_date: Set(input.service_date.clone()),
        mileage: Set(input.mileage),
        description: Set(input.description),
        parts_cost_cents: Set(input.parts_cost_cents),
        parts_cost_currency: Set(input.parts_cost_currency),
        labor_cost_cents: Set(input.labor_cost_cents),
        labor_cost_currency: Set(input.labor_cost_currency),
        total_cost_cents: Set(input.total_cost_cents),
        total_cost_currency: Set(input.total_cost_currency),
        shop_name: Set(input.shop_name),
        shop_id: Set(input.shop_id),
        notes: Set(input.notes),
        build_id: Set(input.build_id),
        ..Default::default()
    };
    let record = record.insert(&txn).await?;

    for item_id in &schedule_item_ids {
        let link = service_schedule_link::ActiveModel {
            service_record_id: Set(record.id),
            schedule_item_id: Set(*item_id),
        };
        link.insert(&txn).await?;
    }

    // Mark linked parts as installed
    let part_ids = input.part_ids.unwrap_or_default();
    for part_id in &part_ids {
        let existing_part = part::Entity::find_by_id(*part_id)
            .filter(part::Column::VehicleId.eq(vehicle_id))
            .one(&txn)
            .await?
            .ok_or_else(|| DomainError::NotFound(format!("Part {part_id} not found")))?;

        let mut active_part: part::ActiveModel = existing_part.into();
        active_part.status = Set("installed".to_string());
        active_part.installed_service_id = Set(Some(record.id));
        active_part.installed_date = Set(Some(input.service_date.clone()));
        if let Some(miles) = record.mileage {
            active_part.installed_odometer = Set(Some(miles));
        }
        active_part.update(&txn).await?;
    }

    // Insert line items
    let line_items_input = input.line_items.unwrap_or_default();
    insert_line_items(&txn, record.id, line_items_input).await?;

    // Also create a mileage log entry if mileage was provided
    if let Some(miles) = record.mileage {
        let mileage_entry = mileage_log::ActiveModel {
            vehicle_id: Set(vehicle_id),
            mileage: Set(miles),
            recorded_at: Set(record.service_date.clone()),
            source: Set(Some("service".to_string())),
            notes: Set(None),
            ..Default::default()
        };
        mileage_entry.insert(&txn).await?;
    }

    // Load back the inserted line items to return
    let line_items = service_record_line_item::Entity::find()
        .filter(service_record_line_item::Column::ServiceRecordId.eq(record.id))
        .order_by_asc(service_record_line_item::Column::Id)
        .all(&txn)
        .await?;

    txn.commit().await?;

    Ok(ServiceRecordWithLinks {
        record,
        schedule_item_ids,
        part_ids,
        line_items,
    })
}

#[allow(clippy::too_many_lines)]
pub async fn update<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    input: UpdateServiceRecord,
) -> DomainResult<ServiceRecordWithLinks> {
    let existing = service_record::Entity::find_by_id(id)
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Service record {id} not found")))?;

    let txn = db.begin().await?;

    // A linked build must belong to the same vehicle; a cross-vehicle build
    // must be indistinguishable from a nonexistent one. Checked inside the txn
    // (like the schedule-items guard) so a concurrent build::delete can't slip
    // between guard and write.
    if let Some(Some(build_id)) = input.build_id {
        crate::services::build::require_owned(&txn, vehicle_id, build_id).await?;
    }

    let mut active: service_record::ActiveModel = existing.into();

    if let Some(v) = input.service_date {
        active.service_date = Set(v);
    }
    if let Some(v) = input.mileage {
        active.mileage = Set(v);
    }
    if let Some(v) = input.description {
        active.description = Set(v);
    }
    if let Some(v) = input.parts_cost_cents {
        active.parts_cost_cents = Set(v);
    }
    if let Some(v) = input.parts_cost_currency {
        active.parts_cost_currency = Set(v);
    }
    if let Some(v) = input.labor_cost_cents {
        active.labor_cost_cents = Set(v);
    }
    if let Some(v) = input.labor_cost_currency {
        active.labor_cost_currency = Set(v);
    }
    if let Some(v) = input.total_cost_cents {
        active.total_cost_cents = Set(v);
    }
    if let Some(v) = input.total_cost_currency {
        active.total_cost_currency = Set(v);
    }
    if let Some(v) = input.shop_name {
        active.shop_name = Set(v);
    }
    if let Some(v) = input.shop_id {
        active.shop_id = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }
    if let Some(v) = input.build_id {
        active.build_id = Set(v);
    }

    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

    let record = active.update(&txn).await?;

    let schedule_item_ids = if let Some(item_ids) = input.schedule_item_ids {
        require_schedule_items_in_scope(&txn, vehicle_id, &item_ids).await?;
        service_schedule_link::Entity::delete_many()
            .filter(service_schedule_link::Column::ServiceRecordId.eq(record.id))
            .exec(&txn)
            .await?;

        for item_id in &item_ids {
            let link = service_schedule_link::ActiveModel {
                service_record_id: Set(record.id),
                schedule_item_id: Set(*item_id),
            };
            link.insert(&txn).await?;
        }
        item_ids
    } else {
        let links = service_schedule_link::Entity::find()
            .filter(service_schedule_link::Column::ServiceRecordId.eq(record.id))
            .all(&txn)
            .await?;
        links.into_iter().map(|l| l.schedule_item_id).collect()
    };

    // Handle part linking on update
    let part_ids = if let Some(new_part_ids) = input.part_ids {
        // Unlink previously linked parts
        let old_parts = part::Entity::find()
            .filter(part::Column::InstalledServiceId.eq(record.id))
            .all(&txn)
            .await?;
        for old_part in old_parts {
            if !new_part_ids.contains(&old_part.id) {
                let mut active_part: part::ActiveModel = old_part.clone().into();
                // Only revert to purchased if the part was installed (not replaced/returned)
                if old_part.status == "installed" {
                    active_part.status = Set("purchased".to_string());
                }
                active_part.installed_service_id = Set(None);
                active_part.installed_date = Set(None);
                active_part.installed_odometer = Set(None);
                active_part.update(&txn).await?;
            }
        }
        // Link new parts
        for part_id in &new_part_ids {
            let existing_part = part::Entity::find_by_id(*part_id)
                .filter(part::Column::VehicleId.eq(vehicle_id))
                .one(&txn)
                .await?
                .ok_or_else(|| DomainError::NotFound(format!("Part {part_id} not found")))?;

            let mut active_part: part::ActiveModel = existing_part.into();
            active_part.status = Set("installed".to_string());
            active_part.installed_service_id = Set(Some(record.id));
            active_part.installed_date = Set(Some(record.service_date.clone()));
            active_part.installed_odometer = Set(record.mileage);
            active_part.update(&txn).await?;
        }
        new_part_ids
    } else {
        let parts = part::Entity::find()
            .filter(part::Column::InstalledServiceId.eq(record.id))
            .all(&txn)
            .await?;
        parts.into_iter().map(|p| p.id).collect()
    };

    // Handle line items: replace-all semantics when provided
    if let Some(new_line_items) = input.line_items {
        service_record_line_item::Entity::delete_many()
            .filter(service_record_line_item::Column::ServiceRecordId.eq(record.id))
            .exec(&txn)
            .await?;
        insert_line_items(&txn, record.id, new_line_items).await?;
    }

    let line_items = service_record_line_item::Entity::find()
        .filter(service_record_line_item::Column::ServiceRecordId.eq(record.id))
        .order_by_asc(service_record_line_item::Column::Id)
        .all(&txn)
        .await?;

    txn.commit().await?;

    Ok(ServiceRecordWithLinks {
        record,
        schedule_item_ids,
        part_ids,
        line_items,
    })
}

pub async fn delete<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<()> {
    let existing = service_record::Entity::find_by_id(id)
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Service record {id} not found")))?;

    let txn = db.begin().await?;

    // Unlink parts installed during this service (revert to purchased)
    let linked_parts = part::Entity::find()
        .filter(part::Column::InstalledServiceId.eq(existing.id))
        .all(&txn)
        .await?;
    for p in linked_parts {
        let mut active_part: part::ActiveModel = p.into();
        active_part.status = Set("purchased".to_string());
        active_part.installed_service_id = Set(None);
        active_part.installed_date = Set(None);
        active_part.installed_odometer = Set(None);
        active_part.update(&txn).await?;
    }

    // Remove schedule links and line items
    service_schedule_link::Entity::delete_many()
        .filter(service_schedule_link::Column::ServiceRecordId.eq(existing.id))
        .exec(&txn)
        .await?;
    service_record_line_item::Entity::delete_many()
        .filter(service_record_line_item::Column::ServiceRecordId.eq(existing.id))
        .exec(&txn)
        .await?;

    // Delete the service record
    existing.delete(&txn).await?;

    txn.commit().await?;

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

    async fn seed_part(db: &impl ConnectionTrait, vehicle_id: i32, name: &str) -> i32 {
        part::ActiveModel {
            vehicle_id: Set(vehicle_id),
            name: Set(name.into()),
            status: Set("purchased".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn seed_schedule_item(db: &impl ConnectionTrait, vehicle_id: i32, name: &str) -> i32 {
        use crate::entities::maintenance_schedule_item;
        maintenance_schedule_item::ActiveModel {
            vehicle_id: Set(Some(vehicle_id)),
            name: Set(name.into()),
            enabled: Set(true),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn create_wires_line_items_schedule_links_and_parts() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let part_id = seed_part(&db, vid, "Oil filter").await;
        let sched_id = seed_schedule_item(&db, vid, "Oil change").await;

        let created = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: "2024-03-01".into(),
                mileage: Some(50_000),
                description: Some("Oil change".into()),
                parts_cost_cents: None,
                parts_cost_currency: None,
                labor_cost_cents: None,
                labor_cost_currency: None,
                total_cost_cents: Some(6_000),
                total_cost_currency: None,
                shop_name: None,
                shop_id: None,
                notes: None,
                build_id: None,
                schedule_item_ids: Some(vec![sched_id]),
                part_ids: Some(vec![part_id]),
                line_items: Some(vec![NewLineItem {
                    description: "Synthetic oil".into(),
                    category: Some("parts".into()),
                    quantity: Some(5.0),
                    unit_cost_cents: Some(1_000),
                    cost_cents: Some(5_000),
                }]),
            },
        )
        .await
        .unwrap();

        // The record links back to schedule item, part, and line item
        assert_eq!(created.schedule_item_ids, vec![sched_id]);
        assert_eq!(created.part_ids, vec![part_id]);
        assert_eq!(created.line_items.len(), 1);
        assert_eq!(created.line_items[0].description, "Synthetic oil");

        // The linked part was marked installed and wired to this service
        let p = part::Entity::find_by_id(part_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(p.status, "installed");
        assert_eq!(p.installed_service_id, Some(created.record.id));
        assert_eq!(p.installed_odometer, Some(50_000));

        // A mileage log entry was created for the provided mileage
        let logs = mileage_log::Entity::find()
            .filter(mileage_log::Column::VehicleId.eq(vid))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].mileage, 50_000);

        // get() reads back the same wiring
        let fetched = get(&db, vid, created.record.id).await.unwrap();
        assert_eq!(fetched.schedule_item_ids, vec![sched_id]);
        assert_eq!(fetched.part_ids, vec![part_id]);
        assert_eq!(fetched.line_items.len(), 1);
    }

    #[tokio::test]
    async fn update_replaces_line_items_and_relinks_parts() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let part_a = seed_part(&db, vid, "Part A").await;
        let part_b = seed_part(&db, vid, "Part B").await;

        let created = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: "2024-03-01".into(),
                mileage: Some(1_000),
                description: None,
                parts_cost_cents: None,
                parts_cost_currency: None,
                labor_cost_cents: None,
                labor_cost_currency: None,
                total_cost_cents: None,
                total_cost_currency: None,
                shop_name: None,
                shop_id: None,
                notes: None,
                build_id: None,
                schedule_item_ids: None,
                part_ids: Some(vec![part_a]),
                line_items: Some(vec![NewLineItem {
                    description: "old".into(),
                    category: None,
                    quantity: None,
                    unit_cost_cents: None,
                    cost_cents: Some(100),
                }]),
            },
        )
        .await
        .unwrap();

        let updated = update(
            &db,
            vid,
            created.record.id,
            UpdateServiceRecord {
                part_ids: Some(vec![part_b]),
                line_items: Some(vec![NewLineItem {
                    description: "new".into(),
                    category: None,
                    quantity: None,
                    unit_cost_cents: None,
                    cost_cents: Some(200),
                }]),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Line items replaced
        assert_eq!(updated.line_items.len(), 1);
        assert_eq!(updated.line_items[0].description, "new");
        // Parts relinked: A unlinked (reverted to purchased), B installed
        assert_eq!(updated.part_ids, vec![part_b]);
        let a = part::Entity::find_by_id(part_a)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(a.status, "purchased");
        assert_eq!(a.installed_service_id, None);
        let b = part::Entity::find_by_id(part_b)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(b.status, "installed");
        assert_eq!(b.installed_service_id, Some(created.record.id));
    }

    #[tokio::test]
    async fn delete_unlinks_parts_and_removes_links() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let part_id = seed_part(&db, vid, "Part").await;
        let sched_id = seed_schedule_item(&db, vid, "Item").await;

        let created = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: "2024-03-01".into(),
                mileage: None,
                description: None,
                parts_cost_cents: None,
                parts_cost_currency: None,
                labor_cost_cents: None,
                labor_cost_currency: None,
                total_cost_cents: None,
                total_cost_currency: None,
                shop_name: None,
                shop_id: None,
                notes: None,
                build_id: None,
                schedule_item_ids: Some(vec![sched_id]),
                part_ids: Some(vec![part_id]),
                line_items: None,
            },
        )
        .await
        .unwrap();

        delete(&db, vid, created.record.id).await.unwrap();

        // Record gone
        assert!(matches!(
            get(&db, vid, created.record.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        // Part reverted
        let p = part::Entity::find_by_id(part_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(p.status, "purchased");
        assert_eq!(p.installed_service_id, None);
        // Schedule links removed
        let links = service_schedule_link::Entity::find()
            .all(&db)
            .await
            .unwrap();
        assert!(links.is_empty());
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

    fn minimal_record(schedule_item_ids: Option<Vec<i32>>) -> NewServiceRecord {
        NewServiceRecord {
            service_date: "2024-03-01".into(),
            mileage: None,
            description: None,
            parts_cost_cents: None,
            parts_cost_currency: None,
            labor_cost_cents: None,
            labor_cost_currency: None,
            total_cost_cents: None,
            total_cost_currency: None,
            shop_name: None,
            shop_id: None,
            notes: None,
            build_id: None,
            schedule_item_ids,
            part_ids: None,
            line_items: None,
        }
    }

    #[tokio::test]
    async fn create_rejects_other_vehicles_schedule_items() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign_item = seed_schedule_item(&db, other, "Foreign").await;

        // Linking another vehicle's schedule item must 404 and create nothing.
        assert!(matches!(
            create(&db, owner, minimal_record(Some(vec![foreign_item])))
                .await
                .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(list(&db, owner).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn update_rejects_other_vehicles_schedule_items() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let own_item = seed_schedule_item(&db, owner, "Mine").await;
        let foreign_item = seed_schedule_item(&db, other, "Foreign").await;
        let created = create(&db, owner, minimal_record(Some(vec![own_item])))
            .await
            .unwrap();

        assert!(matches!(
            update(
                &db,
                owner,
                created.record.id,
                UpdateServiceRecord {
                    schedule_item_ids: Some(vec![foreign_item]),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        // Existing links survive the rejected update (transaction rolled back).
        let fetched = get(&db, owner, created.record.id).await.unwrap();
        assert_eq!(fetched.schedule_item_ids, vec![own_item]);
    }

    #[tokio::test]
    async fn create_allows_inherited_template_and_platform_schedule_items() {
        use crate::entities::{model_template, platform, vehicle};
        let db = test_db().await;

        let platform_id = platform::ActiveModel {
            name: Set("MQB".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        let template_id = model_template::ActiveModel {
            platform_id: Set(Some(platform_id)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        let vid = vehicle::ActiveModel {
            name: Set("Car".into()),
            model_template_id: Set(Some(template_id)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;

        let platform_item = maintenance_schedule_item::ActiveModel {
            platform_id: Set(Some(platform_id)),
            name: Set("Platform oil change".into()),
            enabled: Set(true),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        let template_item = maintenance_schedule_item::ActiveModel {
            model_template_id: Set(Some(template_id)),
            name: Set("Template inspection".into()),
            enabled: Set(true),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;

        // Items inherited via the vehicle's template/platform chain are legit link targets.
        let created = create(
            &db,
            vid,
            minimal_record(Some(vec![platform_item, template_item])),
        )
        .await
        .unwrap();
        assert_eq!(
            created.schedule_item_ids,
            vec![platform_item, template_item]
        );
    }
}
