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

/// Payer whitelist for `service_records.paid_by`.
const VALID_PAYERS: [&str; 3] = ["self", "insurance", "third_party"];

fn validate_payer(paid_by: &str) -> DomainResult<()> {
    if VALID_PAYERS.contains(&paid_by) {
        return Ok(());
    }
    Err(DomainError::BadRequest(format!(
        "Invalid paid_by '{}'. Must be one of: {}",
        paid_by,
        VALID_PAYERS.join(", ")
    )))
}

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
    /// Soft advisory: other same-day records on this vehicle, surfaced only
    /// when a record was created WITHOUT an `invoice_ref` so the caller can
    /// reconcile a possible duplicate. Never blocks — distinct records
    /// coexist. Empty (and omitted from JSON) on every other path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub possible_duplicates: Vec<PossibleDuplicate>,
}

/// A lightweight same-day peer surfaced in `possible_duplicates`.
#[derive(Debug, Serialize)]
pub struct PossibleDuplicate {
    pub id: i32,
    pub service_date: String,
    pub description: Option<String>,
    pub total_cost_cents: Option<i32>,
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

/// Insert the mileage log a service record auto-creates for its odometer
/// reading (`source = "service"`, linked via `service_record_id`).
async fn insert_auto_mileage_log(
    txn: &impl ConnectionTrait,
    vehicle_id: i32,
    record: &service_record::Model,
    miles: i32,
) -> DomainResult<()> {
    let mileage_entry = mileage_log::ActiveModel {
        vehicle_id: Set(vehicle_id),
        mileage: Set(miles),
        recorded_at: Set(record.service_date.clone()),
        source: Set(Some("service".to_string())),
        notes: Set(None),
        service_record_id: Set(Some(record.id)),
        ..Default::default()
    };
    mileage_entry.insert(txn).await?;
    Ok(())
}

/// Reconcile the auto-created mileage log with the record's current state
/// (mirrors what `create` produces): mileage present → the linked log carries
/// that mileage at the service date (created if missing); mileage cleared →
/// the linked log is removed.
async fn sync_auto_mileage_log(
    txn: &impl ConnectionTrait,
    vehicle_id: i32,
    record: &service_record::Model,
) -> DomainResult<()> {
    let existing_log = mileage_log::Entity::find()
        .filter(mileage_log::Column::ServiceRecordId.eq(record.id))
        .one(txn)
        .await?;

    match (record.mileage, existing_log) {
        (Some(miles), Some(log)) => {
            if log.mileage != miles || log.recorded_at != record.service_date {
                let mut active_log: mileage_log::ActiveModel = log.into();
                active_log.mileage = Set(miles);
                active_log.recorded_at = Set(record.service_date.clone());
                active_log.update(txn).await?;
            }
        }
        (Some(miles), None) => {
            insert_auto_mileage_log(txn, vehicle_id, record, miles).await?;
        }
        (None, Some(_)) => {
            mileage_log::Entity::delete_many()
                .filter(mileage_log::Column::ServiceRecordId.eq(record.id))
                .exec(txn)
                .await?;
        }
        (None, None) => {}
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
                possible_duplicates: Vec::new(),
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
        possible_duplicates: Vec::new(),
    })
}

