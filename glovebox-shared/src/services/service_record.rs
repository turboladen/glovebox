use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{
        incident, incident_service_link, mileage_log, part, service_recall_link, service_record,
        service_record_line_item, service_schedule_link,
    },
    error::{DomainError, DomainResult},
    inputs::{
        part::NewPart,
        service_record::{NewLineItem, NewServiceRecord, UpdateServiceRecord},
    },
    services::{
        incident::{self as incident_svc, ServiceRef},
        part as part_svc,
        recall::{self, RecallRef},
        schedule,
        work_item::{self, WorkItemRef},
    },
};

/// Allowed values for `service_records.performed_by`.
pub const VALID_PERFORMED_BY: [&str; 2] = ["shop", "self"];

/// Allowed values for `service_record_line_items.work_kind`. `fee` is a
/// charge on the invoice that is not work, such as tax, shop supplies or
/// disposal.
pub const VALID_WORK_KINDS: [&str; 6] = [
    "maintenance",
    "mod",
    "repair",
    "inspection",
    "recall",
    "fee",
];

fn validate_choice(field: &str, value: &str, allowed: &[&str]) -> DomainResult<()> {
    if allowed.contains(&value) {
        return Ok(());
    }
    Err(DomainError::invalid(
        field,
        format!(
            "'{value}' is not valid. Must be one of: {}",
            allowed.join(", ")
        ),
    ))
}

fn validate_line_items(items: &[NewLineItem]) -> DomainResult<()> {
    items.iter().try_for_each(|li| {
        validate_choice("work_kind", &li.work_kind, &VALID_WORK_KINDS)?;
        li.paid_by.as_deref().map_or(Ok(()), validate_payer)
    })
}

/// Checks the dates are `YYYY-MM-DD`, the range runs forward, and a
/// self-performed job has no shop. Dates compare as `YYYY-MM-DD` text.
fn validate_who_and_when(
    service_date: &str,
    end_date: Option<&str>,
    performed_by: &str,
    shop_name: Option<&str>,
) -> DomainResult<()> {
    validate_choice("performed_by", performed_by, &VALID_PERFORMED_BY)?;
    super::validate_date("service_date", service_date)?;
    super::validate_optional_date("end_date", end_date)?;
    if end_date.is_some_and(|end| end < service_date) {
        return Err(DomainError::invalid(
            "end_date",
            "must not be before service_date",
        ));
    }
    if performed_by == "self" && shop_name.is_some() {
        return Err(DomainError::invalid(
            "shop_name",
            "a self-performed service has no shop",
        ));
    }
    Ok(())
}

/// Trims an invoice number and treats a blank one as absent. An LLM may send
/// "" or spaces rather than omitting the field, and a blank ref stored as a
/// real one would make the partial unique index reject a second ref-less
/// record as a duplicate.
fn normalize_invoice_ref(invoice_ref: Option<&str>) -> Option<String> {
    invoice_ref
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty())
}

/// Payer whitelist for `service_records.paid_by` and
/// `service_record_line_items.paid_by`.
pub const VALID_PAYERS: [&str; 4] = ["self", "insurance", "third_party", "manufacturer"];

/// The `paid_by` value of a payer that is the owner.
pub const PAYER_SELF: &str = "self";

/// Who paid for a line: its own `paid_by` when it has one, otherwise the
/// service's.
pub fn effective_payer<'a>(line_paid_by: Option<&'a str>, service_paid_by: &'a str) -> &'a str {
    line_paid_by.unwrap_or(service_paid_by)
}

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

#[derive(Debug, Serialize)]
pub struct ServiceRecordWithLinks {
    #[serde(flatten)]
    pub record: service_record::Model,
    pub schedule_item_ids: Vec<i32>,
    pub part_ids: Vec<i32>,
    /// The full rows behind `part_ids`.
    pub parts: Vec<part::Model>,
    pub line_items: Vec<service_record_line_item::Model>,
    /// Incidents first noticed during this service. Each one's
    /// `other_service` is the service that fixed it, if any.
    pub noticed_incidents: Vec<IncidentRef>,
    /// Incidents this service fixed. Each one's `other_service` is the
    /// service during which it was noticed, if any.
    pub resolved_incidents: Vec<IncidentRef>,
    /// Recalls this service performed.
    pub performed_recalls: Vec<RecallRef>,
    /// To-do items this service completed.
    pub completed_work_items: Vec<WorkItemRef>,
    /// True when `create` found a record with the same `invoice_ref` on this
    /// vehicle and returned it unchanged instead of writing anything.
    /// Serialized only when true.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub already_recorded: bool,
    /// Present only when `already_recorded` is true: says that nothing in the
    /// call was applied and points at `correct_service`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The sum of the line items' `cost_cents`, or `None` when the service has
    /// no lines or none has a cost.
    pub lines_total_cents: Option<i64>,
    /// Present only when `total_cost_cents` and `lines_total_cents` both exist
    /// and differ: states both amounts and the difference. It is information
    /// about the record, unlike `message`, which reports what a call did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Soft advisory: other same-day records on this vehicle, surfaced only
    /// when a record was created WITHOUT an `invoice_ref` so the caller can
    /// reconcile a possible duplicate. Never blocks — distinct records
    /// coexist. Empty (and omitted from JSON) on every other path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub possible_duplicates: Vec<PossibleDuplicate>,
}

/// An incident as seen from a service record.
#[derive(Clone, Debug, Serialize)]
pub struct IncidentRef {
    pub id: i32,
    pub title: String,
    pub category: String,
    pub resolved: bool,
    /// The service on the other end of the incident's link; see the field it
    /// appears under on `ServiceRecordWithLinks`.
    pub other_service: Option<ServiceRef>,
}

/// The incident links of a batch of service records, keyed by record id.
#[derive(Default)]
struct IncidentLinks {
    noticed: std::collections::HashMap<i32, Vec<IncidentRef>>,
    resolved: std::collections::HashMap<i32, Vec<IncidentRef>>,
}

/// Loads the noticed and resolved incidents of each record with four batched
/// queries however many records there are.
async fn load_incident_links(
    db: &impl ConnectionTrait,
    record_ids: &[i32],
) -> DomainResult<IncidentLinks> {
    let mut out = IncidentLinks::default();
    if record_ids.is_empty() {
        return Ok(out);
    }
    let mine = incident_service_link::Entity::find()
        .filter(incident_service_link::Column::ServiceRecordId.is_in(record_ids.to_vec()))
        .all(db)
        .await?;
    if mine.is_empty() {
        return Ok(out);
    }
    let incident_ids: Vec<i32> = mine.iter().map(|l| l.incident_id).collect();
    let all_links = incident_service_link::Entity::find()
        .filter(incident_service_link::Column::IncidentId.is_in(incident_ids.clone()))
        .all(db)
        .await?;
    let incidents: std::collections::HashMap<i32, incident::Model> = incident::Entity::find()
        .filter(incident::Column::Id.is_in(incident_ids))
        .all(db)
        .await?
        .into_iter()
        .map(|i| (i.id, i))
        .collect();
    let other_ids: Vec<i32> = all_links.iter().map(|l| l.service_record_id).collect();
    let services: std::collections::HashMap<i32, service_record::Model> =
        service_record::Entity::find()
            .filter(service_record::Column::Id.is_in(other_ids))
            .all(db)
            .await?
            .into_iter()
            .map(|s| (s.id, s))
            .collect();

    // The other end of a link: the incident's noticing service, or, when
    // looking from the noticing side, the latest service that fixed it.
    let other_for = |incident_id: i32, role: &str| -> Option<ServiceRef> {
        all_links
            .iter()
            .filter(|l| l.incident_id == incident_id && l.role == role)
            .filter_map(|l| services.get(&l.service_record_id))
            .max_by(|a, b| (&a.service_date, a.id).cmp(&(&b.service_date, b.id)))
            .cloned()
            .map(ServiceRef::from)
    };

    for link in &mine {
        let Some(inc) = incidents.get(&link.incident_id) else {
            continue;
        };
        let (bucket, other_role) = match link.role.as_str() {
            "noticed" => (&mut out.noticed, "resolved"),
            "resolved" => (&mut out.resolved, "noticed"),
            _ => continue,
        };
        bucket
            .entry(link.service_record_id)
            .or_default()
            .push(IncidentRef {
                id: inc.id,
                title: inc.title.clone(),
                category: inc.category.clone(),
                resolved: inc.resolved,
                other_service: other_for(inc.id, other_role),
            });
    }
    for refs in out.noticed.values_mut().chain(out.resolved.values_mut()) {
        refs.sort_by_key(|r| r.id);
    }
    Ok(out)
}