#[allow(clippy::too_many_lines)] // linear create + idempotency + advisory
pub async fn create<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    input: NewServiceRecord,
) -> DomainResult<ServiceRecordWithLinks> {
    if let Some(paid_by) = &input.paid_by {
        validate_payer(paid_by)?;
    }

    // Normalize a blank invoice_ref to absent. An LLM may send "" (or spaces)
    // rather than omitting the field; treating that as a real ref would let the
    // partial unique index reject a SECOND ref-less record as a false
    // duplicate. Trim so "INV-1 " and "INV-1" are the same identity signal.
    let invoice_ref = input
        .invoice_ref
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty());

    // Hard-idempotency: an invoice_ref is a deterministic identity signal.
    // If this vehicle already has a record with the same ref, return it
    // UNTOUCHED — no new record, no new links/parts/line-items from this call
    // (reconcile those via `link` if needed). Short-circuits before any guard
    // or write. The partial unique index is the backstop against a race.
    if let Some(invoice_ref) = &invoice_ref {
        let existing = service_record::Entity::find()
            .filter(service_record::Column::VehicleId.eq(vehicle_id))
            .filter(service_record::Column::InvoiceRef.eq(invoice_ref.as_str()))
            .one(db)
            .await?;
        if let Some(existing) = existing {
            let (schedule_item_ids, part_ids, line_items) =
                load_service_links(db, existing.id).await?;
            return Ok(ServiceRecordWithLinks {
                record: existing,
                schedule_item_ids,
                part_ids,
                line_items,
                possible_duplicates: Vec::new(),
            });
        }
    }

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
        paid_by: Set(input.paid_by.unwrap_or_else(|| "self".into())),
        payer_note: Set(input.payer_note),
        invoice_ref: Set(invoice_ref),
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

    // Also create a mileage log entry if mileage was provided. The FK is the
    // real linkage (activity dedupe + update/delete maintenance key on it);
    // `source` stays as a display label.
    if let Some(miles) = record.mileage {
        insert_auto_mileage_log(&txn, vehicle_id, &record, miles).await?;
    }

    // Load back the inserted line items to return
    let line_items = service_record_line_item::Entity::find()
        .filter(service_record_line_item::Column::ServiceRecordId.eq(record.id))
        .order_by_asc(service_record_line_item::Column::Id)
        .all(&txn)
        .await?;

    txn.commit().await?;

    // Soft advisory: with no invoice_ref there is no deterministic identity
    // signal, so surface other same-day records on this vehicle for the caller
    // to reconcile. Read-only, never blocks — distinct records coexist.
    let possible_duplicates = if record.invoice_ref.is_none() {
        service_record::Entity::find()
            .filter(service_record::Column::VehicleId.eq(vehicle_id))
            .filter(service_record::Column::ServiceDate.eq(record.service_date.clone()))
            .filter(service_record::Column::Id.ne(record.id))
            .order_by_asc(service_record::Column::Id)
            .all(db)
            .await?
            .into_iter()
            .map(|r| PossibleDuplicate {
                id: r.id,
                service_date: r.service_date,
                description: r.description,
                total_cost_cents: r.total_cost_cents,
            })
            .collect()
    } else {
        Vec::new()
    };

    Ok(ServiceRecordWithLinks {
        record,
        schedule_item_ids,
        part_ids,
        line_items,
        possible_duplicates,
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
    if let Some(v) = input.paid_by {
        validate_payer(&v)?;
        active.paid_by = Set(v);
    }
    if let Some(v) = input.payer_note {
        active.payer_note = Set(v);
    }

    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

    let record = active.update(&txn).await?;

    // Keep the auto-created mileage log in step with the record.
    sync_auto_mileage_log(&txn, vehicle_id, &record).await?;

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
        possible_duplicates: Vec::new(),
    })
}

/// How [`link_schedule_items`] treats the record's existing schedule links.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkMode {
    /// Union the given items with the existing links (safe for incremental
    /// reconciliation loops — nothing already linked is clobbered).
    Add,
    /// Overwrite: the given items become the record's only links.
    Replace,
}

impl LinkMode {
    pub fn parse(mode: &str) -> DomainResult<Self> {
        match mode {
            "add" => Ok(Self::Add),
            "replace" => Ok(Self::Replace),
            other => Err(DomainError::BadRequest(format!(
                "Invalid mode '{other}'. Must be one of: add, replace"
            ))),
        }
    }
}