/// A lightweight same-day peer surfaced in `possible_duplicates`.
#[derive(Debug, Serialize)]
pub struct PossibleDuplicate {
    pub id: i32,
    pub service_date: String,
    pub description: Option<String>,
    pub total_cost_cents: Option<i32>,
}

impl ServiceRecordWithLinks {
    fn assemble(
        record: service_record::Model,
        schedule_item_ids: Vec<i32>,
        parts: Vec<part::Model>,
        line_items: Vec<service_record_line_item::Model>,
        noticed_incidents: Vec<IncidentRef>,
        resolved_incidents: Vec<IncidentRef>,
        performed_recalls: Vec<RecallRef>,
    ) -> Self {
        let lines_total_cents = line_items
            .iter()
            .filter_map(|l| l.cost_cents)
            .map(i64::from)
            .fold(None, |acc, c| Some(acc.unwrap_or(0) + c));
        let note = match (record.total_cost_cents, lines_total_cents) {
            (Some(total), Some(lines)) if i64::from(total) != lines => Some(format!(
                "The total is {total} cents and the line items add up to {lines} cents, a \
                 difference of {} cents. Tax or fees can be recorded as a line with work_kind \
                 \"fee\".",
                i64::from(total) - lines
            )),
            _ => None,
        };
        Self {
            lines_total_cents,
            note,
            record,
            schedule_item_ids,
            part_ids: parts.iter().map(|p| p.id).collect(),
            parts,
            line_items,
            noticed_incidents,
            resolved_incidents,
            performed_recalls,
            completed_work_items: Vec::new(),
            already_recorded: false,
            message: None,
            possible_duplicates: Vec::new(),
        }
    }
}

/// Load a record's schedule link ids, installed parts, line items, incident
/// links, performed recalls and completed to-do items.
async fn load_service_links(
    db: &impl ConnectionTrait,
    record: service_record::Model,
) -> DomainResult<ServiceRecordWithLinks> {
    let schedule_item_ids = service_schedule_link::Entity::find()
        .filter(service_schedule_link::Column::ServiceRecordId.eq(record.id))
        .all(db)
        .await?
        .into_iter()
        .map(|l| l.schedule_item_id)
        .collect();

    let parts = part::Entity::find()
        .filter(part::Column::InstalledServiceId.eq(record.id))
        .order_by_asc(part::Column::Id)
        .all(db)
        .await?;

    let line_items = service_record_line_item::Entity::find()
        .filter(service_record_line_item::Column::ServiceRecordId.eq(record.id))
        .order_by_asc(service_record_line_item::Column::Id)
        .all(db)
        .await?;

    let mut incident_links = load_incident_links(db, &[record.id]).await?;
    let noticed = incident_links
        .noticed
        .remove(&record.id)
        .unwrap_or_default();
    let resolved = incident_links
        .resolved
        .remove(&record.id)
        .unwrap_or_default();
    let performed_recalls = recall::load_performed(db, &[record.id])
        .await?
        .remove(&record.id)
        .unwrap_or_default();

    let completed_work_items = work_item::load_completed_by(db, &[record.id])
        .await?
        .remove(&record.id)
        .unwrap_or_default();

    let mut links = ServiceRecordWithLinks::assemble(
        record,
        schedule_item_ids,
        parts,
        line_items,
        noticed,
        resolved,
        performed_recalls,
    );
    links.completed_work_items = completed_work_items;
    Ok(links)
}

/// Insert line items for a service record within a transaction.
async fn insert_line_items(
    txn: &impl ConnectionTrait,
    record_id: i32,
    items: Vec<NewLineItem>,
) -> DomainResult<()> {
    // An empty batch inserts nothing.
    let rows = items
        .into_iter()
        .map(|item| service_record_line_item::ActiveModel {
            service_record_id: Set(record_id),
            description: Set(item.description),
            work_kind: Set(item.work_kind),
            paid_by: Set(item.paid_by),
            quantity: Set(item.quantity),
            unit_cost_cents: Set(item.unit_cost_cents),
            cost_cents: Set(item.cost_cents),
            ..Default::default()
        });
    service_record_line_item::Entity::insert_many(rows)
        .exec_without_returning(txn)
        .await?;
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
    list_between(db, vehicle_id, None, None).await
}

/// Lists a vehicle's services newest first, keeping those whose service date
/// falls on or after `from` and on or before `to` (both `YYYY-MM-DD`; either
/// bound may be omitted).
pub async fn list_between(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    from: Option<&str>,
    to: Option<&str>,
) -> DomainResult<Vec<ServiceRecordWithLinks>> {
    super::vehicle::require(db, vehicle_id).await?;
    super::validate_optional_date("from", from)?;
    super::validate_optional_date("to", to)?;
    let mut query =
        service_record::Entity::find().filter(service_record::Column::VehicleId.eq(vehicle_id));
    if let Some(from) = from {
        query = query.filter(service_record::Column::ServiceDate.gte(from));
    }
    if let Some(to) = to {
        query = query.filter(service_record::Column::ServiceDate.lte(to));
    }
    let records = query
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
            .order_by_asc(part::Column::Id)
            .all(db)
            .await?
    };

    let mut incident_links = load_incident_links(db, &record_ids).await?;
    let mut performed = recall::load_performed(db, &record_ids).await?;
    let mut completed = work_item::load_completed_by(db, &record_ids).await?;

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
            let parts = all_parts
                .iter()
                .filter(|p| p.installed_service_id == Some(record.id))
                .cloned()
                .collect();
            let line_items = all_line_items
                .iter()
                .filter(|li| li.service_record_id == record.id)
                .cloned()
                .collect();
            let noticed = incident_links
                .noticed
                .remove(&record.id)
                .unwrap_or_default();
            let resolved = incident_links
                .resolved
                .remove(&record.id)
                .unwrap_or_default();
            let performed_recalls = performed.remove(&record.id).unwrap_or_default();
            let completed_work_items = completed.remove(&record.id).unwrap_or_default();
            let mut links = ServiceRecordWithLinks::assemble(
                record,
                schedule_item_ids,
                parts,
                line_items,
                noticed,
                resolved,
                performed_recalls,
            );
            links.completed_work_items = completed_work_items;
            links
        })
        .collect();

    Ok(results)
}