/// Link a service record to maintenance-schedule items without touching the
/// rest of the record. Self-guarded: the record must belong to `vehicle_id`,
/// and every item must be in the vehicle's schedule scope
/// ([`require_schedule_items_in_scope`]) — wrong-parent ids read as missing.
/// Linking is what clears the corresponding reminders.
pub async fn link_schedule_items<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    item_ids: &[i32],
    mode: LinkMode,
) -> DomainResult<ServiceRecordWithLinks> {
    let existing = service_record::Entity::find_by_id(id)
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Service record {id} not found")))?;

    let txn = db.begin().await?;
    require_schedule_items_in_scope(&txn, vehicle_id, item_ids).await?;

    let already_linked: std::collections::HashSet<i32> = match mode {
        LinkMode::Replace => {
            service_schedule_link::Entity::delete_many()
                .filter(service_schedule_link::Column::ServiceRecordId.eq(existing.id))
                .exec(&txn)
                .await?;
            std::collections::HashSet::new()
        }
        LinkMode::Add => service_schedule_link::Entity::find()
            .filter(service_schedule_link::Column::ServiceRecordId.eq(existing.id))
            .all(&txn)
            .await?
            .into_iter()
            .map(|l| l.schedule_item_id)
            .collect(),
    };

    let mut inserted = already_linked;
    for item_id in item_ids {
        if inserted.insert(*item_id) {
            service_schedule_link::ActiveModel {
                service_record_id: Set(existing.id),
                schedule_item_id: Set(*item_id),
            }
            .insert(&txn)
            .await?;
        }
    }

    // Linking changes what the record accounts for — stamp it like update().
    let mut active: service_record::ActiveModel = existing.into();
    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
    let record = active.update(&txn).await?;
    txn.commit().await?;

    let (schedule_item_ids, part_ids, line_items) = load_service_links(db, record.id).await?;
    Ok(ServiceRecordWithLinks {
        record,
        schedule_item_ids,
        part_ids,
        line_items,
        possible_duplicates: Vec::new(),
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

    // Remove the auto-created mileage log(s) — an orphaned log would keep
    // feeding reminders' mileage estimate while staying hidden from the feed.
    mileage_log::Entity::delete_many()
        .filter(mileage_log::Column::ServiceRecordId.eq(existing.id))
        .exec(&txn)
        .await?;

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
                paid_by: None,
                payer_note: None,
                schedule_item_ids: Some(vec![sched_id]),
                part_ids: Some(vec![part_id]),
                line_items: Some(vec![NewLineItem {
                    description: "Synthetic oil".into(),
                    category: Some("parts".into()),
                    quantity: Some(5.0),
                    unit_cost_cents: Some(1_000),
                    cost_cents: Some(5_000),
                }]),
                invoice_ref: None,
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
                paid_by: None,
                payer_note: None,
                schedule_item_ids: None,
                part_ids: Some(vec![part_a]),
                line_items: Some(vec![NewLineItem {
                    description: "old".into(),
                    category: None,
                    quantity: None,
                    unit_cost_cents: None,
                    cost_cents: Some(100),
                }]),
                invoice_ref: None,
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
                paid_by: None,
                payer_note: None,
                schedule_item_ids: Some(vec![sched_id]),
                part_ids: Some(vec![part_id]),
                line_items: None,
                invoice_ref: None,
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
    async fn create_links_auto_mileage_log_via_fk() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;

        let created = create(
            &db,
            vid,
            NewServiceRecord {
                mileage: Some(50_000),
                ..minimal_record(None)
            },
        )
        .await
        .unwrap();

        let logs = mileage_log::Entity::find()
            .filter(mileage_log::Column::VehicleId.eq(vid))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].service_record_id, Some(created.record.id));
        assert_eq!(logs[0].source.as_deref(), Some("service"));
    }

    #[tokio::test]
    async fn delete_removes_auto_created_mileage_log() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;

        let created = create(
            &db,
            vid,
            NewServiceRecord {
                mileage: Some(48_000),
                ..minimal_record(None)
            },
        )
        .await
        .unwrap();

        // A manual log must survive the service delete.
        let manual = mileage_log::ActiveModel {
            vehicle_id: Set(vid),
            mileage: Set(47_000),
            recorded_at: Set("2024-02-01 09:00:00".into()),
            source: Set(Some("manual".into())),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        delete(&db, vid, created.record.id).await.unwrap();

        let logs = mileage_log::Entity::find()
            .filter(mileage_log::Column::VehicleId.eq(vid))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(
            logs.iter().map(|l| l.id).collect::<Vec<_>>(),
            vec![manual.id],
            "only the manual log survives; the auto-log is gone from the DB"
        );
    }

    #[tokio::test]
    async fn update_syncs_auto_mileage_log() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;

        let created = create(
            &db,
            vid,
            NewServiceRecord {
                mileage: Some(50_000),
                ..minimal_record(None)
            },
        )
        .await
        .unwrap();

        // Changing the record's mileage updates the linked auto-log in place.
        update(
            &db,
            vid,
            created.record.id,
            UpdateServiceRecord {
                mileage: Some(Some(51_000)),
                service_date: Some("2024-04-01".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let logs = mileage_log::Entity::find()
            .filter(mileage_log::Column::ServiceRecordId.eq(created.record.id))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].mileage, 51_000);
        assert_eq!(logs[0].recorded_at, "2024-04-01");

        // Clearing the mileage removes the auto-log.
        update(
            &db,
            vid,
            created.record.id,
            UpdateServiceRecord {
                mileage: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(
            mileage_log::Entity::find()
                .filter(mileage_log::Column::ServiceRecordId.eq(created.record.id))
                .all(&db)
                .await
                .unwrap()
                .is_empty()
        );

        // Setting mileage again (no log left) creates a fresh linked auto-log.
        update(
            &db,
            vid,
            created.record.id,
            UpdateServiceRecord {
                mileage: Some(Some(52_000)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let logs = mileage_log::Entity::find()
            .filter(mileage_log::Column::ServiceRecordId.eq(created.record.id))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].mileage, 52_000);
        assert_eq!(logs[0].source.as_deref(), Some("service"));
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
            paid_by: None,
            payer_note: None,
            schedule_item_ids,
            part_ids: None,
            line_items: None,
            invoice_ref: None,
        }
    }

    // ─── invoice_ref idempotency + possible_duplicates advisory (hwaf) ───

    /// Recording the same invoice_ref twice returns the existing record
    /// UNTOUCHED — no second row, and the second call's extra links/line-items
    /// (and even a bogus schedule id, which the short-circuit skips guarding)
    /// are ignored.
    #[tokio::test]
    async fn record_same_invoice_ref_is_hard_idempotent_and_untouched() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;

        let first = create(
            &db,
            vid,
            NewServiceRecord {
                invoice_ref: Some("INV-42".into()),
                ..minimal_record(None)
            },
        )
        .await
        .unwrap();
        assert!(first.line_items.is_empty());

        let second = create(
            &db,
            vid,
            NewServiceRecord {
                invoice_ref: Some("INV-42".into()),
                // These MUST be ignored on the idempotent path — a bogus
                // schedule id would fail the guard if it ran.
                schedule_item_ids: Some(vec![9999]),
                line_items: Some(vec![NewLineItem {
                    description: "should not be added".into(),
                    category: None,
                    quantity: None,
                    unit_cost_cents: None,
                    cost_cents: Some(100),
                }]),
                ..minimal_record(None)
            },
        )
        .await
        .unwrap();

        assert_eq!(second.record.id, first.record.id, "same ref → same record");
        assert!(
            second.line_items.is_empty(),
            "idempotent return must not add the 2nd call's line items"
        );
        assert_eq!(list(&db, vid).await.unwrap().len(), 1, "no duplicate row");
    }

    /// Distinct invoice_refs are distinct records.
    #[tokio::test]
    async fn record_different_invoice_refs_are_distinct() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        for reference in ["A-1", "A-2"] {
            create(
                &db,
                vid,
                NewServiceRecord {
                    invoice_ref: Some(reference.into()),
                    ..minimal_record(None)
                },
            )
            .await
            .unwrap();
        }
        assert_eq!(list(&db, vid).await.unwrap().len(), 2);
    }

    /// Regression: two records with the SAME day and SAME cost but NO
    /// invoice_ref must BOTH persist — resemblance is never a dedup signal.
    #[tokio::test]
    async fn record_no_ref_same_day_same_cost_coexist() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        for _ in 0..2 {
            create(
                &db,
                vid,
                NewServiceRecord {
                    total_cost_cents: Some(5_000),
                    ..minimal_record(None)
                },
            )
            .await
            .unwrap();
        }
        assert_eq!(
            list(&db, vid).await.unwrap().len(),
            2,
            "distinct same-day/same-cost records must coexist"
        );
    }

    /// A ref-less create surfaces same-day peers as a soft advisory; the first
    /// (no peers) is empty, the second sees the first.
    #[tokio::test]
    async fn record_no_ref_surfaces_possible_duplicates() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;

        let first = create(&db, vid, minimal_record(None)).await.unwrap();
        assert!(
            first.possible_duplicates.is_empty(),
            "no prior same-day record → no advisory"
        );

        let second = create(&db, vid, minimal_record(None)).await.unwrap();
        assert_eq!(second.possible_duplicates.len(), 1);
        assert_eq!(second.possible_duplicates[0].id, first.record.id);
    }

    /// A blank invoice_ref is normalized to absent: two ref-less records sent
    /// with `Some("")`/whitespace must coexist, not collide on the unique
    /// index. And a ref with surrounding whitespace is the same identity.
    #[tokio::test]
    async fn blank_invoice_ref_is_treated_as_absent() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;

        for reference in ["", "   "] {
            create(
                &db,
                vid,
                NewServiceRecord {
                    invoice_ref: Some(reference.into()),
                    ..minimal_record(None)
                },
            )
            .await
            .expect("blank ref must not collide");
        }
        assert_eq!(list(&db, vid).await.unwrap().len(), 2, "both must persist");

        // Whitespace around a real ref is trimmed → same identity → idempotent.
        let a = create(
            &db,
            vid,
            NewServiceRecord {
                invoice_ref: Some("INV-9".into()),
                ..minimal_record(None)
            },
        )
        .await
        .unwrap();
        let b = create(
            &db,
            vid,
            NewServiceRecord {
                invoice_ref: Some("  INV-9  ".into()),
                ..minimal_record(None)
            },
        )
        .await
        .unwrap();
        assert_eq!(b.record.id, a.record.id, "trimmed ref is the same identity");
    }

    /// The partial unique index is a DB-level backstop: a raw second insert of
    /// the same (vehicle_id, invoice_ref) — bypassing the service's
    /// short-circuit — is rejected.
    #[tokio::test]
    async fn invoice_ref_partial_unique_index_rejects_raw_duplicate() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;

        let insert_raw = |reference: &str| {
            let reference = reference.to_string();
            service_record::ActiveModel {
                vehicle_id: Set(vid),
                service_date: Set("2024-03-01".into()),
                paid_by: Set("self".into()),
                invoice_ref: Set(Some(reference)),
                ..Default::default()
            }
        };
        insert_raw("DUP-1").insert(&db).await.unwrap();
        let err = insert_raw("DUP-1").insert(&db).await;
        assert!(
            err.is_err(),
            "duplicate (vehicle, invoice_ref) must be rejected"
        );

        // NULL refs are exempt from the partial index — two coexist.
        for _ in 0..2 {
            service_record::ActiveModel {
                vehicle_id: Set(vid),
                service_date: Set("2024-03-01".into()),
                paid_by: Set("self".into()),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn create_round_trips_payer_and_defaults_to_self() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;

        // Explicit payer round-trips, note included.
        let insured = create(
            &db,
            vid,
            NewServiceRecord {
                paid_by: Some("insurance".into()),
                payer_note: Some("Progressive claim #12345".into()),
                ..minimal_record(None)
            },
        )
        .await
        .unwrap();
        assert_eq!(insured.record.paid_by, "insurance");
        assert_eq!(
            insured.record.payer_note.as_deref(),
            Some("Progressive claim #12345")
        );
        let fetched = get(&db, vid, insured.record.id).await.unwrap();
        assert_eq!(fetched.record.paid_by, "insurance");
        assert_eq!(
            fetched.record.payer_note.as_deref(),
            Some("Progressive claim #12345")
        );

        // Omitted payer defaults to self.
        let plain = create(&db, vid, minimal_record(None)).await.unwrap();
        assert_eq!(plain.record.paid_by, "self");
        assert_eq!(plain.record.payer_note, None);
    }

    #[tokio::test]
    async fn create_rejects_unknown_payer() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;

        let err = create(
            &db,
            vid,
            NewServiceRecord {
                paid_by: Some("my neighbor".into()),
                ..minimal_record(None)
            },
        )
        .await
        .unwrap_err();
        match err {
            DomainError::BadRequest(msg) => {
                // The message lists the valid values (build-status whitelist shape).
                assert!(
                    msg.contains("self"),
                    "message should list valid values: {msg}"
                );
                assert!(msg.contains("insurance"), "{msg}");
                assert!(msg.contains("third_party"), "{msg}");
            }
            other => panic!("expected BadRequest, got {other:?}"),
        }
        assert!(list(&db, vid).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn update_changes_payer_and_sets_then_clears_note() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = create(&db, vid, minimal_record(None)).await.unwrap();
        assert_eq!(created.record.paid_by, "self");

        // Change payer + set the note.
        let updated = update(
            &db,
            vid,
            created.record.id,
            UpdateServiceRecord {
                paid_by: Some("third_party".into()),
                payer_note: Some(Some("Neighbor's insurance, side-swipe".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.record.paid_by, "third_party");
        assert_eq!(
            updated.record.payer_note.as_deref(),
            Some("Neighbor's insurance, side-swipe")
        );

        // An update not mentioning payer fields leaves them alone.
        let untouched = update(
            &db,
            vid,
            created.record.id,
            UpdateServiceRecord {
                notes: Some(Some("unrelated".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(untouched.record.paid_by, "third_party");
        assert!(untouched.record.payer_note.is_some());

        // Explicit null clears the note (double-option).
        let cleared = update(
            &db,
            vid,
            created.record.id,
            UpdateServiceRecord {
                payer_note: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.record.payer_note, None);

        // Invalid payer on update is rejected and mutates nothing.
        assert!(matches!(
            update(
                &db,
                vid,
                created.record.id,
                UpdateServiceRecord {
                    paid_by: Some("bogus".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err(),
            DomainError::BadRequest(_)
        ));
        let survived = get(&db, vid, created.record.id).await.unwrap();
        assert_eq!(survived.record.paid_by, "third_party");
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
    async fn link_schedule_items_add_unions_and_dedups() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let item_a = seed_schedule_item(&db, vid, "Oil change").await;
        let item_b = seed_schedule_item(&db, vid, "Tire rotation").await;
        let created = create(&db, vid, minimal_record(Some(vec![item_a])))
            .await
            .unwrap();

        // Add mode unions with the existing link; re-sending item_a is a no-op.
        let linked = link_schedule_items(
            &db,
            vid,
            created.record.id,
            &[item_a, item_b],
            LinkMode::Add,
        )
        .await
        .unwrap();
        let mut ids = linked.schedule_item_ids.clone();
        ids.sort_unstable();
        assert_eq!(ids, vec![item_a, item_b]);

        // No duplicate link rows were written.
        let links = service_schedule_link::Entity::find()
            .filter(service_schedule_link::Column::ServiceRecordId.eq(created.record.id))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(links.len(), 2);
    }

    #[tokio::test]
    async fn link_schedule_items_replace_overwrites() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let item_a = seed_schedule_item(&db, vid, "Oil change").await;
        let item_b = seed_schedule_item(&db, vid, "Tire rotation").await;
        let created = create(&db, vid, minimal_record(Some(vec![item_a])))
            .await
            .unwrap();

        let linked = link_schedule_items(&db, vid, created.record.id, &[item_b], LinkMode::Replace)
            .await
            .unwrap();
        assert_eq!(linked.schedule_item_ids, vec![item_b]);
    }

    #[tokio::test]
    async fn link_schedule_items_wrong_parent_is_byte_identical_not_found() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let own_item = seed_schedule_item(&db, owner, "Mine").await;
        let foreign_item = seed_schedule_item(&db, other, "Foreign").await;
        let mine = create(&db, owner, minimal_record(None)).await.unwrap();
        let theirs = create(&db, other, minimal_record(None)).await.unwrap();

        // Another vehicle's service record must read exactly like a
        // nonexistent one.
        let wrong_parent =
            link_schedule_items(&db, owner, theirs.record.id, &[own_item], LinkMode::Add)
                .await
                .unwrap_err();
        assert_eq!(
            wrong_parent.to_string(),
            format!("Service record {} not found", theirs.record.id)
        );

        // Another vehicle's schedule item likewise; nothing was linked.
        let wrong_item =
            link_schedule_items(&db, owner, mine.record.id, &[foreign_item], LinkMode::Add)
                .await
                .unwrap_err();
        assert_eq!(
            wrong_item.to_string(),
            format!("Schedule item {foreign_item} not found")
        );
        let fetched = get(&db, owner, mine.record.id).await.unwrap();
        assert!(fetched.schedule_item_ids.is_empty());
    }

    #[test]
    fn link_mode_parses_and_rejects() {
        assert_eq!(LinkMode::parse("add").unwrap(), LinkMode::Add);
        assert_eq!(LinkMode::parse("replace").unwrap(), LinkMode::Replace);
        assert!(matches!(
            LinkMode::parse("merge").unwrap_err(),
            DomainError::BadRequest(_)
        ));
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