/// Fetches a service record scoped to its vehicle. A record that belongs to
/// another vehicle produces the same `NotFound` as one that does not exist.
pub(crate) async fn require(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<service_record::Model> {
    service_record::Entity::find_by_id(id)
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Service record {id} not found")))
}

pub async fn get(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<ServiceRecordWithLinks> {
    let record = require(db, vehicle_id, id).await?;
    load_service_links(db, record).await
}

/// Marks existing parts as installed by `record` and creates the parts its
/// invoice lists, already installed.
async fn install_parts(
    txn: &impl ConnectionTrait,
    vehicle_id: i32,
    record: &service_record::Model,
    existing_ids: Vec<i32>,
    new_parts: Vec<NewPart>,
) -> DomainResult<()> {
    // A record with no odometer reading leaves each part's own reading.
    let odometer = record.mileage.map_or(NotSet, |miles| Set(Some(miles)));
    for part_id in existing_ids {
        part_svc::install(txn, vehicle_id, part_id, record, odometer.clone()).await?;
    }

    for mut new_part in new_parts {
        new_part.status = Some("installed".into());
        new_part.installed_service_id = Some(record.id);
        new_part.installed_date = Some(record.service_date.clone());
        new_part.installed_odometer = record.mileage;
        part_svc::create(txn, vehicle_id, new_part).await?;
    }
    Ok(())
}

/// Other records on the vehicle dated the same day as `record`.
async fn same_day_peers(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    record: &service_record::Model,
) -> DomainResult<Vec<PossibleDuplicate>> {
    Ok(service_record::Entity::find()
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
        .collect())
}

/// Links the record to the incidents it fixed and the ones it noticed. A
/// completed to-do that came from an incident fixes that incident too.
async fn link_incidents(
    txn: &impl ConnectionTrait,
    vehicle_id: i32,
    record_id: i32,
    mut resolved_ids: Vec<i32>,
    noticed_ids: Vec<i32>,
    completed: &[crate::entities::work_item::Model],
) -> DomainResult<()> {
    resolved_ids.extend(completed.iter().filter_map(|w| w.incident_id));
    for (ids, role) in [(resolved_ids, "resolved"), (noticed_ids, "noticed")] {
        for id in super::dedup_ids(&ids) {
            incident_svc::link_service(txn, vehicle_id, id, record_id, role).await?;
        }
    }
    Ok(())
}

/// Loads a record found under a repeated `invoice_ref`, flagged so the caller
/// knows nothing in its call was applied.
async fn load_existing(
    db: &impl ConnectionTrait,
    existing: service_record::Model,
) -> DomainResult<ServiceRecordWithLinks> {
    let mut found = load_service_links(db, existing).await?;
    found.already_recorded = true;
    found.message = Some(format!(
        "This invoice number is already recorded as service {}; nothing in this call was applied. \
         Use correct_service to change it.",
        found.record.id
    ));
    Ok(found)
}

/// Creates a service record with its line items, installed parts, schedule
/// links and odometer reading in one transaction; any failure leaves nothing
/// behind. A record already filed under the same `invoice_ref` is returned
/// unchanged with `already_recorded` set, and the incident and recall ids in
/// this call are ignored in that case. A completed to-do that came from a
/// recall links that recall to the record as well.
pub async fn create<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    input: NewServiceRecord,
) -> DomainResult<ServiceRecordWithLinks> {
    super::vehicle::require(db, vehicle_id).await?;
    if let Some(paid_by) = &input.paid_by {
        validate_payer(paid_by)?;
    }

    let invoice_ref = normalize_invoice_ref(input.invoice_ref.as_deref());

    // Hard-idempotency: an invoice_ref is a deterministic identity signal.
    // If this vehicle already has a record with the same ref, return it
    // UNTOUCHED: no new record, no new links, parts or line items from this
    // call. The short-circuit runs before any other check or write. The
    // partial unique index is the backstop against a race.
    if let Some(invoice_ref) = &invoice_ref {
        let existing = service_record::Entity::find()
            .filter(service_record::Column::VehicleId.eq(vehicle_id))
            .filter(service_record::Column::InvoiceRef.eq(invoice_ref.as_str()))
            .one(db)
            .await?;
        if let Some(existing) = existing {
            let found = load_existing(db, existing).await?;
            return Ok(found);
        }
    }

    let performed_by = input.performed_by.unwrap_or_else(|| "shop".into());
    validate_who_and_when(
        &input.service_date,
        input.end_date.as_deref(),
        &performed_by,
        input.shop_name.as_deref(),
    )?;
    let line_items_input = input.line_items.unwrap_or_default();
    validate_line_items(&line_items_input)?;

    let txn = db.begin().await?;

    // Resolved before the record is written so a bad schedule id is reported
    // ahead of any error the insert itself could raise.
    let schedule_item_ids = schedule::tracked_ids(
        &txn,
        vehicle_id,
        &input.schedule_item_ids.unwrap_or_default(),
    )
    .await?;

    let record = service_record::ActiveModel {
        vehicle_id: Set(vehicle_id),
        service_date: Set(input.service_date.clone()),
        end_date: Set(input.end_date),
        performed_by: Set(performed_by),
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
        paid_by: Set(input.paid_by.unwrap_or_else(|| "self".into())),
        payer_note: Set(input.payer_note),
        invoice_ref: Set(invoice_ref),
        ..Default::default()
    };
    let record = record.insert(&txn).await?;

    // Every link step below runs on `txn`, so a failure in any of them rolls
    // back the record and everything written before it.
    schedule::insert_service_links(&txn, record.id, &schedule_item_ids, LinkMode::Add).await?;

    let completed = work_item::complete_with_service(
        &txn,
        vehicle_id,
        &input.work_item_ids.unwrap_or_default(),
        record.id,
    )
    .await?;

    link_incidents(
        &txn,
        vehicle_id,
        record.id,
        input.resolved_incident_ids.unwrap_or_default(),
        input.noticed_incident_ids.unwrap_or_default(),
        &completed,
    )
    .await?;

    let mut recall_ids = input.recall_finding_ids.unwrap_or_default();
    // Only the explicit ids must be recalls; a completed to-do may come from
    // any kind of finding, and only a recall-sourced one links a recall.
    let from_work: Vec<i32> = completed
        .iter()
        .filter_map(|w| w.research_finding_id)
        .collect();
    recall_ids.extend(recall::recall_ids_among(&txn, &from_work).await?);
    recall::link_service(&txn, vehicle_id, &recall_ids, record.id).await?;

    install_parts(
        &txn,
        vehicle_id,
        &record,
        input.part_ids.unwrap_or_default(),
        input.parts.unwrap_or_default(),
    )
    .await?;

    insert_line_items(&txn, record.id, line_items_input).await?;

    // The odometer reading is linked by FK (activity dedupe and
    // update/delete maintenance key on it); `source` is a display label.
    if let Some(miles) = record.mileage {
        insert_auto_mileage_log(&txn, vehicle_id, &record, miles).await?;
    }

    let mut created = load_service_links(&txn, record).await?;

    txn.commit().await?;

    // Soft advisory: with no invoice_ref there is no deterministic identity
    // signal, so surface other same-day records on this vehicle for the caller
    // to reconcile. Read-only, never blocks; distinct records coexist.
    if created.record.invoice_ref.is_none() {
        created.possible_duplicates = same_day_peers(db, vehicle_id, &created.record).await?;
    }

    Ok(created)
}

#[allow(clippy::too_many_lines)]
pub async fn update<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    input: UpdateServiceRecord,
) -> DomainResult<ServiceRecordWithLinks> {
    let existing = require(db, vehicle_id, id).await?;

    validate_who_and_when(
        input
            .service_date
            .as_ref()
            .unwrap_or(&existing.service_date),
        input
            .end_date
            .as_ref()
            .unwrap_or(&existing.end_date)
            .as_deref(),
        input
            .performed_by
            .as_ref()
            .unwrap_or(&existing.performed_by),
        input
            .shop_name
            .as_ref()
            .unwrap_or(&existing.shop_name)
            .as_deref(),
    )?;
    if let Some(items) = &input.line_items {
        validate_line_items(items)?;
    }
    let invoice_ref = input
        .invoice_ref
        .map(|r| normalize_invoice_ref(r.as_deref()));
    if let Some(Some(invoice_ref)) = &invoice_ref {
        require_invoice_ref_free(db, vehicle_id, id, invoice_ref).await?;
    }

    let txn = db.begin().await?;

    let (date_before, mileage_before) = (existing.service_date.clone(), existing.mileage);
    let mut active: service_record::ActiveModel = existing.into();
    if let Some(v) = invoice_ref {
        active.invoice_ref = Set(v);
    }

    if let Some(v) = input.service_date {
        active.service_date = Set(v);
    }
    if let Some(v) = input.end_date {
        active.end_date = Set(v);
    }
    if let Some(v) = input.performed_by {
        active.performed_by = Set(v);
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
    if let Some(v) = input.paid_by {
        validate_payer(&v)?;
        active.paid_by = Set(v);
    }
    if let Some(v) = input.payer_note {
        active.payer_note = Set(v);
    }

    active.updated_at = Set(super::now_stamp());

    let record = active.update(&txn).await?;

    // Keep the auto-created mileage log in step with the record.
    sync_auto_mileage_log(&txn, vehicle_id, &record).await?;

    if let Some(item_ids) = input.schedule_item_ids {
        schedule::link_service(&txn, vehicle_id, record.id, &item_ids, LinkMode::Replace).await?;
    }

    // Handle part linking on update
    if let Some(new_part_ids) = input.part_ids {
        // Unlink previously linked parts
        let old_parts = part::Entity::find()
            .filter(part::Column::InstalledServiceId.eq(record.id))
            .all(&txn)
            .await?;
        for old_part in old_parts {
            if !new_part_ids.contains(&old_part.id) {
                part_svc::uninstall(
                    &txn,
                    old_part,
                    part_svc::UninstallStatus::PurchasedIfInstalled,
                )
                .await?;
            }
        }
        // Link new parts
        for &part_id in &new_part_ids {
            part_svc::install(&txn, vehicle_id, part_id, &record, Set(record.mileage)).await?;
        }
    } else if record.service_date != date_before || record.mileage != mileage_before {
        // The parts this service installed carry its date and mileage as
        // their install date and odometer.
        part_svc::follow_service(&txn, &record).await?;
    }

    if let Some(recall_ids) = input.recall_finding_ids {
        service_recall_link::Entity::delete_many()
            .filter(service_recall_link::Column::ServiceRecordId.eq(record.id))
            .exec(&txn)
            .await?;
        recall::link_service(&txn, vehicle_id, &recall_ids, record.id).await?;
    }

    // Handle line items: replace-all semantics when provided
    if let Some(new_line_items) = input.line_items {
        service_record_line_item::Entity::delete_many()
            .filter(service_record_line_item::Column::ServiceRecordId.eq(record.id))
            .exec(&txn)
            .await?;
        insert_line_items(&txn, record.id, new_line_items).await?;
    }

    let updated = load_service_links(&txn, record).await?;

    txn.commit().await?;

    Ok(updated)
}

/// Refuses an invoice number that another service on the vehicle already
/// carries, naming that service.
async fn require_invoice_ref_free(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    service_id: i32,
    invoice_ref: &str,
) -> DomainResult<()> {
    let other = service_record::Entity::find()
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .filter(service_record::Column::InvoiceRef.eq(invoice_ref))
        .filter(service_record::Column::Id.ne(service_id))
        .one(db)
        .await?;
    match other {
        Some(other) => Err(DomainError::BadRequest(format!(
            "Invoice ref '{invoice_ref}' already belongs to service {} on this vehicle",
            other.id
        ))),
        None => Ok(()),
    }
}

pub use super::schedule::LinkMode;

/// Link a service record to maintenance-schedule items without touching the
/// rest of the record. Self-guarded: the record must belong to `vehicle_id`,
/// and every item must be in the vehicle's schedule scope; wrong-parent ids
/// read as missing. An inherited item the vehicle has its own copy of links
/// to the copy. Linking is what clears the corresponding reminders.
pub async fn link_schedule_items<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    item_ids: &[i32],
    mode: LinkMode,
) -> DomainResult<ServiceRecordWithLinks> {
    let existing = require(db, vehicle_id, id).await?;

    let txn = db.begin().await?;
    schedule::link_service(&txn, vehicle_id, existing.id, item_ids, mode).await?;

    // Linking changes what the record accounts for — stamp it like update().
    let mut active: service_record::ActiveModel = existing.into();
    active.updated_at = Set(super::now_stamp());
    let record = active.update(&txn).await?;
    txn.commit().await?;

    load_service_links(db, record).await
}

pub use super::recall::RecallsOnDelete;

/// What [`delete`] changed besides the service itself.
#[derive(Debug, Default, Serialize)]
pub struct DeletedService {
    /// Ids of the parts the service installed, now `purchased` and unlinked.
    pub kept_part_ids: Vec<i32>,
    /// Ids of the recalls that read `open` again after a `Reopen` delete.
    pub reopened_recall_ids: Vec<i32>,
    /// Ids of the recalls a `Reopen` delete left `done` because another service also performed them.
    pub still_done_elsewhere_recall_ids: Vec<i32>,
    /// Ids of the recalls stored as `done` because the caller chose `KeepDone`.
    pub kept_done_recall_ids: Vec<i32>,
}

/// Deletes a service record and its owned rows, and reopens what it closed.
///
/// If the service performed recalls, `recalls` must say what becomes of them:
/// with `None` nothing is deleted and the call returns `BadRequest` listing
/// the recalls and telling the caller to ask the owner. Without recalls the
/// parameter is ignored. A recall another service also performed stays `done` and is reported in
/// `still_done_elsewhere_recall_ids`. Each incident the service resolved goes back to
/// unresolved unless another service also resolved it. Each work item it
/// completed returns to `planned`. Parts it installed stay, reverted to
/// `purchased` with their install fields cleared, including parts created
/// inline by this service; their ids come back in `kept_part_ids` so the
/// caller can attach them to a re-filed service or delete them with
/// `part::delete`.
pub async fn delete<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    recalls: Option<RecallsOnDelete>,
) -> DomainResult<DeletedService> {
    let existing = require(db, vehicle_id, id).await?;

    let txn = db.begin().await?;

    let performed = recall::load_performed(&txn, &[existing.id])
        .await?
        .remove(&existing.id)
        .unwrap_or_default();
    // The choice is checked before anything is written.
    let recall_choice = match (performed.is_empty(), recalls) {
        (true, _) => None,
        (false, None) => return Err(recalls_need_a_choice(&performed)),
        (false, Some(choice)) => Some(choice),
    };
    let mut outcome = DeletedService::default();

    work_item::reopen_completed_by(&txn, existing.id).await?;
    reopen_resolved_incidents(&txn, existing.id).await?;

    // Unlink parts installed during this service (revert to purchased)
    let linked_parts = part::Entity::find()
        .filter(part::Column::InstalledServiceId.eq(existing.id))
        .order_by_asc(part::Column::Id)
        .all(&txn)
        .await?;
    outcome.kept_part_ids = linked_parts.iter().map(|p| p.id).collect();
    for p in linked_parts {
        part_svc::uninstall(&txn, p, part_svc::UninstallStatus::Purchased).await?;
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

    service_recall_link::Entity::delete_many()
        .filter(service_recall_link::Column::ServiceRecordId.eq(existing.id))
        .exec(&txn)
        .await?;

    // Incident links carry an ON DELETE CASCADE FK, but delete explicitly —
    // pragma-independent and inside the txn, mirroring incident::delete.
    incident_service_link::Entity::delete_many()
        .filter(incident_service_link::Column::ServiceRecordId.eq(existing.id))
        .exec(&txn)
        .await?;

    // Delete the service record
    existing.delete(&txn).await?;

    if let Some(choice) = recall_choice {
        let finding_ids = performed.iter().map(|r| r.finding_id).collect();
        let released = recall::release_from_deleted_service(&txn, finding_ids, choice).await?;
        outcome.reopened_recall_ids = released.reopened;
        outcome.kept_done_recall_ids = released.kept_done;
        outcome.still_done_elsewhere_recall_ids = released.still_done_elsewhere;
    }

    txn.commit().await?;

    Ok(outcome)
}

fn recalls_need_a_choice(performed: &[RecallRef]) -> DomainError {
    let listed = performed
        .iter()
        .map(|r| match &r.campaign_number {
            Some(n) => format!("recall {} (campaign {n}): {}", r.finding_id, r.title),
            None => format!("recall {}: {}", r.finding_id, r.title),
        })
        .collect::<Vec<_>>()
        .join("; ");
    DomainError::BadRequest(format!(
        "This service performed {listed}. Ask the owner whether these recalls should be reopened \
         or kept as done, then call again with recalls: \"reopen\" or recalls: \"keep_done\". \
         Nothing was deleted."
    ))
}

/// Marks unresolved each incident a service that is about to be deleted
/// resolved, unless another service also resolved it. Runs before the link
/// rows are removed because it reads them.
async fn reopen_resolved_incidents(
    txn: &impl ConnectionTrait,
    service_id: i32,
) -> DomainResult<()> {
    let resolved_here: Vec<i32> = incident_service_link::Entity::find()
        .filter(incident_service_link::Column::ServiceRecordId.eq(service_id))
        .filter(incident_service_link::Column::Role.eq(incident_svc::ROLE_RESOLVED))
        .all(txn)
        .await?
        .into_iter()
        .map(|l| l.incident_id)
        .collect();
    if resolved_here.is_empty() {
        return Ok(());
    }
    let resolved_elsewhere: std::collections::HashSet<i32> = incident_service_link::Entity::find()
        .filter(incident_service_link::Column::IncidentId.is_in(resolved_here.clone()))
        .filter(incident_service_link::Column::Role.eq(incident_svc::ROLE_RESOLVED))
        .filter(incident_service_link::Column::ServiceRecordId.ne(service_id))
        .all(txn)
        .await?
        .into_iter()
        .map(|l| l.incident_id)
        .collect();
    let reopen: Vec<i32> = resolved_here
        .into_iter()
        .filter(|id| !resolved_elsewhere.contains(id))
        .collect();
    if reopen.is_empty() {
        return Ok(());
    }
    incident::Entity::update_many()
        .set(incident::ActiveModel {
            resolved: Set(false),
            updated_at: Set(super::now_stamp()),
            ..Default::default()
        })
        .filter(incident::Column::Id.is_in(reopen))
        .exec(txn)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inputs::{incident::NewIncident, part::NewPart},
        services::incident,
        test_support::{VehicleFixture, row_count, test_db},
    };

    async fn seed_vehicle(db: &impl ConnectionTrait) -> i32 {
        VehicleFixture::new().insert_id(db).await
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
                paid_by: None,
                payer_note: None,
                schedule_item_ids: Some(vec![sched_id]),
                part_ids: Some(vec![part_id]),
                line_items: Some(vec![NewLineItem {
                    paid_by: None,
                    description: "Synthetic oil".into(),
                    work_kind: "maintenance".into(),
                    quantity: Some(5.0),
                    unit_cost_cents: Some(1_000),
                    cost_cents: Some(5_000),
                }]),
                invoice_ref: None,
                end_date: None,
                performed_by: None,
                work_item_ids: None,
                resolved_incident_ids: None,
                noticed_incident_ids: None,
                recall_finding_ids: None,
                parts: None,
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
                paid_by: None,
                payer_note: None,
                schedule_item_ids: None,
                part_ids: Some(vec![part_a]),
                line_items: Some(vec![NewLineItem {
                    paid_by: None,
                    description: "old".into(),
                    work_kind: "maintenance".into(),
                    quantity: None,
                    unit_cost_cents: None,
                    cost_cents: Some(100),
                }]),
                invoice_ref: None,
                end_date: None,
                performed_by: None,
                work_item_ids: None,
                resolved_incident_ids: None,
                noticed_incident_ids: None,
                recall_finding_ids: None,
                parts: None,
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
                    paid_by: None,
                    description: "new".into(),
                    work_kind: "maintenance".into(),
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
                paid_by: None,
                payer_note: None,
                schedule_item_ids: Some(vec![sched_id]),
                part_ids: Some(vec![part_id]),
                line_items: None,
                invoice_ref: None,
                end_date: None,
                performed_by: None,
                work_item_ids: None,
                resolved_incident_ids: None,
                noticed_incident_ids: None,
                recall_finding_ids: None,
                parts: None,
            },
        )
        .await
        .unwrap();

        let kept = delete(&db, vid, created.record.id, None)
            .await
            .unwrap()
            .kept_part_ids;
        assert_eq!(kept, vec![part_id], "the kept parts are reported");

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
    async fn delete_clears_finding_links_and_removes_incident_links() {
        use crate::entities::incident;

        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc_id = service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set("2024-03-01".into()),
            paid_by: Set("self".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;

        // An incident linked to the record via the M2M link table.
        let inc = incident::ActiveModel {
            vehicle_id: Set(vid),
            category: Set("noise".into()),
            title: Set("Rattle".into()),
            occurred_at: Set("2024-02-01".into()),
            resolved: Set(false),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        incident_service_link::ActiveModel {
            incident_id: Set(inc.id),
            service_record_id: Set(svc_id),
            role: Set("resolved".into()),
        }
        .insert(&db)
        .await
        .unwrap();

        delete(&db, vid, svc_id, None).await.unwrap();

        // The incident link rows are gone.
        let links = incident_service_link::Entity::find()
            .filter(incident_service_link::Column::ServiceRecordId.eq(svc_id))
            .all(&db)
            .await
            .unwrap();
        assert!(links.is_empty());
        // The incident itself survives.
        assert!(
            incident::Entity::find_by_id(inc.id)
                .one(&db)
                .await
                .unwrap()
                .is_some()
        );
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

        delete(&db, vid, created.record.id, None).await.unwrap();

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
            paid_by: None,
            payer_note: None,
            schedule_item_ids,
            part_ids: None,
            line_items: None,
            invoice_ref: None,
            end_date: None,
            performed_by: None,
            work_item_ids: None,
            resolved_incident_ids: None,
            noticed_incident_ids: None,
            recall_finding_ids: None,
            parts: None,
        }
    }

    // ─── invoice_ref idempotency + possible_duplicates advisory (hwaf) ───

    // Recording the same invoice_ref twice returns the existing record
    // UNTOUCHED — no second row, and the second call's extra links/line-items
    // (and even a bogus schedule id, which the short-circuit skips guarding)
    // are ignored.
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
                    paid_by: None,
                    description: "should not be added".into(),
                    work_kind: "maintenance".into(),
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

    // Distinct invoice_refs are distinct records.
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

    // Regression: two records with the SAME day and SAME cost but NO
    // invoice_ref must BOTH persist — resemblance is never a dedup signal.
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

    // A ref-less create surfaces same-day peers as a soft advisory; the first
    // (no peers) is empty, the second sees the first.
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

    // A blank invoice_ref is normalized to absent: two ref-less records sent
    // with `Some("")`/whitespace must coexist, not collide on the unique
    // index. And a ref with surrounding whitespace is the same identity.
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

    // The partial unique index is a DB-level backstop: a raw second insert of
    // the same (vehicle_id, invoice_ref) — bypassing the service's
    // short-circuit — is rejected.
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
                // The message lists the valid values (whitelist shape).
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
    async fn a_repeated_schedule_item_id_links_once() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let item = seed_schedule_item(&db, vid, "Oil change").await;

        let created = create(&db, vid, minimal_record(Some(vec![item, item])))
            .await
            .unwrap();
        assert_eq!(created.schedule_item_ids, vec![item]);

        let updated = update(
            &db,
            vid,
            created.record.id,
            UpdateServiceRecord {
                schedule_item_ids: Some(vec![item, item]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.schedule_item_ids, vec![item]);
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
        use crate::entities::{maintenance_schedule_item, model_template, platform};
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
        let vid = VehicleFixture::new()
            .model_template_id(template_id)
            .insert_id(&db)
            .await;

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

    // --- invoice detail: who did the work, date range, line kinds, parts ---

    fn line(description: &str, work_kind: &str) -> NewLineItem {
        NewLineItem {
            paid_by: None,
            description: description.into(),
            work_kind: work_kind.into(),
            quantity: None,
            unit_cost_cents: None,
            cost_cents: None,
        }
    }

    #[tokio::test]
    async fn a_line_stores_its_own_payer_and_refuses_an_unknown_one() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let got = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: "2026-11-03".into(),
                line_items: Some(vec![
                    line("Oil change", "maintenance"),
                    NewLineItem {
                        paid_by: Some("manufacturer".into()),
                        ..line("Water pump under warranty", "repair")
                    },
                ]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let payers: Vec<Option<&str>> = got
            .line_items
            .iter()
            .map(|l| l.paid_by.as_deref())
            .collect();
        assert_eq!(payers, vec![None, Some("manufacturer")]);
        assert_eq!(effective_payer(None, "insurance"), "insurance");
        assert_eq!(effective_payer(Some("self"), "insurance"), "self");

        let err = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: "2026-11-04".into(),
                line_items: Some(vec![NewLineItem {
                    paid_by: Some("my neighbor".into()),
                    ..line("Oil change", "maintenance")
                }]),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "got: {err}");
    }

    fn inline_part(name: &str, supplied_by: &str) -> NewPart {
        NewPart {
            name: name.into(),
            supplied_by: Some(supplied_by.into()),
            ..Default::default()
        }
    }

    fn kinds(got: &ServiceRecordWithLinks) -> Vec<&str> {
        got.line_items
            .iter()
            .map(|l| l.work_kind.as_str())
            .collect()
    }

    #[tokio::test]
    async fn create_stores_who_did_it_the_date_range_and_line_kinds() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let got = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: "2025-03-24".into(),
                end_date: Some("2025-03-25".into()),
                performed_by: Some("shop".into()),
                line_items: Some(vec![
                    line("Replace suction jet pump", "repair"),
                    line("Intake upgrade", "mod"),
                ]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(got.record.performed_by, "shop");
        assert_eq!(got.record.end_date.as_deref(), Some("2025-03-25"));
        assert_eq!(kinds(&got), vec!["repair", "mod"]);
    }

    #[tokio::test]
    async fn create_rejects_an_end_date_before_the_start() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let err = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: "2025-03-25".into(),
                end_date: Some("2025-03-24".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "end_date"));
        assert_eq!(row_count::<service_record::Entity>(&db).await, 0);
    }

    #[tokio::test]
    async fn create_rejects_an_unknown_work_kind_and_performer() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let err = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: "2025-03-24".into(),
                line_items: Some(vec![line("Upgrade", "upgrade")]),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "work_kind"));

        let err = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: "2025-03-24".into(),
                performed_by: Some("dealer".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "performed_by"));
        assert_eq!(row_count::<service_record::Entity>(&db).await, 0);
    }

    #[tokio::test]
    async fn create_rejects_a_self_performed_service_with_a_shop_name() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let err = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: "2025-03-24".into(),
                performed_by: Some("self".into()),
                shop_name: Some("Example VW".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "shop_name"));
    }

    #[tokio::test]
    async fn inline_parts_are_installed_by_the_service_and_keep_their_supplier() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let got = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: "2025-03-24".into(),
                mileage: Some(52_747),
                parts: Some(vec![
                    NewPart {
                        seller: Some("FCP Euro".into()),
                        ..inline_part("Front brake pads", "self")
                    },
                    inline_part("Brake fluid", "shop"),
                ]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(got.parts.len(), 2);
        assert_eq!(got.part_ids.len(), 2);
        assert_eq!(got.parts[0].supplied_by, "self");
        assert_eq!(got.parts[0].installed_service_id, Some(got.record.id));
        assert_eq!(got.parts[0].installed_odometer, Some(52_747));
        assert_eq!(got.parts[0].installed_date.as_deref(), Some("2025-03-24"));
        assert_eq!(got.parts[0].status, "installed");
        assert_eq!(got.parts[1].supplied_by, "shop");
    }

    #[tokio::test]
    async fn refiling_an_invoice_number_returns_the_first_record_untouched() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let first = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: "2025-03-24".into(),
                invoice_ref: Some("07447".into()),
                line_items: Some(vec![line("Oil change", "maintenance")]),
                parts: Some(vec![inline_part("Oil filter", "shop")]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(!first.already_recorded);
        assert!(first.message.is_none());

        let second = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: "2025-03-24".into(),
                invoice_ref: Some("07447".into()),
                line_items: Some(vec![line("Other", "repair"), line("Another", "mod")]),
                parts: Some(vec![inline_part("A", "self"), inline_part("B", "self")]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(second.already_recorded);
        assert!(
            second
                .message
                .as_deref()
                .is_some_and(|m| m.contains(&format!("service {}", first.record.id))),
            "got: {:?}",
            second.message
        );
        assert_eq!(second.record.id, first.record.id);
        assert_eq!(row_count::<service_record_line_item::Entity>(&db).await, 1);
        assert_eq!(row_count::<part::Entity>(&db).await, 1);
    }

    #[tokio::test]
    async fn a_bad_part_supplier_rolls_back_the_whole_service() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let result = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: "2025-03-24".into(),
                mileage: Some(50_000),
                line_items: Some(vec![line("Oil change", "maintenance")]),
                parts: Some(vec![inline_part("Filter", "dealer")]),
                ..Default::default()
            },
        )
        .await;
        assert!(matches!(
            result,
            Err(DomainError::Invalid { ref field, .. }) if field == "supplied_by"
        ));
        assert_eq!(row_count::<service_record::Entity>(&db).await, 0);
        assert_eq!(row_count::<service_record_line_item::Entity>(&db).await, 0);
        assert_eq!(row_count::<part::Entity>(&db).await, 0);
        assert_eq!(row_count::<mileage_log::Entity>(&db).await, 0);
    }

    #[tokio::test]
    async fn a_self_performed_service_records_an_ecu_flash() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let got = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: "2025-03-24".into(),
                performed_by: Some("self".into()),
                line_items: Some(vec![line("IE Stage 1 tune flash", "mod")]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(got.record.performed_by, "self");
        assert!(got.record.shop_name.is_none());
    }

    #[tokio::test]
    async fn update_changes_performer_and_end_date_and_validates_them() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let made = create(&db, vid, minimal_record(None)).await.unwrap();

        let updated = update(
            &db,
            vid,
            made.record.id,
            UpdateServiceRecord {
                performed_by: Some("self".into()),
                end_date: Some(Some("2024-03-02".into())),
                line_items: Some(vec![line("Flash", "mod")]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.record.performed_by, "self");
        assert_eq!(updated.record.end_date.as_deref(), Some("2024-03-02"));
        assert_eq!(kinds(&updated), vec!["mod"]);

        let err = update(
            &db,
            vid,
            made.record.id,
            UpdateServiceRecord {
                end_date: Some(Some("2024-02-01".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "end_date"));

        let cleared = update(
            &db,
            vid,
            made.record.id,
            UpdateServiceRecord {
                end_date: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(cleared.record.end_date.is_none());
    }
    // --- incident links ---------------------------------------------------

    async fn service_dated(db: &DatabaseConnection, vehicle_id: i32, date: &str) -> i32 {
        create(
            db,
            vehicle_id,
            NewServiceRecord {
                service_date: date.into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .record
        .id
    }

    async fn observation(
        db: &DatabaseConnection,
        vehicle_id: i32,
        noticed_during: Option<i32>,
    ) -> i32 {
        incident::create(
            db,
            vehicle_id,
            NewIncident {
                category: "shop_observation".into(),
                title: "N80 purge valve failing".into(),
                noticed_during_service_id: noticed_during,
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .incident
        .id
    }

    #[tokio::test]
    async fn a_shop_observation_links_the_service_that_noticed_it_and_the_one_that_fixed_it() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let svc_2023 = service_dated(&db, v, "2023-05-03").await;
        let incident_id = observation(&db, v, Some(svc_2023)).await;
        let svc_2024 = create(
            &db,
            v,
            NewServiceRecord {
                service_date: "2024-06-12".into(),
                resolved_incident_ids: Some(vec![incident_id]),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .record
        .id;

        let s23 = get(&db, v, svc_2023).await.unwrap();
        assert_eq!(s23.noticed_incidents[0].title, "N80 purge valve failing");
        assert!(s23.noticed_incidents[0].resolved);
        assert_eq!(
            s23.noticed_incidents[0]
                .other_service
                .as_ref()
                .unwrap()
                .service_date,
            "2024-06-12"
        );
        let s24 = get(&db, v, svc_2024).await.unwrap();
        assert_eq!(
            s24.resolved_incidents[0]
                .other_service
                .as_ref()
                .unwrap()
                .service_date,
            "2023-05-03"
        );
        let inc = incident::get(&db, v, incident_id).await.unwrap();
        assert_eq!(inc.noticed_during.unwrap().id, svc_2023);
        assert_eq!(inc.resolved_by[0].id, svc_2024);

        // The list path carries the same links.
        let listed = list(&db, v).await.unwrap();
        let listed_23 = listed.iter().find(|s| s.record.id == svc_2023).unwrap();
        assert_eq!(listed_23.noticed_incidents.len(), 1);
        let listed_24 = listed.iter().find(|s| s.record.id == svc_2024).unwrap();
        assert_eq!(listed_24.resolved_incidents.len(), 1);
    }

    #[tokio::test]
    async fn an_unresolved_observation_shows_as_open_on_the_service_that_noticed_it() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let svc_2023 = service_dated(&db, v, "2023-05-03").await;
        observation(&db, v, Some(svc_2023)).await;

        let s23 = get(&db, v, svc_2023).await.unwrap();
        assert!(!s23.noticed_incidents[0].resolved);
        assert!(s23.noticed_incidents[0].other_service.is_none());
    }

    #[tokio::test]
    async fn recording_a_service_can_note_what_it_noticed() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let incident_id = observation(&db, v, None).await;
        let svc = create(
            &db,
            v,
            NewServiceRecord {
                service_date: "2023-05-03".into(),
                noticed_incident_ids: Some(vec![incident_id]),
                recall_finding_ids: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(svc.noticed_incidents[0].id, incident_id);
        let inc = incident::get(&db, v, incident_id).await.unwrap();
        assert_eq!(inc.noticed_during.unwrap().id, svc.record.id);
        assert!(!inc.incident.resolved);

        // An incident is noticed during at most one service.
        let err = create(
            &db,
            v,
            NewServiceRecord {
                service_date: "2023-06-01".into(),
                noticed_incident_ids: Some(vec![incident_id]),
                recall_finding_ids: None,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "got {err:?}");
        assert_eq!(row_count::<service_record::Entity>(&db).await, 1);
    }

    #[tokio::test]
    async fn completing_an_incident_sourced_work_item_resolves_the_incident() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let svc_2023 = service_dated(&db, v, "2023-05-03").await;
        let incident_id = observation(&db, v, Some(svc_2023)).await;
        let planned = work_item::create(
            &db,
            v,
            crate::inputs::work_item::NewWorkItem {
                title: "Replace purge valve".into(),
                notes: None,
                schedule_item_id: None,
                research_finding_id: None,
                incident_id: Some(incident_id),
                est_cost_cents: None,
                revisit_by_date: None,
                revisit_by_miles: None,
            },
        )
        .await
        .unwrap();

        // Naming the incident as well as the work item must not double-link.
        let svc_2024 = create(
            &db,
            v,
            NewServiceRecord {
                service_date: "2024-06-12".into(),
                work_item_ids: Some(vec![planned.id]),
                resolved_incident_ids: Some(vec![incident_id]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(svc_2024.resolved_incidents.len(), 1);
        let inc = incident::get(&db, v, incident_id).await.unwrap();
        assert!(inc.incident.resolved);
        assert_eq!(inc.resolved_by.len(), 1);
        assert_eq!(inc.resolved_by[0].id, svc_2024.record.id);

        // The service names the to-do it completed, when created and when read
        // back alone or in a list.
        let listed = list(&db, v).await.unwrap();
        let read = get(&db, v, svc_2024.record.id).await.unwrap();
        for got in [&svc_2024, &read, &listed[0]] {
            assert_eq!(got.completed_work_items.len(), 1);
            assert_eq!(got.completed_work_items[0].id, planned.id);
            assert_eq!(got.completed_work_items[0].title, "Replace purge valve");
        }
        assert!(listed[1].completed_work_items.is_empty());
    }

    #[tokio::test]
    async fn incident_and_service_ids_from_another_vehicle_are_not_found() {
        let db = test_db().await;
        let v1 = seed_vehicle(&db).await;
        let v2 = seed_vehicle(&db).await;
        let foreign_incident = observation(&db, v2, None).await;
        let foreign_service = service_dated(&db, v2, "2023-05-03").await;
        let services_before = row_count::<service_record::Entity>(&db).await;

        // A foreign noticed_during_service_id on create reads like a missing one.
        let wrong = incident::create(
            &db,
            v1,
            NewIncident {
                noticed_during_service_id: Some(foreign_service),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        let missing = incident::create(
            &db,
            v1,
            NewIncident {
                noticed_during_service_id: Some(9999),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(wrong, DomainError::NotFound(_)));
        assert_eq!(
            wrong.to_string().replace(&foreign_service.to_string(), "N"),
            missing.to_string().replace("9999", "N")
        );
        assert!(incident::list(&db, v1).await.unwrap().is_empty());

        // Foreign incident ids on record_service leave no footprint at all.
        let sched = seed_schedule_item(&db, v1, "Oil change").await;
        let full = |as_resolved: bool, id: i32| {
            let ids = Some(vec![id]);
            NewServiceRecord {
                service_date: "2024-06-12".into(),
                mileage: Some(50_000),
                line_items: Some(vec![line("Oil", "maintenance")]),
                parts: Some(vec![NewPart {
                    name: "Filter".into(),
                    ..Default::default()
                }]),
                schedule_item_ids: Some(vec![sched]),
                resolved_incident_ids: if as_resolved { ids.clone() } else { None },
                noticed_incident_ids: if as_resolved { None } else { ids },
                recall_finding_ids: None,
                ..Default::default()
            }
        };
        for as_resolved in [true, false] {
            let err = create(&db, v1, full(as_resolved, foreign_incident))
                .await
                .unwrap_err();
            let gone = create(&db, v1, full(as_resolved, 9999)).await.unwrap_err();
            assert!(matches!(err, DomainError::NotFound(_)));
            assert_eq!(
                err.to_string().replace(&foreign_incident.to_string(), "N"),
                gone.to_string().replace("9999", "N")
            );
            assert_eq!(
                row_count::<service_record::Entity>(&db).await,
                services_before
            );
            assert_eq!(row_count::<service_record_line_item::Entity>(&db).await, 0);
            assert_eq!(row_count::<part::Entity>(&db).await, 0);
            assert_eq!(row_count::<mileage_log::Entity>(&db).await, 0);
            assert_eq!(row_count::<service_schedule_link::Entity>(&db).await, 0);
            assert_eq!(row_count::<incident_service_link::Entity>(&db).await, 0);
        }
        assert!(
            !incident::get(&db, v2, foreign_incident)
                .await
                .unwrap()
                .incident
                .resolved
        );
    }

    // --- deleting a service reopens what it closed --------------------------

    #[tokio::test]
    async fn deleting_a_service_reopens_what_it_closed() {
        use crate::{
            inputs::{service_record::NewServiceRecord, work_item::NewWorkItem},
            services::{recall, research, work_item as work_item_svc},
        };

        let db = test_db().await;
        let v = seed_vehicle(&db).await;

        let earlier = service_dated(&db, v, "2023-01-10").await;
        let incident_a = observation(&db, v, None).await;
        let incident_b = observation(&db, v, None).await;
        incident::update(
            &db,
            v,
            incident_b,
            crate::inputs::incident::UpdateIncident {
                resolved_by_service_ids: Some(vec![earlier]),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let finding_id = research::record_recalls(
            &db,
            v,
            &[crate::services::nhtsa::RecallInfo {
                campaign_number: "24V123".into(),
                manufacturer: None,
                title: "Pump".into(),
                summary: None,
                consequence: None,
                remedy: None,
                report_date: None,
                component: None,
                action_number: None,
            }],
        )
        .await
        .unwrap()
        .recalls[0]
            .finding_id;
        let work_item = work_item_svc::create(
            &db,
            v,
            NewWorkItem {
                title: "Do the thing".into(),
                notes: None,
                schedule_item_id: None,
                research_finding_id: None,
                incident_id: None,
                est_cost_cents: None,
                revisit_by_date: None,
                revisit_by_miles: None,
            },
        )
        .await
        .unwrap();

        let closing = create(
            &db,
            v,
            NewServiceRecord {
                service_date: "2024-06-01".into(),
                work_item_ids: Some(vec![work_item.id]),
                resolved_incident_ids: Some(vec![incident_a, incident_b]),
                recall_finding_ids: Some(vec![finding_id]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let work_item_before = work_item_svc::get(&db, v, work_item.id).await.unwrap();
        assert_eq!(work_item_before.status, "done");
        assert_eq!(recall::list(&db, v).await.unwrap()[0].status, "done");

        delete(&db, v, closing.record.id, Some(RecallsOnDelete::Reopen))
            .await
            .unwrap();

        let work_item_after = work_item_svc::get(&db, v, work_item.id).await.unwrap();
        assert_eq!(work_item_after.status, "planned");
        assert_eq!(work_item_after.completed_service_id, None);
        let incident_a_after = incident::get(&db, v, incident_a).await.unwrap();
        let incident_b_after = incident::get(&db, v, incident_b).await.unwrap();
        assert!(!incident_a_after.incident.resolved);
        assert!(incident_b_after.incident.resolved);
        assert_eq!(recall::list(&db, v).await.unwrap()[0].status, "open");
    }

    async fn seed_recall(db: &DatabaseConnection, v: i32, campaign: &str) -> i32 {
        crate::services::research::record_recalls(
            db,
            v,
            &[crate::services::nhtsa::RecallInfo {
                campaign_number: campaign.into(),
                manufacturer: None,
                title: "Pump".into(),
                summary: None,
                consequence: None,
                remedy: None,
                report_date: None,
                component: None,
                action_number: None,
            }],
        )
        .await
        .unwrap()
        .recalls[0]
            .finding_id
    }

    async fn service_with_recall(db: &DatabaseConnection, v: i32, finding_id: i32) -> i32 {
        create(
            db,
            v,
            NewServiceRecord {
                service_date: "2024-06-01".into(),
                recall_finding_ids: Some(vec![finding_id]),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .record
        .id
    }

    async fn recall_status(db: &DatabaseConnection, v: i32) -> Vec<(String, Option<i32>)> {
        crate::services::recall::list(db, v)
            .await
            .unwrap()
            .into_iter()
            .map(|r| (r.status, r.done_service.map(|s| s.id)))
            .collect()
    }

    #[tokio::test]
    async fn deleting_a_service_with_recalls_and_no_choice_changes_nothing() {
        use crate::{inputs::work_item::NewWorkItem, services::work_item as work_item_svc};

        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let finding_id = seed_recall(&db, v, "24V123").await;
        let incident_id = observation(&db, v, None).await;
        let work_item = work_item_svc::create(
            &db,
            v,
            NewWorkItem {
                title: "Do the thing".into(),
                notes: None,
                schedule_item_id: None,
                research_finding_id: None,
                incident_id: None,
                est_cost_cents: None,
                revisit_by_date: None,
                revisit_by_miles: None,
            },
        )
        .await
        .unwrap();
        let svc = create(
            &db,
            v,
            NewServiceRecord {
                service_date: "2024-06-01".into(),
                recall_finding_ids: Some(vec![finding_id]),
                work_item_ids: Some(vec![work_item.id]),
                resolved_incident_ids: Some(vec![incident_id]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let id = svc.record.id;

        let err = delete(&db, v, id, None).await.unwrap_err();

        let DomainError::BadRequest(message) = err else {
            panic!("expected BadRequest, got {err:?}");
        };
        assert!(message.contains(&finding_id.to_string()), "{message}");
        assert!(message.contains("24V123"), "{message}");
        assert!(message.contains("Pump"), "{message}");
        assert!(message.contains("reopen"), "{message}");
        assert!(message.contains("keep_done"), "{message}");
        assert!(get(&db, v, id).await.is_ok());
        assert_eq!(recall_status(&db, v).await, vec![("done".into(), Some(id))]);
        assert_eq!(
            work_item_svc::get(&db, v, work_item.id)
                .await
                .unwrap()
                .status,
            "done"
        );
        assert!(
            incident::get(&db, v, incident_id)
                .await
                .unwrap()
                .incident
                .resolved
        );
    }

    #[tokio::test]
    async fn reopen_makes_a_performed_recall_open_even_if_the_owner_marked_it_done() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let finding_id = seed_recall(&db, v, "24V999").await;
        let svc = service_with_recall(&db, v, finding_id).await;
        crate::services::recall::set_status(&db, v, finding_id, "done")
            .await
            .unwrap();

        let out = delete(&db, v, svc, Some(RecallsOnDelete::Reopen))
            .await
            .unwrap();

        assert_eq!(out.reopened_recall_ids, vec![finding_id]);
        assert!(out.kept_done_recall_ids.is_empty());
        assert_eq!(recall_status(&db, v).await, vec![("open".into(), None)]);
    }

    #[tokio::test]
    async fn keep_done_leaves_a_performed_recall_done_with_no_service() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let finding_id = seed_recall(&db, v, "24V999").await;
        let svc = service_with_recall(&db, v, finding_id).await;

        let out = delete(&db, v, svc, Some(RecallsOnDelete::KeepDone))
            .await
            .unwrap();

        assert_eq!(out.kept_done_recall_ids, vec![finding_id]);
        assert!(out.reopened_recall_ids.is_empty());
        assert_eq!(recall_status(&db, v).await, vec![("done".into(), None)]);
    }

    #[tokio::test]
    async fn a_service_without_recalls_deletes_with_no_choice() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let svc = service_dated(&db, v, "2024-06-01").await;

        let out = delete(&db, v, svc, None).await.unwrap();

        assert!(out.reopened_recall_ids.is_empty() && out.kept_done_recall_ids.is_empty());
        assert!(get(&db, v, svc).await.is_err());
    }

    #[tokio::test]
    async fn reopen_leaves_a_recall_done_through_another_service() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let finding_id = seed_recall(&db, v, "24V999").await;
        let first = service_with_recall(&db, v, finding_id).await;
        let second = service_with_recall(&db, v, finding_id).await;

        let out = delete(&db, v, first, Some(RecallsOnDelete::Reopen))
            .await
            .unwrap();

        assert!(out.reopened_recall_ids.is_empty());
        assert_eq!(out.still_done_elsewhere_recall_ids, vec![finding_id]);
        assert_eq!(
            recall_status(&db, v).await,
            vec![("done".into(), Some(second))]
        );
    }

    #[tokio::test]
    async fn deleting_a_service_keeps_its_inline_parts_as_purchased() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let svc = create(
            &db,
            v,
            NewServiceRecord {
                service_date: "2024-06-01".into(),
                mileage: Some(40_000),
                parts: Some(vec![NewPart {
                    name: "Brake pads".into(),
                    ..Default::default()
                }]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let part_id = svc.parts[0].id;

        delete(&db, v, svc.record.id, None).await.unwrap();

        let kept = part_svc::get(&db, v, part_id).await.unwrap();
        assert_eq!(kept.status, "purchased");
        assert_eq!(kept.installed_service_id, None);
    }

    #[tokio::test]
    async fn correcting_the_date_or_mileage_moves_the_install_of_its_parts() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let svc = create(
            &db,
            v,
            NewServiceRecord {
                service_date: "2024-03-01".into(),
                mileage: Some(50_000),
                parts: Some(vec![inline_part("Pads", "shop")]),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let corrected = update(
            &db,
            v,
            svc.record.id,
            UpdateServiceRecord {
                service_date: Some("2024-03-10".into()),
                mileage: Some(Some(50_500)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            corrected.parts[0].installed_date.as_deref(),
            Some("2024-03-10")
        );
        assert_eq!(corrected.parts[0].installed_odometer, Some(50_500));

        // Clearing the service's mileage leaves the part its own reading.
        let cleared = update(
            &db,
            v,
            svc.record.id,
            UpdateServiceRecord {
                mileage: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.parts[0].installed_odometer, Some(50_500));
        assert_eq!(
            cleared.parts[0].installed_date.as_deref(),
            Some("2024-03-10")
        );
    }

    #[tokio::test]
    async fn list_between_keeps_only_services_inside_the_bounds() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        for date in ["2023-01-05", "2024-03-01", "2025-07-09"] {
            service_dated(&db, v, date).await;
        }
        let dates = |rows: Vec<ServiceRecordWithLinks>| {
            rows.into_iter()
                .map(|r| r.record.service_date)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            dates(
                list_between(&db, v, Some("2024-01-01"), None)
                    .await
                    .unwrap()
            ),
            vec!["2025-07-09", "2024-03-01"]
        );
        assert_eq!(
            dates(
                list_between(&db, v, None, Some("2024-03-01"))
                    .await
                    .unwrap()
            ),
            vec!["2024-03-01", "2023-01-05"]
        );
        assert_eq!(list_between(&db, v, None, None).await.unwrap().len(), 3);
    }
}
