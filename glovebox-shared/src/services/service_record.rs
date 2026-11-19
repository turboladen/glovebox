use sea_orm::*;
use serde::Serialize;

use crate::dates::DayOrTime;

use crate::{
    entities::{
        incident::{self, IncidentCategory},
        incident_service_link::{self, LinkRole},
        job_need, mileage_log,
        part::{self, PartStatus},
        service_recall_link,
        service_record::{self, Payer, Performer},
        service_record_line_item, service_schedule_link,
        work_item::{self as job, JobStatus},
    },
    error::{DomainError, DomainResult},
    inputs::{
        part::{Install, NewFitting, NewPart},
        service_record::{NewLineItem, NewServiceRecord, UpdateServiceRecord},
    },
    services::{
        appointment,
        incident::{self as incident_svc, ServiceRef},
        need, part as part_svc,
        recall::{self, RecallRef},
        schedule,
        work_item::{self, WorkItemRef},
    },
};

/// Checks the date range runs forward and a self-performed job has no shop.
fn validate_who_and_when(
    service_date: chrono::NaiveDate,
    end_date: Option<chrono::NaiveDate>,
    performed_by: Performer,
    shop_name: Option<&str>,
) -> DomainResult<()> {
    if end_date.is_some_and(|end| end < service_date) {
        return Err(DomainError::invalid(
            "end_date",
            "must not be before service_date",
        ));
    }
    if performed_by == Performer::Owner && shop_name.is_some() {
        return Err(DomainError::invalid(
            "shop_name",
            "a self-performed service has no shop",
        ));
    }
    Ok(())
}

/// Who paid for a line: its own `paid_by` when it has one, otherwise the
/// service's.
pub fn effective_payer(line_paid_by: Option<Payer>, service_paid_by: Payer) -> Payer {
    line_paid_by.unwrap_or(service_paid_by)
}

#[derive(Debug, Serialize)]
pub struct ServiceRecordWithLinks {
    #[serde(flatten)]
    pub record: service_record::Model,
    pub schedule_item_ids: Vec<i32>,
    pub part_ids: Vec<i32>,
    /// The full rows behind `part_ids`, each with its kit contents.
    pub parts: Vec<part_svc::PartView>,
    pub line_items: Vec<LineItemView>,
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
    /// Planned jobs whose quote came with this service, such as an estimate
    /// on the invoice for work not done. Omitted from JSON when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub quoted_work_items: Vec<WorkItemRef>,
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
    /// Needs on the jobs this service completed whose part is linked but
    /// neither installed nor replaced, so the caller can ask whether each was
    /// fitted. Filled only
    /// by `create` and `update`; empty (and omitted from JSON) otherwise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unfitted_needs: Vec<UnfittedNeed>,
    /// Ids of the hand-closed appointments that are open again because a
    /// correction took one of their jobs off this service as work that did
    /// not happen. Filled only by `update`; omitted from JSON when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reopened_appointment_ids: Vec<i32>,
    /// Ids of the jobs such a correction took off a cancelled appointment.
    /// Filled only by `update`; omitted from JSON when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unassigned_job_ids: Vec<i32>,
}

/// A need on a completed job whose linked part is neither installed nor
/// replaced.
#[derive(Clone, Debug, Serialize)]
pub struct UnfittedNeed {
    pub need_id: i32,
    pub name: String,
    pub job_title: String,
    pub part_id: i32,
}

/// A line item with the payer it resolves to.
#[derive(Clone, Debug, Serialize)]
pub struct LineItemView {
    #[serde(flatten)]
    pub line: service_record_line_item::Model,
    /// The line's own `paid_by` when it has one, otherwise the service's.
    /// `paid_by` itself stays null for a line that follows its service.
    pub effective_paid_by: Payer,
}

impl std::ops::Deref for LineItemView {
    type Target = service_record_line_item::Model;

    fn deref(&self) -> &Self::Target {
        &self.line
    }
}

/// An incident as seen from a service record.
#[derive(Clone, Debug, Serialize)]
pub struct IncidentRef {
    pub id: i32,
    pub title: String,
    pub category: IncidentCategory,
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
    let other_for = |incident_id: i32, role: LinkRole| -> Option<ServiceRef> {
        all_links
            .iter()
            .filter(|l| l.incident_id == incident_id && l.role == role)
            .filter_map(|l| services.get(&l.service_record_id))
            .max_by_key(|s| (s.service_date, s.id))
            .cloned()
            .map(ServiceRef::from)
    };

    for link in &mine {
        let Some(inc) = incidents.get(&link.incident_id) else {
            continue;
        };
        let (bucket, other_role) = match link.role {
            LinkRole::Noticed => (&mut out.noticed, LinkRole::Resolved),
            LinkRole::Resolved => (&mut out.resolved, LinkRole::Noticed),
        };
        bucket
            .entry(link.service_record_id)
            .or_default()
            .push(IncidentRef {
                id: inc.id,
                title: inc.title.clone(),
                category: inc.category,
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
    pub service_date: chrono::NaiveDate,
    pub description: Option<String>,
    pub total_cost_cents: Option<i32>,
}

impl ServiceRecordWithLinks {
    fn assemble(
        record: service_record::Model,
        schedule_item_ids: Vec<i32>,
        parts: Vec<part_svc::PartView>,
        line_items: Vec<service_record_line_item::Model>,
        noticed_incidents: Vec<IncidentRef>,
        resolved_incidents: Vec<IncidentRef>,
        performed_recalls: Vec<RecallRef>,
    ) -> Self {
        let line_items: Vec<LineItemView> = line_items
            .into_iter()
            .map(|line| LineItemView {
                effective_paid_by: effective_payer(line.paid_by, record.paid_by),
                line,
            })
            .collect();
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
            part_ids: parts.iter().map(|p| p.part.id).collect(),
            parts,
            line_items,
            noticed_incidents,
            resolved_incidents,
            performed_recalls,
            completed_work_items: Vec::new(),
            quoted_work_items: Vec::new(),
            already_recorded: false,
            message: None,
            possible_duplicates: Vec::new(),
            unfitted_needs: Vec::new(),
            reopened_appointment_ids: Vec::new(),
            unassigned_job_ids: Vec::new(),
        }
    }
}

/// The needs on `jobs` whose linked part has not been fitted (it is neither
/// installed nor replaced), in need order, with two batched queries.
async fn unfitted_needs(
    db: &impl ConnectionTrait,
    jobs: &[WorkItemRef],
) -> DomainResult<Vec<UnfittedNeed>> {
    if jobs.is_empty() {
        return Ok(Vec::new());
    }
    let needs = job_need::Entity::find()
        .filter(job_need::Column::WorkItemId.is_in(jobs.iter().map(|j| j.id)))
        .filter(job_need::Column::PartId.is_not_null())
        .order_by_asc(job_need::Column::Id)
        .all(db)
        .await?;
    let parts: std::collections::HashMap<i32, part::Model> = part::Entity::find()
        .filter(part::Column::Id.is_in(needs.iter().filter_map(|n| n.part_id)))
        .all(db)
        .await?
        .into_iter()
        .map(|p| (p.id, p))
        .collect();
    let titles: std::collections::HashMap<i32, &str> =
        jobs.iter().map(|j| (j.id, j.title.as_str())).collect();
    Ok(needs
        .into_iter()
        .filter_map(|n| {
            let part_id = n.part_id?;
            if part_svc::was_fitted(parts.get(&part_id)?) {
                return None;
            }
            Some(UnfittedNeed {
                need_id: n.id,
                name: n.name,
                job_title: (*titles.get(&n.work_item_id)?).to_owned(),
                part_id,
            })
        })
        .collect())
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
    let parts = part_svc::views(db, parts).await?;

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

    let mut jobs = work_item::load_by_service(db, &[record.id]).await?;

    let mut links = ServiceRecordWithLinks::assemble(
        record,
        schedule_item_ids,
        parts,
        line_items,
        noticed,
        resolved,
        performed_recalls,
    );
    links.completed_work_items = jobs.completed.remove(&links.record.id).unwrap_or_default();
    links.quoted_work_items = jobs.quoted.remove(&links.record.id).unwrap_or_default();
    Ok(links)
}

/// Insert line items for a service record within a transaction.
async fn insert_line_items(
    txn: &impl ConnectionTrait,
    record_id: i32,
    items: Vec<NewLineItem>,
) -> DomainResult<()> {
    // A bulk insert does not pass through the entity's `before_save`, so the
    // stamps are set here. An empty batch inserts nothing.
    let now = super::now();
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
            created_at: Set(now),
            updated_at: Set(now),
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
        recorded_at: Set(record.service_date.into()),
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
            if log.mileage != miles || log.recorded_at != DayOrTime::Day(record.service_date) {
                let mut active_log: mileage_log::ActiveModel = log.into();
                active_log.mileage = Set(miles);
                active_log.recorded_at = Set(record.service_date.into());
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
/// falls on or after `from` and on or before `to` (either bound may be
/// omitted).
pub async fn list_between(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    from: Option<chrono::NaiveDate>,
    to: Option<chrono::NaiveDate>,
) -> DomainResult<Vec<ServiceRecordWithLinks>> {
    super::vehicle::require(db, vehicle_id).await?;
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
        let parts = part::Entity::find()
            .filter(part::Column::InstalledServiceId.is_in(record_ids.clone()))
            .order_by_asc(part::Column::Id)
            .all(db)
            .await?;
        part_svc::views(db, parts).await?
    };

    let mut parts_by_service: std::collections::HashMap<i32, Vec<part_svc::PartView>> =
        std::collections::HashMap::new();
    for p in all_parts {
        if let Some(service_id) = p.part.installed_service_id {
            parts_by_service.entry(service_id).or_default().push(p);
        }
    }
    let mut incident_links = load_incident_links(db, &record_ids).await?;
    let mut performed = recall::load_performed(db, &record_ids).await?;
    let mut jobs = work_item::load_by_service(db, &record_ids).await?;

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
            let parts = parts_by_service.remove(&record.id).unwrap_or_default();
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
            let completed_work_items = jobs.completed.remove(&record.id).unwrap_or_default();
            let quoted_work_items = jobs.quoted.remove(&record.id).unwrap_or_default();
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
            links.quoted_work_items = quoted_work_items;
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
/// invoice lists, already installed. A listed part that names a need is
/// linked to it with the checks of `need::link_part`.
async fn install_parts(
    txn: &(impl ConnectionTrait + TransactionTrait),
    vehicle_id: i32,
    record: &service_record::Model,
    existing_ids: Vec<i32>,
    new_parts: Vec<NewPart>,
) -> DomainResult<()> {
    // A record with no odometer reading leaves each part's own reading.
    let odometer = record.mileage.map(Some);
    for part_id in existing_ids {
        part_svc::install(txn, vehicle_id, part_id, record, odometer).await?;
    }

    for (n, mut new_part) in (1..).zip(new_parts) {
        // A kit item's error names its part, because an invoice can list
        // several kits and each counts its items from 1.
        part_svc::validate_contents("parts", &new_part.contents).map_err(|e| match e {
            DomainError::Invalid { field, message } => {
                DomainError::invalid(field, format!("part {n}'s {message}"))
            }
            other => other,
        })?;
        new_part.fitting = NewFitting::Installed(Install {
            service_id: Some(record.id),
            date: Some(record.service_date),
            odometer: record.mileage,
        });
        let need_id = new_part.need_id;
        let created = part_svc::create(txn, vehicle_id, new_part).await?;
        if let Some(need_id) = need_id {
            need::link_part(txn, vehicle_id, need_id, created.id, None).await?;
        }
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
        .filter(service_record::Column::ServiceDate.eq(record.service_date))
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
    for (ids, role) in [
        (resolved_ids, LinkRole::Resolved),
        (noticed_ids, LinkRole::Noticed),
    ] {
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
#[allow(clippy::too_many_lines)]
pub async fn create<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    input: NewServiceRecord,
) -> DomainResult<ServiceRecordWithLinks> {
    let pool = db;
    let txn = super::begin_write(pool).await?;
    let db = &txn;
    super::vehicle::require(db, vehicle_id).await?;

    // An LLM may send "" or spaces rather than omitting the invoice number, and a
    // blank ref stored as a real one would make the partial unique index reject a
    // second ref-less record as a duplicate.
    let invoice_ref = input.invoice_ref.as_deref().and_then(super::trim_to_none);

    // Hard-idempotency: an invoice_ref is a deterministic identity signal.
    // If this vehicle already has a record with the same ref, return it
    // UNTOUCHED: no new record, no new links, parts or line items from this
    // call. The short-circuit runs before any other check or write in this
    // function; the dates in `input` are typed, so the caller has already
    // parsed them. The partial unique index is the backstop against a race.
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

    let performed_by = input.performed_by.unwrap_or(Performer::Shop);
    validate_who_and_when(
        input.service_date,
        input.end_date,
        performed_by,
        input.shop_name.as_deref(),
    )?;
    let line_items_input = input.line_items.unwrap_or_default();
    if let Some(appointment_id) = input.appointment_id {
        appointment::require_assignable(db, vehicle_id, appointment_id).await?;
    }
    super::shop::require_if_named(db, input.shop_id).await?;

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
        service_date: Set(input.service_date),
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
        paid_by: Set(input.paid_by.unwrap_or(Payer::Owner)),
        payer_note: Set(input.payer_note),
        invoice_ref: Set(invoice_ref),
        appointment_id: Set(input.appointment_id),
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
    created.unfitted_needs = unfitted_needs(&txn, &created.completed_work_items).await?;

    txn.commit().await?;

    // Soft advisory: with no invoice_ref there is no deterministic identity
    // signal, so surface other same-day records on this vehicle for the caller
    // to reconcile. Read-only, never blocks; distinct records coexist.
    if created.record.invoice_ref.is_none() {
        created.possible_duplicates = same_day_peers(pool, vehicle_id, &created.record).await?;
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
    let txn = super::begin_write(db).await?;
    let db = &txn;
    let existing = require(db, vehicle_id, id).await?;
    // What the correction takes off the service is known before anything is
    // written, so a refusal names the work it would change and writes nothing.
    let recorded = if input.work_item_ids.is_some()
        || input.part_ids.is_some()
        || input.recall_finding_ids.is_some()
    {
        RecordedWork::read(db, existing.id).await?
    } else {
        RecordedWork::default()
    };
    // The ids the lists name are checked first, so a wrong one is NotFound
    // before the owner is asked anything.
    let mut listed_jobs = Vec::new();
    for &job_id in input.work_item_ids.iter().flatten() {
        listed_jobs.push(work_item::get(db, vehicle_id, job_id).await?);
    }
    for &part_id in input.part_ids.iter().flatten() {
        part_svc::get(db, vehicle_id, part_id).await?;
    }
    if let Some(finding_ids) = &input.recall_finding_ids {
        recall::require_recalls(db, vehicle_id, &super::dedup_ids(finding_ids)).await?;
    }
    let completed_before = recorded.job_ids();
    let removal = recorded.left_out_by(db, &input, &listed_jobs).await?;
    let choice = removal.choice(existing.id, ChoiceFor::Correction, input.completed_work)?;

    validate_who_and_when(
        input.service_date.unwrap_or(existing.service_date),
        input.end_date.unwrap_or(existing.end_date),
        input.performed_by.unwrap_or(existing.performed_by),
        input
            .shop_name
            .as_ref()
            .unwrap_or(&existing.shop_name)
            .as_deref(),
    )?;
    let invoice_ref = input
        .invoice_ref
        .map(|r| r.as_deref().and_then(super::trim_to_none));
    if let Some(Some(invoice_ref)) = &invoice_ref {
        require_invoice_ref_free(db, vehicle_id, id, invoice_ref).await?;
    }
    // Keeping the appointment a service already names is not a new
    // assignment, so it stays allowed after the appointment closes.
    if let Some(Some(appointment_id)) = input.appointment_id
        && existing.appointment_id != Some(appointment_id)
    {
        appointment::require_assignable(db, vehicle_id, appointment_id).await?;
    }
    super::shop::require_if_named(db, input.shop_id.flatten()).await?;

    let (date_before, mileage_before) = (existing.service_date, existing.mileage);
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
        active.paid_by = Set(v);
    }
    if let Some(v) = input.payer_note {
        active.payer_note = Set(v);
    }
    if let Some(v) = input.appointment_id {
        active.appointment_id = Set(v);
    }

    active.updated_at = Set(super::now());

    let record = active.update(&txn).await?;

    // Keep the auto-created mileage log in step with the record.
    sync_auto_mileage_log(&txn, vehicle_id, &record).await?;

    // Jobs come first, with the work that hangs off each one; the lists the
    // call sends then apply as sent.
    let settled = work_item::release_completed(&txn, &removal.job_ids(), choice).await?;
    let added = match &input.work_item_ids {
        Some(job_ids) => {
            let added: Vec<i32> = job_ids
                .iter()
                .copied()
                .filter(|id| !completed_before.contains(id))
                .collect();
            work_item::complete_with_service(&txn, vehicle_id, &added, record.id).await?
        }
        None => Vec::new(),
    };
    link_incidents(&txn, vehicle_id, record.id, Vec::new(), Vec::new(), &added).await?;
    let from_added: Vec<i32> = added.iter().filter_map(|w| w.research_finding_id).collect();
    let added_recalls = recall::recall_ids_among(&txn, &from_added).await?;
    release_incidents(&txn, record.id, &removal.incidents, choice).await?;

    if let Some(item_ids) = input.schedule_item_ids {
        schedule::link_service(&txn, vehicle_id, record.id, &item_ids, LinkMode::Replace).await?;
    }

    let parts_listed = input.part_ids.is_some();
    part_svc::release_from_service(&txn, removal.parts, choice).await?;
    if let Some(new_part_ids) = input.part_ids {
        for &part_id in &new_part_ids {
            part_svc::install(&txn, vehicle_id, part_id, &record, Some(record.mileage)).await?;
        }
    }
    if !parts_listed && (record.service_date != date_before || record.mileage != mileage_before) {
        // The parts this service installed carry its date and mileage as
        // their install date and odometer.
        part_svc::follow_service(&txn, &record).await?;
    }

    // A sent list replaces every recall link; without one, only the recalls
    // of the jobs taken off lose theirs.
    let removed_recalls: Vec<i32> = removal.recalls.iter().map(|r| r.finding_id).collect();
    let mut relink = added_recalls;
    let unlink = service_recall_link::Entity::delete_many()
        .filter(service_recall_link::Column::ServiceRecordId.eq(record.id));
    if let Some(recall_ids) = input.recall_finding_ids {
        relink.extend(recall_ids);
        unlink.exec(&txn).await?;
    } else if !removed_recalls.is_empty() {
        unlink
            .filter(service_recall_link::Column::ResearchFindingId.is_in(removed_recalls.clone()))
            .exec(&txn)
            .await?;
    }
    recall::link_service(&txn, vehicle_id, &relink, record.id).await?;
    recall::release_from_service(&txn, removed_recalls, choice).await?;

    // Handle line items: replace-all semantics when provided
    if let Some(new_line_items) = input.line_items {
        service_record_line_item::Entity::delete_many()
            .filter(service_record_line_item::Column::ServiceRecordId.eq(record.id))
            .exec(&txn)
            .await?;
        insert_line_items(&txn, record.id, new_line_items).await?;
    }

    let mut updated = load_service_links(&txn, record).await?;
    updated.unfitted_needs = unfitted_needs(&txn, &updated.completed_work_items).await?;
    updated.reopened_appointment_ids = settled.reopened_appointment_ids;
    updated.unassigned_job_ids = settled.unassigned_job_ids;

    txn.commit().await?;

    Ok(updated)
}

/// The work a service records as done: the jobs it completed that still read
/// done, the parts it installed, the incidents it resolved and the recalls it
/// performed. Deleting the service releases all of it, and a correction
/// releases what its replacement lists leave out.
#[derive(Default)]
struct RecordedWork {
    jobs: Vec<job::Model>,
    parts: Vec<part::Model>,
    incidents: Vec<incident::Model>,
    recalls: Vec<RecallRef>,
}

impl RecordedWork {
    async fn read(db: &impl ConnectionTrait, service_id: i32) -> DomainResult<Self> {
        // A job moved off done loses its link with the service through its
        // foreign key, so only a job that still reads done is listed.
        let jobs = job::Entity::find()
            .filter(job::Column::CompletedServiceId.eq(service_id))
            .filter(job::Column::Status.eq(JobStatus::Done))
            .order_by_asc(job::Column::Id)
            .all(db)
            .await?;
        let parts = part::Entity::find()
            .filter(part::Column::InstalledServiceId.eq(service_id))
            .order_by_asc(part::Column::Id)
            .all(db)
            .await?;
        let recalls = recall::load_performed(db, &[service_id])
            .await?
            .remove(&service_id)
            .unwrap_or_default();
        Ok(Self {
            jobs,
            parts,
            incidents: resolved_by(db, service_id).await?,
            recalls,
        })
    }

    /// The part of this work that `input` takes off the service: the jobs,
    /// parts and recalls its replacement lists leave out, and with each job
    /// taken off the work that hangs off it (the incident and the recall it
    /// came from, and the parts fitted to its needs) unless a list the call
    /// sends names that work or a job in `listed_jobs`, which the service
    /// keeps or gains, came from the same incident or recall.
    async fn left_out_by(
        mut self,
        db: &impl ConnectionTrait,
        input: &UpdateServiceRecord,
        listed_jobs: &[job::Model],
    ) -> DomainResult<Self> {
        if let Some(list) = &input.work_item_ids {
            self.jobs.retain(|j| !list.contains(&j.id));
        } else {
            self.jobs.clear();
        }
        let job_ids = self.job_ids();
        let released = |id: Option<i32>, of: fn(&job::Model) -> Option<i32>| {
            id.filter(|id| !listed_jobs.iter().any(|j| of(j) == Some(*id)))
        };
        let incident_ids: Vec<i32> = self
            .jobs
            .iter()
            .filter_map(|j| released(j.incident_id, |l| l.incident_id))
            .collect();
        let finding_ids: Vec<i32> = self
            .jobs
            .iter()
            .filter_map(|j| released(j.research_finding_id, |l| l.research_finding_id))
            .collect();
        self.incidents.retain(|i| incident_ids.contains(&i.id));
        match &input.recall_finding_ids {
            Some(list) => self.recalls.retain(|r| !list.contains(&r.finding_id)),
            None => self.recalls.retain(|r| finding_ids.contains(&r.finding_id)),
        }
        match &input.part_ids {
            Some(list) => self.parts.retain(|p| !list.contains(&p.id)),
            None if job_ids.is_empty() => self.parts.clear(),
            None => {
                let on_jobs: Vec<i32> = job_need::Entity::find()
                    .filter(job_need::Column::WorkItemId.is_in(job_ids))
                    .all(db)
                    .await?
                    .into_iter()
                    .filter_map(|n| n.part_id)
                    .collect();
                self.parts.retain(|p| on_jobs.contains(&p.id));
            }
        }
        Ok(self)
    }

    fn job_ids(&self) -> Vec<i32> {
        self.jobs.iter().map(|j| j.id).collect()
    }

    /// The owner's answer for this work. Work that needs none (no jobs, no
    /// fitted parts, no incidents, no recalls) takes `did_not_happen` when no
    /// answer is given; other work is refused, naming each item, until it
    /// has one.
    fn choice(
        &self,
        service_id: i32,
        action: ChoiceFor,
        completed: Option<CompletedWork>,
    ) -> DomainResult<CompletedWork> {
        // A returned part keeps its status either way, so it needs no answer.
        let fitted: Vec<&part::Model> = self
            .parts
            .iter()
            .filter(|p| p.status != PartStatus::Returned)
            .collect();
        let needs_answer = !(self.jobs.is_empty()
            && fitted.is_empty()
            && self.incidents.is_empty()
            && self.recalls.is_empty());
        match completed {
            Some(choice) => Ok(choice),
            None if !needs_answer => Ok(CompletedWork::DidNotHappen),
            None => Err(completed_work_needs_a_choice(
                service_id,
                action,
                &self
                    .jobs
                    .iter()
                    .map(|j| WorkItemRef {
                        id: j.id,
                        title: j.title.clone(),
                    })
                    .collect::<Vec<_>>(),
                &fitted,
                &self.incidents,
                &self.recalls,
            )),
        }
    }
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
    let pool = db;
    let txn = super::begin_write(pool).await?;
    let db = &txn;
    let existing = require(db, vehicle_id, id).await?;

    schedule::link_service(&txn, vehicle_id, existing.id, item_ids, mode).await?;

    // Linking changes what the record accounts for — stamp it like update().
    let mut active: service_record::ActiveModel = existing.into();
    active.updated_at = Set(super::now());
    let record = active.update(&txn).await?;
    txn.commit().await?;

    load_service_links(pool, record).await
}

crate::vocab::vocabulary! {
    /// What happened to the work a service recorded as done, which deleting
    /// the service needs to know: `did_not_happen` undoes the work, and
    /// `done_another_way` keeps it and drops only the invoice.
    pub enum CompletedWork {
        DidNotHappen => "did_not_happen",
        DoneAnotherWay => "done_another_way",
    }
}

// The answer most tests give when they delete a service: the work did not
// happen, so the delete undoes it.
#[cfg(test)]
pub(crate) const UNDO: Option<CompletedWork> = Some(CompletedWork::DidNotHappen);

/// What [`delete`] changed besides the service itself.
#[derive(Debug, Default, Serialize)]
pub struct DeletedService {
    /// Ids of the parts the service installed, which are kept. They are
    /// unlinked from the service, and after `did_not_happen` an installed
    /// one is `purchased` again.
    pub kept_part_ids: Vec<i32>,
    /// Ids of the recalls that read `open` again after `did_not_happen`.
    pub reopened_recall_ids: Vec<i32>,
    /// Ids of the recalls `did_not_happen` left `done` because another
    /// service also performed them.
    pub still_done_elsewhere_recall_ids: Vec<i32>,
    /// Ids of the recalls stored as `done` after `done_another_way`.
    pub kept_done_recall_ids: Vec<i32>,
    /// Ids of the hand-closed appointments that are open again because a job
    /// the service completed is planned again.
    pub reopened_appointment_ids: Vec<i32>,
    /// Ids of the jobs the service completed that came off a cancelled
    /// appointment.
    pub unassigned_job_ids: Vec<i32>,
    /// Ids of the planned jobs whose quote came with the service. They stay
    /// and keep who quoted them and when, but no longer name the service.
    pub unlinked_quote_ids: Vec<i32>,
}

/// Deletes a service record and its owned rows.
///
/// A service that recorded work as done (jobs it completed, parts it
/// installed, incidents it resolved or recalls it performed) needs
/// `completed`: with `None` nothing is written and the call returns
/// `BadRequest` naming each item. `did_not_happen` undoes that work unless
/// another service also covers it, and `done_another_way` keeps it and
/// unlinks it from the service. A part a later service replaced stays
/// replaced either way. The parts are kept and listed in `kept_part_ids`.
pub async fn delete<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    completed: Option<CompletedWork>,
) -> DomainResult<DeletedService> {
    // The write lock is held before anything is read, so the work named in a
    // refusal is the work the delete would undo.
    let txn = super::begin_write(db).await?;
    let existing = require(&txn, vehicle_id, id).await?;

    // The choice is checked before anything is written.
    let work = RecordedWork::read(&txn, existing.id).await?;
    let choice = work.choice(existing.id, ChoiceFor::Delete, completed)?;
    let settled = work_item::release_completed(&txn, &work.job_ids(), choice).await?;
    let finding_ids = work.recalls.iter().map(|r| r.finding_id).collect();
    if choice == CompletedWork::DidNotHappen {
        let incident_ids: Vec<i32> = work.incidents.iter().map(|i| i.id).collect();
        reopen_resolved_incidents(&txn, existing.id, incident_ids).await?;
    }
    // The key clears these on delete too; the explicit write stamps them and
    // does not depend on the foreign_keys pragma.
    let unlinked_quote_ids: Vec<i32> = job::Entity::find()
        .filter(job::Column::QuoteServiceId.eq(existing.id))
        .order_by_asc(job::Column::Id)
        .all(&txn)
        .await?
        .into_iter()
        .map(|j| j.id)
        .collect();
    if !unlinked_quote_ids.is_empty() {
        job::Entity::update_many()
            .set(job::ActiveModel {
                quote_service_id: Set(None),
                updated_at: Set(super::now()),
                ..Default::default()
            })
            .filter(job::Column::Id.is_in(unlinked_quote_ids.clone()))
            .exec(&txn)
            .await?;
    }
    let mut outcome = DeletedService {
        unlinked_quote_ids,
        kept_part_ids: work.parts.iter().map(|p| p.id).collect(),
        reopened_appointment_ids: settled.reopened_appointment_ids,
        unassigned_job_ids: settled.unassigned_job_ids,
        ..Default::default()
    };
    part_svc::release_from_service(&txn, work.parts, choice).await?;

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

    let released = recall::release_from_service(&txn, finding_ids, choice).await?;
    outcome.reopened_recall_ids = released.reopened;
    outcome.kept_done_recall_ids = released.kept_done;
    outcome.still_done_elsewhere_recall_ids = released.still_done_elsewhere;

    txn.commit().await?;

    Ok(outcome)
}

/// Which change to a service needs the owner's answer about its work.
#[derive(Clone, Copy)]
enum ChoiceFor {
    Delete,
    Correction,
}

// The refusal to delete a service that recorded work as done, or to take
// that work off it, before the caller says what happened to it, naming each
// item.
fn completed_work_needs_a_choice(
    service_id: i32,
    action: ChoiceFor,
    jobs: &[WorkItemRef],
    parts: &[&part::Model],
    incidents: &[incident::Model],
    recalls: &[RecallRef],
) -> DomainError {
    let items: Vec<String> = jobs
        .iter()
        .map(|j| format!("job {} \"{}\"", j.id, j.title))
        .chain(
            parts
                .iter()
                .map(|p| format!("part {} \"{}\"", p.id, p.name)),
        )
        .chain(
            incidents
                .iter()
                .map(|i| format!("incident {} \"{}\"", i.id, i.title)),
        )
        .chain(recalls.iter().map(|r| match &r.campaign_number {
            Some(n) => format!("recall {} (campaign {n}) \"{}\"", r.finding_id, r.title),
            None => format!("recall {} \"{}\"", r.finding_id, r.title),
        }))
        .collect();
    let (needs, nothing) = match action {
        ChoiceFor::Delete => ("Deleting it", "Nothing was deleted."),
        ChoiceFor::Correction => ("Taking it off the service", "Nothing was changed."),
    };
    DomainError::BadRequest(format!(
        "Service {service_id} recorded this work as done: {}. {needs} needs to know whether that \
         work did not happen or was done another way. {nothing}",
        items.join(", ")
    ))
}

// The incidents the service resolved that still read resolved, oldest first.
async fn resolved_by(
    txn: &impl ConnectionTrait,
    service_id: i32,
) -> DomainResult<Vec<incident::Model>> {
    let ids: Vec<i32> = incident_service_link::Entity::find()
        .filter(incident_service_link::Column::ServiceRecordId.eq(service_id))
        .filter(incident_service_link::Column::Role.eq(LinkRole::Resolved))
        .all(txn)
        .await?
        .into_iter()
        .map(|l| l.incident_id)
        .collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(incident::Entity::find()
        .filter(incident::Column::Id.is_in(ids))
        .filter(incident::Column::Resolved.eq(true))
        .order_by_asc(incident::Column::Id)
        .all(txn)
        .await?)
}

// Takes `incidents` off the incidents service `service_id` resolved, because
// a correction took off the jobs they came from. After `did_not_happen` each
// is unresolved again unless another service also resolved it.
async fn release_incidents(
    txn: &impl ConnectionTrait,
    service_id: i32,
    incidents: &[incident::Model],
    choice: CompletedWork,
) -> DomainResult<()> {
    if incidents.is_empty() {
        return Ok(());
    }
    let ids: Vec<i32> = incidents.iter().map(|i| i.id).collect();
    incident_service_link::Entity::delete_many()
        .filter(incident_service_link::Column::ServiceRecordId.eq(service_id))
        .filter(incident_service_link::Column::Role.eq(LinkRole::Resolved))
        .filter(incident_service_link::Column::IncidentId.is_in(ids.clone()))
        .exec(txn)
        .await?;
    if choice == CompletedWork::DidNotHappen {
        reopen_resolved_incidents(txn, service_id, ids).await?;
    }
    Ok(())
}

// Marks unresolved each of `resolved_here`, incidents service `service_id`
// resolved and no longer does, unless another service also resolved it.
async fn reopen_resolved_incidents(
    txn: &impl ConnectionTrait,
    service_id: i32,
    resolved_here: Vec<i32>,
) -> DomainResult<()> {
    if resolved_here.is_empty() {
        return Ok(());
    }
    let resolved_elsewhere: std::collections::HashSet<i32> = incident_service_link::Entity::find()
        .filter(incident_service_link::Column::IncidentId.is_in(resolved_here.clone()))
        .filter(incident_service_link::Column::Role.eq(LinkRole::Resolved))
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
            updated_at: Set(super::now()),
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
        entities::{part::Supplier, service_record_line_item::WorkKind, work_item::JobStatus},
        inputs::{incident::NewIncident, part::NewPart},
        services::incident,
        test_support::{VehicleFixture, date, row_count, test_db},
    };

    async fn seed_vehicle(db: &impl ConnectionTrait) -> i32 {
        VehicleFixture::new().insert_id(db).await
    }

    async fn seed_part(db: &impl ConnectionTrait, vehicle_id: i32, name: &str) -> i32 {
        part::ActiveModel {
            vehicle_id: Set(vehicle_id),
            name: Set(name.into()),
            status: Set(PartStatus::Purchased),
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
                service_date: date("2024-03-01"),
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
                    work_kind: WorkKind::Maintenance,
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
                appointment_id: None,
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
        assert_eq!(p.status, PartStatus::Installed);
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
                service_date: date("2024-03-01"),
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
                    work_kind: WorkKind::Maintenance,
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
                appointment_id: None,
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
                completed_work: Some(CompletedWork::DidNotHappen),
                line_items: Some(vec![NewLineItem {
                    paid_by: None,
                    description: "new".into(),
                    work_kind: WorkKind::Maintenance,
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
        assert_eq!(a.status, PartStatus::Purchased);
        assert_eq!(a.installed_service_id, None);
        let b = part::Entity::find_by_id(part_b)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(b.status, PartStatus::Installed);
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
                service_date: date("2024-03-01"),
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
                appointment_id: None,
            },
        )
        .await
        .unwrap();

        let kept = delete(&db, vid, created.record.id, UNDO)
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
        assert_eq!(p.status, PartStatus::Purchased);
        assert_eq!(p.installed_service_id, None);
        // Schedule links removed
        let links = service_schedule_link::Entity::find()
            .all(&db)
            .await
            .unwrap();
        crate::test_support::assert_empty(&links);
    }

    #[tokio::test]
    async fn delete_clears_finding_links_and_removes_incident_links() {
        use crate::entities::incident;

        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc_id = service_record::ActiveModel {
            vehicle_id: Set(vid),
            service_date: Set(date("2024-03-01")),
            paid_by: Set(Payer::Owner),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;

        // An incident linked to the record via the M2M link table.
        let inc = incident::ActiveModel {
            vehicle_id: Set(vid),
            category: Set(IncidentCategory::Noise),
            title: Set("Rattle".into()),
            occurred_at: Set(date("2024-02-01")),
            resolved: Set(false),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        incident_service_link::ActiveModel {
            incident_id: Set(inc.id),
            service_record_id: Set(svc_id),
            role: Set(LinkRole::Resolved),
        }
        .insert(&db)
        .await
        .unwrap();

        delete(&db, vid, svc_id, UNDO).await.unwrap();

        // The incident link rows are gone.
        let links = incident_service_link::Entity::find()
            .filter(incident_service_link::Column::ServiceRecordId.eq(svc_id))
            .all(&db)
            .await
            .unwrap();
        crate::test_support::assert_empty(&links);
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
            recorded_at: Set(date("2024-02-01 09:00:00")),
            source: Set(Some("manual".into())),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        delete(&db, vid, created.record.id, UNDO).await.unwrap();

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
                service_date: Some(date("2024-04-01")),
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
        assert_eq!(logs[0].recorded_at, date("2024-04-01"));

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
        crate::test_support::assert_empty(
            &mileage_log::Entity::find()
                .filter(mileage_log::Column::ServiceRecordId.eq(created.record.id))
                .all(&db)
                .await
                .unwrap(),
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

    #[tokio::test]
    async fn a_shop_id_that_names_no_shop_is_not_found() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let err = create(
            &db,
            vid,
            NewServiceRecord {
                shop_id: Some(9999),
                ..minimal_record(None)
            },
        )
        .await
        .unwrap_err();
        assert_eq!(err.to_string(), "Shop 9999 not found");
        crate::test_support::assert_empty(&list_between(&db, vid, None, None).await.unwrap());

        let made = create(&db, vid, minimal_record(None)).await.unwrap();
        let err = update(
            &db,
            vid,
            made.record.id,
            UpdateServiceRecord {
                shop_id: Some(Some(9999)),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert_eq!(err.to_string(), "Shop 9999 not found");
    }

    fn minimal_record(schedule_item_ids: Option<Vec<i32>>) -> NewServiceRecord {
        NewServiceRecord {
            service_date: date("2024-03-01"),
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
            appointment_id: None,
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
                    work_kind: WorkKind::Maintenance,
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
                service_date: Set(date("2024-03-01")),
                paid_by: Set(Payer::Owner),
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
                service_date: Set(date("2024-03-01")),
                paid_by: Set(Payer::Owner),
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
                paid_by: Some(Payer::Insurance),
                payer_note: Some("Progressive claim #12345".into()),
                ..minimal_record(None)
            },
        )
        .await
        .unwrap();
        assert_eq!(insured.record.paid_by, Payer::Insurance);
        assert_eq!(
            insured.record.payer_note.as_deref(),
            Some("Progressive claim #12345")
        );
        let fetched = get(&db, vid, insured.record.id).await.unwrap();
        assert_eq!(fetched.record.paid_by, Payer::Insurance);
        assert_eq!(
            fetched.record.payer_note.as_deref(),
            Some("Progressive claim #12345")
        );

        // Omitted payer defaults to self.
        let plain = create(&db, vid, minimal_record(None)).await.unwrap();
        assert_eq!(plain.record.paid_by, Payer::Owner);
        assert_eq!(plain.record.payer_note, None);
    }

    #[tokio::test]
    async fn update_changes_payer_and_sets_then_clears_note() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = create(&db, vid, minimal_record(None)).await.unwrap();
        assert_eq!(created.record.paid_by, Payer::Owner);

        // Change payer + set the note.
        let updated = update(
            &db,
            vid,
            created.record.id,
            UpdateServiceRecord {
                paid_by: Some(Payer::ThirdParty),
                payer_note: Some(Some("Neighbor's insurance, side-swipe".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.record.paid_by, Payer::ThirdParty);
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
        assert_eq!(untouched.record.paid_by, Payer::ThirdParty);
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
        crate::test_support::assert_empty(&fetched.schedule_item_ids);
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

    fn line(description: &str, work_kind: WorkKind) -> NewLineItem {
        NewLineItem {
            paid_by: None,
            description: description.into(),
            work_kind,
            quantity: None,
            unit_cost_cents: None,
            cost_cents: None,
        }
    }

    #[tokio::test]
    async fn a_line_stores_its_own_payer() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let got = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: date("2026-11-03"),
                line_items: Some(vec![
                    line("Oil change", WorkKind::Maintenance),
                    NewLineItem {
                        paid_by: Some(Payer::Manufacturer),
                        ..line("Water pump under warranty", WorkKind::Repair)
                    },
                ]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let payers: Vec<Option<Payer>> = got.line_items.iter().map(|l| l.paid_by).collect();
        assert_eq!(payers, vec![None, Some(Payer::Manufacturer)]);
        assert_eq!(effective_payer(None, Payer::Insurance), Payer::Insurance);
        assert_eq!(
            effective_payer(Some(Payer::Owner), Payer::Insurance),
            Payer::Owner
        );
    }

    fn inline_part(name: &str, supplied_by: Supplier) -> NewPart {
        NewPart {
            name: name.into(),
            supplied_by: Some(supplied_by),
            ..Default::default()
        }
    }

    fn kinds(got: &ServiceRecordWithLinks) -> Vec<WorkKind> {
        got.line_items.iter().map(|l| l.work_kind).collect()
    }

    #[tokio::test]
    async fn create_stores_who_did_it_the_date_range_and_line_kinds() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let got = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: date("2025-03-24"),
                end_date: Some(date("2025-03-25")),
                performed_by: Some(Performer::Shop),
                line_items: Some(vec![
                    line("Replace suction jet pump", WorkKind::Repair),
                    line("Intake upgrade", WorkKind::Mod),
                ]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(got.record.performed_by, Performer::Shop);
        assert_eq!(got.record.end_date, Some(date("2025-03-25")));
        assert_eq!(kinds(&got), vec![WorkKind::Repair, WorkKind::Mod]);
    }

    #[tokio::test]
    async fn create_rejects_an_end_date_before_the_start() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let err = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: date("2025-03-25"),
                end_date: Some(date("2025-03-24")),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "end_date"));
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
                service_date: date("2025-03-24"),
                performed_by: Some(Performer::Owner),
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
                service_date: date("2025-03-24"),
                mileage: Some(52_747),
                parts: Some(vec![
                    NewPart {
                        seller: Some("FCP Euro".into()),
                        ..inline_part("Front brake pads", Supplier::Owner)
                    },
                    inline_part("Brake fluid", Supplier::Shop),
                ]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(got.parts.len(), 2);
        assert_eq!(got.part_ids.len(), 2);
        assert_eq!(got.parts[0].part.supplied_by, Supplier::Owner);
        assert_eq!(got.parts[0].part.installed_service_id, Some(got.record.id));
        assert_eq!(got.parts[0].part.installed_odometer, Some(52_747));
        assert_eq!(got.parts[0].part.installed_date, Some(date("2025-03-24")));
        assert_eq!(got.parts[0].part.status, PartStatus::Installed);
        assert_eq!(got.parts[1].part.supplied_by, Supplier::Shop);
    }

    #[tokio::test]
    async fn installing_a_returned_part_through_a_service_is_refused() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let returned = part::ActiveModel {
            vehicle_id: Set(vid),
            name: Set("Rotors".into()),
            status: Set(PartStatus::Returned),
            supplied_by: Set(Supplier::Owner),
            returned_on: Set(Some(date("2026-11-06"))),
            return_reason: Set(Some("Wrong size".into())),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let err = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: date("2026-11-10"),
                part_ids: Some(vec![returned.id]),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        assert_eq!(row_count::<service_record::Entity>(&db).await, 0);
        let after = part::Entity::find_by_id(returned.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.status, PartStatus::Returned);
    }

    #[tokio::test]
    async fn refiling_an_invoice_number_returns_the_first_record_untouched() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let first = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: date("2025-03-24"),
                invoice_ref: Some("07447".into()),
                line_items: Some(vec![line("Oil change", WorkKind::Maintenance)]),
                parts: Some(vec![inline_part("Oil filter", Supplier::Shop)]),
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
                service_date: date("2025-03-24"),
                invoice_ref: Some("07447".into()),
                line_items: Some(vec![
                    line("Other", WorkKind::Repair),
                    line("Another", WorkKind::Mod),
                ]),
                parts: Some(vec![
                    inline_part("A", Supplier::Owner),
                    inline_part("B", Supplier::Owner),
                ]),
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
    async fn a_refused_inline_part_rolls_back_the_whole_service() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let result = create(
            &db,
            vid,
            NewServiceRecord {
                service_date: date("2025-03-24"),
                mileage: Some(50_000),
                line_items: Some(vec![line("Oil change", WorkKind::Maintenance)]),
                parts: Some(vec![NewPart {
                    need_id: Some(99_999),
                    ..inline_part("Filter", Supplier::Owner)
                }]),
                ..Default::default()
            },
        )
        .await;
        assert!(matches!(result, Err(DomainError::NotFound(_))));
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
                service_date: date("2025-03-24"),
                performed_by: Some(Performer::Owner),
                line_items: Some(vec![line("IE Stage 1 tune flash", WorkKind::Mod)]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(got.record.performed_by, Performer::Owner);
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
                performed_by: Some(Performer::Owner),
                end_date: Some(Some(date("2024-03-02"))),
                line_items: Some(vec![line("Flash", WorkKind::Mod)]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.record.performed_by, Performer::Owner);
        assert_eq!(updated.record.end_date, Some(date("2024-03-02")));
        assert_eq!(kinds(&updated), vec![WorkKind::Mod]);

        let err = update(
            &db,
            vid,
            made.record.id,
            UpdateServiceRecord {
                end_date: Some(Some(date("2024-02-01"))),
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
                service_date: crate::test_support::date(date),
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
                category: IncidentCategory::ShopObservation,
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
                service_date: date("2024-06-12"),
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
            date("2024-06-12")
        );
        let s24 = get(&db, v, svc_2024).await.unwrap();
        assert_eq!(
            s24.resolved_incidents[0]
                .other_service
                .as_ref()
                .unwrap()
                .service_date,
            date("2023-05-03")
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
                service_date: date("2023-05-03"),
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
                service_date: date("2023-06-01"),
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
                incident_id: Some(incident_id),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Naming the incident as well as the work item must not double-link.
        let svc_2024 = create(
            &db,
            v,
            NewServiceRecord {
                service_date: date("2024-06-12"),
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
        crate::test_support::assert_empty(&incident::list_filtered(&db, v1, None).await.unwrap());

        // Foreign incident ids on record_service leave no footprint at all.
        let sched = seed_schedule_item(&db, v1, "Oil change").await;
        let full = |as_resolved: bool, id: i32| {
            let ids = Some(vec![id]);
            NewServiceRecord {
                service_date: date("2024-06-12"),
                mileage: Some(50_000),
                line_items: Some(vec![line("Oil", WorkKind::Maintenance)]),
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
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let closing = create(
            &db,
            v,
            NewServiceRecord {
                service_date: date("2024-06-01"),
                work_item_ids: Some(vec![work_item.id]),
                resolved_incident_ids: Some(vec![incident_a, incident_b]),
                recall_finding_ids: Some(vec![finding_id]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let work_item_before = work_item_svc::get(&db, v, work_item.id).await.unwrap();
        assert_eq!(work_item_before.status, JobStatus::Done);
        assert_eq!(
            recall::list(&db, v).await.unwrap()[0].status,
            recall::ShownRecallStatus::Done
        );

        delete(&db, v, closing.record.id, UNDO).await.unwrap();

        let work_item_after = work_item_svc::get(&db, v, work_item.id).await.unwrap();
        assert_eq!(work_item_after.status, JobStatus::Planned);
        assert_eq!(work_item_after.completed_service_id, None);
        let reopened_incident = incident::get(&db, v, incident_a).await.unwrap();
        let kept_incident = incident::get(&db, v, incident_b).await.unwrap();
        assert!(!reopened_incident.incident.resolved);
        assert!(kept_incident.incident.resolved);
        assert_eq!(
            recall::list(&db, v).await.unwrap()[0].status,
            recall::ShownRecallStatus::Open
        );
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
                service_date: date("2024-06-01"),
                recall_finding_ids: Some(vec![finding_id]),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .record
        .id
    }

    async fn recall_status(
        db: &DatabaseConnection,
        v: i32,
    ) -> Vec<(recall::ShownRecallStatus, Option<i32>)> {
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
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let svc = create(
            &db,
            v,
            NewServiceRecord {
                service_date: date("2024-06-01"),
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
        assert!(
            message.contains(&format!("job {} ", work_item.id))
                && message.contains(&format!("incident {incident_id} ")),
            "{message}"
        );
        assert!(
            message.contains("did not happen or was done another way"),
            "{message}"
        );
        assert!(get(&db, v, id).await.is_ok());
        assert_eq!(
            recall_status(&db, v).await,
            vec![(recall::ShownRecallStatus::Done, Some(id))]
        );
        assert_eq!(
            work_item_svc::get(&db, v, work_item.id)
                .await
                .unwrap()
                .status,
            JobStatus::Done
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
        recall::set_status(&db, v, finding_id, recall::RecallStatus::Done)
            .await
            .unwrap();

        let out = delete(&db, v, svc, UNDO).await.unwrap();

        assert_eq!(out.reopened_recall_ids, vec![finding_id]);
        crate::test_support::assert_empty(&out.kept_done_recall_ids);
        assert_eq!(
            recall_status(&db, v).await,
            vec![(recall::ShownRecallStatus::Open, None)]
        );
    }

    #[tokio::test]
    async fn keep_done_leaves_a_performed_recall_done_with_no_service() {
        let db = test_db().await;
        let v = seed_vehicle(&db).await;
        let finding_id = seed_recall(&db, v, "24V999").await;
        let svc = service_with_recall(&db, v, finding_id).await;

        let out = delete(&db, v, svc, Some(CompletedWork::DoneAnotherWay))
            .await
            .unwrap();

        assert_eq!(out.kept_done_recall_ids, vec![finding_id]);
        crate::test_support::assert_empty(&out.reopened_recall_ids);
        assert_eq!(
            recall_status(&db, v).await,
            vec![(recall::ShownRecallStatus::Done, None)]
        );
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

        let out = delete(&db, v, first, UNDO).await.unwrap();

        crate::test_support::assert_empty(&out.reopened_recall_ids);
        assert_eq!(out.still_done_elsewhere_recall_ids, vec![finding_id]);
        assert_eq!(
            recall_status(&db, v).await,
            vec![(recall::ShownRecallStatus::Done, Some(second))]
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
                service_date: date("2024-06-01"),
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
        let part_id = svc.parts[0].part.id;

        delete(&db, v, svc.record.id, UNDO).await.unwrap();

        let kept = part_svc::get(&db, v, part_id).await.unwrap();
        assert_eq!(kept.status, PartStatus::Purchased);
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
                service_date: date("2024-03-01"),
                mileage: Some(50_000),
                parts: Some(vec![inline_part("Pads", Supplier::Shop)]),
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
                service_date: Some(date("2024-03-10")),
                mileage: Some(Some(50_500)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            corrected.parts[0].part.installed_date,
            Some(date("2024-03-10"))
        );
        assert_eq!(corrected.parts[0].part.installed_odometer, Some(50_500));

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
        assert_eq!(cleared.parts[0].part.installed_odometer, Some(50_500));
        assert_eq!(
            cleared.parts[0].part.installed_date,
            Some(date("2024-03-10"))
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
                .map(|r| r.record.service_date.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            dates(
                list_between(&db, v, Some(date("2024-01-01")), None)
                    .await
                    .unwrap()
            ),
            vec!["2025-07-09", "2024-03-01"]
        );
        assert_eq!(
            dates(
                list_between(&db, v, None, Some(date("2024-03-01")))
                    .await
                    .unwrap()
            ),
            vec!["2024-03-01", "2023-01-05"]
        );
        assert_eq!(list_between(&db, v, None, None).await.unwrap().len(), 3);
    }

    // ─── invoices against appointments ───

    // Invoices filed at once from several pools are all recorded: each opens
    // with the write lock before it reads the schedule it links.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_services_on_one_vehicle_are_all_recorded() {
        let (_dir, pools) = crate::test_support::race_pools().await;
        let first = &pools[0];
        let v = seed_vehicle(first).await;

        let results = crate::test_support::race(&pools, |i, db| async move {
            let input = NewServiceRecord {
                service_date: date("2024-06-01"),
                invoice_ref: Some(format!("INV-{i}")),
                ..Default::default()
            };
            create(&db, v, input).await.map(|_| ())
        })
        .await;

        assert!(results.iter().all(Result::is_ok), "{results:?}");
        assert_eq!(
            row_count::<service_record::Entity>(first).await,
            results.len() as u64
        );
    }

    // One invoice filed at once from several pools is stored once, and every
    // other call gets that record back as already recorded.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn one_invoice_filed_at_once_is_recorded_once() {
        let (_dir, pools) = crate::test_support::race_pools().await;
        let first = &pools[0];
        let v = seed_vehicle(first).await;

        let results = crate::test_support::race(&pools, |_, db| async move {
            let input = NewServiceRecord {
                service_date: date("2024-06-01"),
                invoice_ref: Some("INV-1".into()),
                ..Default::default()
            };
            create(&db, v, input).await.map(|r| r.already_recorded)
        })
        .await;

        let repeats: Vec<bool> = results.into_iter().map(Result::unwrap).collect();
        assert_eq!(repeats.iter().filter(|r| !**r).count(), 1, "{repeats:?}");
        assert_eq!(row_count::<service_record::Entity>(first).await, 1);
    }

    // Deletes racing on one file from several pools delete the service once,
    // and the rest find it gone: each takes the write lock before it reads.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_deletes_of_one_service_delete_it_once() {
        let (_dir, pools) = crate::test_support::race_pools().await;
        let first = &pools[0];
        let v = seed_vehicle(first).await;
        let part_id = seed_part(first, v, "Pads").await;
        let id = create(
            first,
            v,
            NewServiceRecord {
                service_date: date("2024-06-01"),
                part_ids: Some(vec![part_id]),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .record
        .id;

        let results = crate::test_support::race(&pools, |_, db| async move {
            delete(&db, v, id, UNDO).await.map(|_| ())
        })
        .await;

        assert_eq!(
            results.iter().filter(|r| r.is_ok()).count(),
            1,
            "{results:?}"
        );
        for err in results.iter().filter_map(|r| r.as_ref().err()) {
            assert!(matches!(err, DomainError::NotFound(_)), "{err:?}");
        }
    }

    mod appointments {
        use super::*;
        use crate::{
            entities::{job_need, work_item},
            error::DomainError,
            inputs::{appointment::NewAppointment, need::NewNeed, work_item::NewWorkItem},
            services::{appointment, need, part as part_svc, work_item as job},
        };

        struct Fixture {
            db: DatabaseConnection,
            vehicle: i32,
            appointment: i32,
        }

        async fn fixture() -> Fixture {
            let db = test_db().await;
            let vehicle = seed_vehicle(&db).await;
            let appointment = appointment::create(
                &db,
                vehicle,
                NewAppointment {
                    purpose: "Brakes".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .id;
            Fixture {
                db,
                vehicle,
                appointment,
            }
        }

        async fn add_job(f: &Fixture, title: &str) -> i32 {
            job::create(
                &f.db,
                f.vehicle,
                NewWorkItem {
                    title: title.into(),
                    appointment_id: Some(f.appointment),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .id
        }

        async fn add_need(
            f: &Fixture,
            job_id: i32,
            name: &str,
            supplied_by: Option<Supplier>,
        ) -> i32 {
            need::add(
                &f.db,
                f.vehicle,
                job_id,
                NewNeed {
                    name: name.into(),
                    supplied_by,
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .id
        }

        // A recorded part linked to the need, as the owner's own purchase.
        async fn link_new_part(f: &Fixture, need_id: i32, received: bool) -> i32 {
            let part_id = seed_part(&f.db, f.vehicle, "Pads").await;
            let received_on = received.then(|| date("2026-11-01"));
            need::link_part(&f.db, f.vehicle, need_id, part_id, received_on)
                .await
                .unwrap();
            part_id
        }

        async fn stage_of(f: &Fixture, need_id: i32) -> need::Stage {
            let n = job_need::Entity::find_by_id(need_id)
                .one(&f.db)
                .await
                .unwrap()
                .unwrap();
            let p = match n.part_id {
                Some(id) => part::Entity::find_by_id(id).one(&f.db).await.unwrap(),
                None => None,
            };
            need::stage(&n, p.as_ref())
        }

        async fn job_status(f: &Fixture, job_id: i32) -> JobStatus {
            work_item::Entity::find_by_id(job_id)
                .one(&f.db)
                .await
                .unwrap()
                .unwrap()
                .status
        }

        async fn appointment_status(f: &Fixture) -> appointment::AppointmentStatus {
            appointment::summary(&f.db, f.vehicle, f.appointment, crate::services::today())
                .await
                .unwrap()
                .status
        }

        fn invoice(f: &Fixture) -> NewServiceRecord {
            NewServiceRecord {
                service_date: date("2026-11-10"),
                appointment_id: Some(f.appointment),
                ..Default::default()
            }
        }

        #[tokio::test]
        async fn naming_an_appointment_installs_and_completes_nothing_by_itself() {
            let f = fixture().await;
            let job_id = add_job(&f, "Rear brakes").await;
            let need_id = add_need(&f, job_id, "Pads", None).await;
            let part_id = link_new_part(&f, need_id, false).await;

            let created = create(&f.db, f.vehicle, invoice(&f)).await.unwrap();

            assert_eq!(created.record.appointment_id, Some(f.appointment));
            assert_eq!(job_status(&f, job_id).await, JobStatus::Planned);
            let part = part_svc::get(&f.db, f.vehicle, part_id).await.unwrap();
            assert_eq!(part.status, PartStatus::Purchased);
            assert_eq!(stage_of(&f, need_id).await, need::Stage::Ordered);
            assert!(created.unfitted_needs.is_empty());
            assert!(created.completed_work_items.is_empty());
        }

        #[tokio::test]
        async fn named_parts_install_and_their_needs_read_installed() {
            let f = fixture().await;
            let job_id = add_job(&f, "Rear brakes").await;
            let need_id = add_need(&f, job_id, "Pads", None).await;
            let part_id = link_new_part(&f, need_id, true).await;

            let created = create(
                &f.db,
                f.vehicle,
                NewServiceRecord {
                    part_ids: Some(vec![part_id]),
                    work_item_ids: Some(vec![job_id]),
                    ..invoice(&f)
                },
            )
            .await
            .unwrap();

            assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);
            assert_eq!(job_status(&f, job_id).await, JobStatus::Done);
            assert!(created.unfitted_needs.is_empty());
        }

        #[tokio::test]
        async fn inline_shop_part_fills_its_need() {
            let f = fixture().await;
            let job_id = add_job(&f, "Rear brakes").await;
            let need_id = add_need(&f, job_id, "Pads", Some(Supplier::Shop)).await;

            let created = create(
                &f.db,
                f.vehicle,
                NewServiceRecord {
                    work_item_ids: Some(vec![job_id]),
                    parts: Some(vec![NewPart {
                        name: "Shop pads".into(),
                        supplied_by: Some(Supplier::Shop),
                        need_id: Some(need_id),
                        ..Default::default()
                    }]),
                    ..invoice(&f)
                },
            )
            .await
            .unwrap();

            let new_part_id = created.part_ids[0];
            let n = job_need::Entity::find_by_id(need_id)
                .one(&f.db)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(n.part_id, Some(new_part_id));
            assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);
            assert!(created.unfitted_needs.is_empty());
        }

        #[tokio::test]
        async fn unfitted_needs_lists_linked_parts_not_named() {
            let f = fixture().await;
            let job_id = add_job(&f, "Rear brakes").await;
            let fitted = add_need(&f, job_id, "Pads", None).await;
            let fitted_part = link_new_part(&f, fitted, true).await;
            let left_over = add_need(&f, job_id, "Sensor", None).await;
            let left_over_part = link_new_part(&f, left_over, false).await;
            // A need with no part and a need on a job this service does not
            // complete are not listed.
            add_need(&f, job_id, "Grease", None).await;
            let other_job = add_job(&f, "Wipers").await;
            let other_need = add_need(&f, other_job, "Blades", None).await;
            link_new_part(&f, other_need, false).await;

            let created = create(
                &f.db,
                f.vehicle,
                NewServiceRecord {
                    part_ids: Some(vec![fitted_part]),
                    work_item_ids: Some(vec![job_id]),
                    ..invoice(&f)
                },
            )
            .await
            .unwrap();

            assert_eq!(created.unfitted_needs.len(), 1);
            let listed = &created.unfitted_needs[0];
            assert_eq!(listed.need_id, left_over);
            assert_eq!(listed.name, "Sensor");
            assert_eq!(listed.job_title, "Rear brakes");
            assert_eq!(listed.part_id, left_over_part);
            let json = serde_json::to_value(&created).unwrap();
            assert_eq!(json["unfitted_needs"].as_array().unwrap().len(), 1);

            // Plain reads leave the list out; an update recomputes it.
            let read = get(&f.db, f.vehicle, created.record.id).await.unwrap();
            assert!(read.unfitted_needs.is_empty());
            assert!(
                serde_json::to_value(&read)
                    .unwrap()
                    .get("unfitted_needs")
                    .is_none()
            );
            let updated = update(
                &f.db,
                f.vehicle,
                created.record.id,
                UpdateServiceRecord {
                    part_ids: Some(vec![fitted_part, left_over_part]),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            assert!(updated.unfitted_needs.is_empty());
            let again = update(
                &f.db,
                f.vehicle,
                created.record.id,
                UpdateServiceRecord {
                    part_ids: Some(vec![fitted_part]),
                    completed_work: Some(CompletedWork::DidNotHappen),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            assert_eq!(again.unfitted_needs.len(), 1);
        }

        async fn set_part_status(f: &Fixture, part_id: i32, status: PartStatus) {
            part::ActiveModel {
                id: Unchanged(part_id),
                status: Set(status),
                ..Default::default()
            }
            .update(&f.db)
            .await
            .unwrap();
        }

        #[tokio::test]
        async fn a_replaced_part_is_not_listed_as_unfitted() {
            let f = fixture().await;
            let job_id = add_job(&f, "Rear brakes").await;
            let need_id = add_need(&f, job_id, "Pads", None).await;
            let part_id = link_new_part(&f, need_id, true).await;
            set_part_status(&f, part_id, PartStatus::Replaced).await;

            let created = create(
                &f.db,
                f.vehicle,
                NewServiceRecord {
                    work_item_ids: Some(vec![job_id]),
                    ..invoice(&f)
                },
            )
            .await
            .unwrap();

            assert!(created.unfitted_needs.is_empty());
        }

        // Dropping a replaced part from the service's `part_ids` keeps its
        // status, so the need stays linked and still reads installed.
        #[tokio::test]
        async fn a_replaced_part_dropped_from_its_service_still_reads_installed() {
            let f = fixture().await;
            let job_id = add_job(&f, "Rear brakes").await;
            let need_id = add_need(&f, job_id, "Pads", None).await;
            let part_id = link_new_part(&f, need_id, true).await;
            let created = create(
                &f.db,
                f.vehicle,
                NewServiceRecord {
                    part_ids: Some(vec![part_id]),
                    work_item_ids: Some(vec![job_id]),
                    ..invoice(&f)
                },
            )
            .await
            .unwrap();
            set_part_status(&f, part_id, PartStatus::Replaced).await;

            update(
                &f.db,
                f.vehicle,
                created.record.id,
                UpdateServiceRecord {
                    part_ids: Some(vec![]),
                    completed_work: Some(CompletedWork::DidNotHappen),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

            let n = job_need::Entity::find_by_id(need_id)
                .one(&f.db)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(n.part_id, Some(part_id));
            assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);
        }

        #[tokio::test]
        async fn appointment_reads_done_when_its_last_job_completes() {
            let f = fixture().await;
            let first = add_job(&f, "Rear brakes").await;
            let second = add_job(&f, "Wipers").await;
            assert_eq!(
                appointment_status(&f).await,
                appointment::AppointmentStatus::Planning
            );

            create(
                &f.db,
                f.vehicle,
                NewServiceRecord {
                    work_item_ids: Some(vec![first]),
                    ..invoice(&f)
                },
            )
            .await
            .unwrap();
            assert_eq!(
                appointment_status(&f).await,
                appointment::AppointmentStatus::Planning
            );

            create(
                &f.db,
                f.vehicle,
                NewServiceRecord {
                    work_item_ids: Some(vec![second]),
                    service_date: date("2026-11-11"),
                    ..invoice(&f)
                },
            )
            .await
            .unwrap();
            assert_eq!(
                appointment_status(&f).await,
                appointment::AppointmentStatus::Done
            );
        }

        #[tokio::test]
        async fn deleting_the_service_reopens_the_appointment_and_needs_fall_back() {
            let f = fixture().await;
            let job_id = add_job(&f, "Rear brakes").await;
            let need_id = add_need(&f, job_id, "Pads", None).await;
            let part_id = link_new_part(&f, need_id, true).await;
            let created = create(
                &f.db,
                f.vehicle,
                NewServiceRecord {
                    part_ids: Some(vec![part_id]),
                    work_item_ids: Some(vec![job_id]),
                    ..invoice(&f)
                },
            )
            .await
            .unwrap();
            assert_eq!(
                appointment_status(&f).await,
                appointment::AppointmentStatus::Done
            );
            assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);

            let deleted = delete(&f.db, f.vehicle, created.record.id, UNDO)
                .await
                .unwrap();

            // An appointment that read done from its jobs had no date to clear.
            crate::test_support::assert_empty(&deleted.reopened_appointment_ids);
            crate::test_support::assert_empty(&deleted.unassigned_job_ids);
            assert_eq!(
                appointment_status(&f).await,
                appointment::AppointmentStatus::Planning
            );
            assert_eq!(job_status(&f, job_id).await, JobStatus::Planned);
            assert_eq!(stage_of(&f, need_id).await, need::Stage::Received);
        }

        #[tokio::test]
        async fn deleting_the_service_of_a_hand_closed_appointment_opens_it_again() {
            let f = fixture().await;
            let job_id = add_job(&f, "Rear brakes").await;
            let need_id = add_need(&f, job_id, "Pads", None).await;
            let part_id = link_new_part(&f, need_id, true).await;
            let filed = || NewServiceRecord {
                part_ids: Some(vec![part_id]),
                work_item_ids: Some(vec![job_id]),
                ..invoice(&f)
            };
            let created = create(&f.db, f.vehicle, filed()).await.unwrap();
            appointment::close(&f.db, f.vehicle, f.appointment)
                .await
                .unwrap();

            let deleted = delete(&f.db, f.vehicle, created.record.id, UNDO)
                .await
                .unwrap();
            assert_eq!(deleted.reopened_appointment_ids, vec![f.appointment]);
            crate::test_support::assert_empty(&deleted.unassigned_job_ids);

            let summary =
                appointment::summary(&f.db, f.vehicle, f.appointment, crate::services::today())
                    .await
                    .unwrap();
            assert_eq!(summary.status, appointment::AppointmentStatus::Planning);
            assert_eq!(summary.appointment.closed_on, None);
            let open = appointment::list(&f.db, f.vehicle, false, crate::services::today())
                .await
                .unwrap();
            assert_eq!(open.len(), 1);
            assert_eq!(job_status(&f, job_id).await, JobStatus::Planned);
            // Re-recording the corrected invoice against the appointment works.
            create(&f.db, f.vehicle, filed()).await.unwrap();
            assert_eq!(
                appointment_status(&f).await,
                appointment::AppointmentStatus::Done
            );
        }

        #[tokio::test]
        async fn deleting_the_service_of_a_cancelled_appointment_unschedules_its_jobs() {
            let f = fixture().await;
            let job_id = add_job(&f, "Rear brakes").await;
            add_need(&f, job_id, "Pads", None).await;
            let created = create(
                &f.db,
                f.vehicle,
                NewServiceRecord {
                    work_item_ids: Some(vec![job_id]),
                    ..invoice(&f)
                },
            )
            .await
            .unwrap();
            appointment::cancel(&f.db, f.vehicle, f.appointment)
                .await
                .unwrap();

            let deleted = delete(&f.db, f.vehicle, created.record.id, UNDO)
                .await
                .unwrap();
            crate::test_support::assert_empty(&deleted.reopened_appointment_ids);
            assert_eq!(deleted.unassigned_job_ids, vec![job_id]);

            assert_eq!(
                appointment_status(&f).await,
                appointment::AppointmentStatus::Cancelled
            );
            let job = work_item::Entity::find_by_id(job_id)
                .one(&f.db)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(job.status, JobStatus::Planned);
            assert_eq!(job.appointment_id, None);
        }

        #[tokio::test]
        async fn deleting_the_service_unlinks_its_inline_shop_part() {
            let f = fixture().await;
            let job_id = add_job(&f, "Rear brakes").await;
            let need_id = add_need(&f, job_id, "Brake fluid", Some(Supplier::Shop)).await;
            let filed = || NewServiceRecord {
                work_item_ids: Some(vec![job_id]),
                parts: Some(vec![NewPart {
                    name: "DOT4 fluid".into(),
                    supplied_by: Some(Supplier::Shop),
                    need_id: Some(need_id),
                    ..Default::default()
                }]),
                ..invoice(&f)
            };
            let created = create(&f.db, f.vehicle, filed()).await.unwrap();

            let deleted = delete(&f.db, f.vehicle, created.record.id, UNDO)
                .await
                .unwrap();

            assert_eq!(deleted.kept_part_ids, created.part_ids);
            assert_eq!(stage_of(&f, need_id).await, need::Stage::ShopSupplies);
            let again = create(&f.db, f.vehicle, filed()).await.unwrap();
            let n = job_need::Entity::find_by_id(need_id)
                .one(&f.db)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(n.part_id, Some(again.part_ids[0]));
            assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);
        }

        // A correction that takes the shop's part off the invoice leaves the
        // need as the delete does, and the owner's own part stays linked.
        #[tokio::test]
        async fn correcting_the_parts_unlinks_a_removed_shop_part_only() {
            let f = fixture().await;
            let job_id = add_job(&f, "Rear brakes").await;
            let fluid = add_need(&f, job_id, "Brake fluid", Some(Supplier::Shop)).await;
            let pads = add_need(&f, job_id, "Pads", None).await;
            let own_part = link_new_part(&f, pads, true).await;
            let created = create(
                &f.db,
                f.vehicle,
                NewServiceRecord {
                    part_ids: Some(vec![own_part]),
                    parts: Some(vec![NewPart {
                        name: "DOT4 fluid".into(),
                        supplied_by: Some(Supplier::Shop),
                        need_id: Some(fluid),
                        ..Default::default()
                    }]),
                    ..invoice(&f)
                },
            )
            .await
            .unwrap();
            assert_eq!(stage_of(&f, fluid).await, need::Stage::Installed);

            update(
                &f.db,
                f.vehicle,
                created.record.id,
                UpdateServiceRecord {
                    part_ids: Some(Vec::new()),
                    completed_work: Some(CompletedWork::DidNotHappen),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

            assert_eq!(stage_of(&f, fluid).await, need::Stage::ShopSupplies);
            assert_eq!(stage_of(&f, pads).await, need::Stage::Received);
        }

        // A "Rear brakes" job whose shop-supplied fluid an invoice installed.
        struct Filed {
            job_id: i32,
            need_id: i32,
            service_id: i32,
            part_id: i32,
        }

        async fn file_shop_part(f: &Fixture) -> Filed {
            let job_id = add_job(f, "Rear brakes").await;
            let need_id = add_need(f, job_id, "Brake fluid", Some(Supplier::Shop)).await;
            let created = create(
                &f.db,
                f.vehicle,
                NewServiceRecord {
                    work_item_ids: Some(vec![job_id]),
                    parts: Some(vec![NewPart {
                        name: "DOT4 fluid".into(),
                        supplied_by: Some(Supplier::Shop),
                        need_id: Some(need_id),
                        ..Default::default()
                    }]),
                    ..invoice(f)
                },
            )
            .await
            .unwrap();
            Filed {
                job_id,
                need_id,
                service_id: created.record.id,
                part_id: created.part_ids[0],
            }
        }

        #[tokio::test]
        async fn work_that_did_not_happen_keeps_a_part_a_later_service_replaced() {
            let f = fixture().await;
            let Filed {
                job_id,
                need_id,
                service_id,
                part_id,
            } = file_shop_part(&f).await;
            set_part_status(&f, part_id, PartStatus::Replaced).await;

            delete(&f.db, f.vehicle, service_id, UNDO).await.unwrap();

            let p = part_svc::get(&f.db, f.vehicle, part_id).await.unwrap();
            assert_eq!(
                (p.status, p.installed_service_id),
                (PartStatus::Replaced, None)
            );
            assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);
            let j = job::get(&f.db, f.vehicle, job_id).await.unwrap();
            assert_eq!(j.status, work_item::JobStatus::Planned);
        }

        #[tokio::test]
        async fn work_done_another_way_stays_done_without_the_invoice() {
            let f = fixture().await;
            let Filed {
                job_id,
                need_id,
                service_id,
                part_id,
            } = file_shop_part(&f).await;
            let installed_on = part_svc::get(&f.db, f.vehicle, part_id)
                .await
                .unwrap()
                .installed_date;

            let deleted = delete(
                &f.db,
                f.vehicle,
                service_id,
                Some(CompletedWork::DoneAnotherWay),
            )
            .await
            .unwrap();

            assert_eq!(deleted.kept_part_ids, vec![part_id]);
            crate::test_support::assert_empty(&deleted.reopened_appointment_ids);
            let p = part_svc::get(&f.db, f.vehicle, part_id).await.unwrap();
            assert_eq!(
                (p.status, p.installed_service_id, p.installed_date),
                (PartStatus::Installed, None, installed_on)
            );
            assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);
            let j = job::get(&f.db, f.vehicle, job_id).await.unwrap();
            assert_eq!(
                (j.status, j.completed_service_id),
                (work_item::JobStatus::Done, None)
            );
        }

        #[tokio::test]
        async fn a_service_with_completed_work_refuses_a_delete_with_no_choice() {
            let f = fixture().await;
            let Filed {
                job_id,
                need_id,
                service_id,
                part_id,
            } = file_shop_part(&f).await;

            let err = delete(&f.db, f.vehicle, service_id, None)
                .await
                .unwrap_err();

            assert_eq!(
                err.to_string(),
                format!(
                    "Service {service_id} recorded this work as done: job {job_id} \"Rear \
                     brakes\", part {part_id} \"DOT4 fluid\". Deleting it needs to know whether \
                     that work did not happen or was done another way. Nothing was deleted."
                )
            );
            assert!(get(&f.db, f.vehicle, service_id).await.is_ok());
            assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);
        }

        #[tokio::test]
        async fn correcting_the_parts_keeps_a_replaced_shop_part_on_its_need() {
            let f = fixture().await;
            let Filed {
                need_id,
                service_id,
                part_id,
                ..
            } = file_shop_part(&f).await;
            set_part_status(&f, part_id, PartStatus::Replaced).await;

            update(
                &f.db,
                f.vehicle,
                service_id,
                UpdateServiceRecord {
                    part_ids: Some(Vec::new()),
                    completed_work: Some(CompletedWork::DidNotHappen),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

            let p = part_svc::get(&f.db, f.vehicle, part_id).await.unwrap();
            assert_eq!(p.status, PartStatus::Replaced);
            assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);
        }

        #[tokio::test]
        async fn appointment_from_another_vehicle_reads_as_missing() {
            let f = fixture().await;
            let other = VehicleFixture::new().name("Other").insert_id(&f.db).await;
            let missing = create(
                &f.db,
                other,
                NewServiceRecord {
                    appointment_id: Some(99_999),
                    ..invoice(&f)
                },
            )
            .await
            .unwrap_err();
            let wrong = create(&f.db, other, invoice(&f)).await.unwrap_err();
            assert!(matches!(wrong, DomainError::NotFound(_)));
            assert_eq!(
                wrong.to_string().replace(&f.appointment.to_string(), "N"),
                missing.to_string().replace("99999", "N")
            );
            assert_eq!(row_count::<service_record::Entity>(&f.db).await, 0);

            // The same guard covers a correction.
            let own = create(
                &f.db,
                other,
                NewServiceRecord {
                    appointment_id: None,
                    ..invoice(&f)
                },
            )
            .await
            .unwrap();
            let err = update(
                &f.db,
                other,
                own.record.id,
                UpdateServiceRecord {
                    appointment_id: Some(Some(f.appointment)),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
            assert!(matches!(err, DomainError::NotFound(_)));
        }

        #[tokio::test]
        async fn cancelled_or_closed_appointment_is_refused() {
            let f = fixture().await;
            appointment::cancel(&f.db, f.vehicle, f.appointment)
                .await
                .unwrap();
            let err = create(&f.db, f.vehicle, invoice(&f)).await.unwrap_err();
            assert!(matches!(err, DomainError::BadRequest(_)));
            assert_eq!(row_count::<service_record::Entity>(&f.db).await, 0);
        }

        #[tokio::test]
        async fn correcting_a_service_sets_changes_and_clears_its_appointment() {
            let f = fixture().await;
            let second = appointment::create(
                &f.db,
                f.vehicle,
                NewAppointment {
                    purpose: "Oil".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .id;
            let created = create(
                &f.db,
                f.vehicle,
                NewServiceRecord {
                    appointment_id: None,
                    ..invoice(&f)
                },
            )
            .await
            .unwrap();
            assert_eq!(created.record.appointment_id, None);
            let set = |value| UpdateServiceRecord {
                appointment_id: Some(value),
                ..Default::default()
            };
            let id = created.record.id;
            let r = update(&f.db, f.vehicle, id, set(Some(f.appointment)))
                .await
                .unwrap();
            assert_eq!(r.record.appointment_id, Some(f.appointment));
            let r = update(&f.db, f.vehicle, id, set(Some(second)))
                .await
                .unwrap();
            assert_eq!(r.record.appointment_id, Some(second));
            // An absent key leaves it alone.
            let r = update(&f.db, f.vehicle, id, UpdateServiceRecord::default())
                .await
                .unwrap();
            assert_eq!(r.record.appointment_id, Some(second));
            let r = update(&f.db, f.vehicle, id, set(None)).await.unwrap();
            assert_eq!(r.record.appointment_id, None);
        }

        #[tokio::test]
        async fn inline_part_need_from_another_vehicle_reads_as_missing() {
            let f = fixture().await;
            let other = VehicleFixture::new().name("Other").insert_id(&f.db).await;
            let job_id = add_job(&f, "Rear brakes").await;
            let need_id = add_need(&f, job_id, "Pads", Some(Supplier::Shop)).await;
            let inline = |need_id| NewServiceRecord {
                appointment_id: None,
                parts: Some(vec![NewPart {
                    name: "Pads".into(),
                    need_id: Some(need_id),
                    ..Default::default()
                }]),
                ..invoice(&f)
            };
            let missing = create(&f.db, other, inline(99_999)).await.unwrap_err();
            let wrong = create(&f.db, other, inline(need_id)).await.unwrap_err();
            assert!(matches!(wrong, DomainError::NotFound(_)));
            assert_eq!(
                wrong.to_string().replace(&need_id.to_string(), "N"),
                missing.to_string().replace("99999", "N")
            );
            assert_eq!(row_count::<service_record::Entity>(&f.db).await, 0);
            assert_eq!(row_count::<part::Entity>(&f.db).await, 0);
        }

        #[tokio::test]
        async fn one_bad_need_id_commits_nothing() {
            let f = fixture().await;
            let job_id = add_job(&f, "Rear brakes").await;
            let good = add_need(&f, job_id, "Pads", Some(Supplier::Shop)).await;
            let linked = add_need(&f, job_id, "Sensor", None).await;
            link_new_part(&f, linked, false).await;
            let parts_before = row_count::<part::Entity>(&f.db).await;

            for (bad, expect_missing) in [(99_999, true), (linked, false)] {
                let err = create(
                    &f.db,
                    f.vehicle,
                    NewServiceRecord {
                        work_item_ids: Some(vec![job_id]),
                        parts: Some(vec![
                            NewPart {
                                name: "Pads".into(),
                                need_id: Some(good),
                                ..Default::default()
                            },
                            NewPart {
                                name: "Other".into(),
                                need_id: Some(bad),
                                ..Default::default()
                            },
                        ]),
                        ..invoice(&f)
                    },
                )
                .await
                .unwrap_err();
                // A nonexistent need reads as missing; a need that already has a
                // part is a malformed request.
                if expect_missing {
                    assert!(matches!(err, DomainError::NotFound(_)), "got: {err:?}");
                } else {
                    assert!(matches!(err, DomainError::BadRequest(_)), "got: {err:?}");
                }
                assert_eq!(row_count::<service_record::Entity>(&f.db).await, 0);
                assert_eq!(row_count::<part::Entity>(&f.db).await, parts_before);
                assert_eq!(job_status(&f, job_id).await, JobStatus::Planned);
                let n = job_need::Entity::find_by_id(good)
                    .one(&f.db)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(n.part_id, None);
            }
        }

        #[tokio::test]
        async fn re_recording_an_invoice_ref_with_another_appointment_writes_nothing() {
            let f = fixture().await;
            let second = appointment::create(
                &f.db,
                f.vehicle,
                NewAppointment {
                    purpose: "Wipers".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .id;
            let job_id = add_job(&f, "Rear brakes").await;
            let need_id = add_need(&f, job_id, "Pads", Some(Supplier::Shop)).await;
            let first = create(
                &f.db,
                f.vehicle,
                NewServiceRecord {
                    invoice_ref: Some("RO-1".into()),
                    ..invoice(&f)
                },
            )
            .await
            .unwrap();
            let parts_before = row_count::<part::Entity>(&f.db).await;

            let again = create(
                &f.db,
                f.vehicle,
                NewServiceRecord {
                    invoice_ref: Some("RO-1".into()),
                    appointment_id: Some(second),
                    parts: Some(vec![NewPart {
                        name: "Pads".into(),
                        need_id: Some(need_id),
                        ..Default::default()
                    }]),
                    ..invoice(&f)
                },
            )
            .await
            .unwrap();

            assert!(again.already_recorded);
            assert_eq!(again.record.id, first.record.id);
            assert_eq!(again.record.appointment_id, Some(f.appointment));
            assert_eq!(row_count::<service_record::Entity>(&f.db).await, 1);
            assert_eq!(row_count::<part::Entity>(&f.db).await, parts_before);
            assert_eq!(stage_of(&f, need_id).await, need::Stage::ShopSupplies);
            let n = job_need::Entity::find_by_id(need_id)
                .one(&f.db)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(n.part_id, None);
        }

        #[tokio::test]
        async fn deleting_a_service_leaves_an_ordered_need_ordered() {
            let f = fixture().await;
            let job_id = add_job(&f, "Rear brakes").await;
            let need_id = add_need(&f, job_id, "Pads", None).await;
            // Ordered but never received.
            let part_id = link_new_part(&f, need_id, false).await;
            let created = create(
                &f.db,
                f.vehicle,
                NewServiceRecord {
                    part_ids: Some(vec![part_id]),
                    work_item_ids: Some(vec![job_id]),
                    ..invoice(&f)
                },
            )
            .await
            .unwrap();
            assert_eq!(stage_of(&f, need_id).await, need::Stage::Installed);

            delete(&f.db, f.vehicle, created.record.id, UNDO)
                .await
                .unwrap();

            assert_eq!(stage_of(&f, need_id).await, need::Stage::Ordered);
            assert_eq!(job_status(&f, job_id).await, JobStatus::Planned);
        }

        #[tokio::test]
        async fn filing_an_invoice_on_a_closed_appointment_gives_a_neutral_message() {
            let f = fixture().await;
            appointment::cancel(&f.db, f.vehicle, f.appointment)
                .await
                .unwrap();

            let err = create(&f.db, f.vehicle, invoice(&f)).await.unwrap_err();

            assert!(matches!(err, DomainError::BadRequest(_)), "got: {err:?}");
            assert_eq!(
                err.to_string(),
                format!(
                    "Appointment {} is cancelled, so it takes no new work",
                    f.appointment
                )
            );
            assert_eq!(row_count::<service_record::Entity>(&f.db).await, 0);
        }
    }

    // `update` taking recorded work off a service: the jobs, parts and
    // recalls its replacement lists leave out.
    mod corrections {
        use super::*;
        use crate::{
            inputs::{appointment::NewAppointment, work_item::NewWorkItem},
            services::{appointment, work_item as job},
        };

        async fn planned(db: &DatabaseConnection, v: i32, title: &str, appt: Option<i32>) -> i32 {
            job::create(
                db,
                v,
                NewWorkItem {
                    title: title.into(),
                    appointment_id: appt,
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .id
        }

        async fn completing(db: &DatabaseConnection, v: i32, jobs: Vec<i32>) -> i32 {
            create(
                db,
                v,
                NewServiceRecord {
                    service_date: date("2026-11-01"),
                    work_item_ids: Some(jobs),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .record
            .id
        }

        fn jobs(ids: Vec<i32>, completed: Option<CompletedWork>) -> UpdateServiceRecord {
            UpdateServiceRecord {
                work_item_ids: Some(ids),
                completed_work: completed,
                ..Default::default()
            }
        }

        #[tokio::test]
        async fn dropping_a_completed_job_needs_an_answer_and_changes_nothing_without_one() {
            let db = test_db().await;
            let v = seed_vehicle(&db).await;
            let j = planned(&db, v, "Flush brake fluid", None).await;
            let svc = completing(&db, v, vec![j]).await;

            let err = update(
                &db,
                v,
                svc,
                UpdateServiceRecord {
                    notes: Some(Some("edited".into())),
                    ..jobs(vec![], None)
                },
            )
            .await
            .unwrap_err();
            let DomainError::BadRequest(message) = err else {
                panic!("{err:?}");
            };
            assert!(
                message.contains(&format!("job {j} \"Flush brake fluid\"")),
                "{message}"
            );
            assert!(message.contains("Taking it off the service"), "{message}");
            assert_eq!(job::get(&db, v, j).await.unwrap().status, JobStatus::Done);
            assert_eq!(get(&db, v, svc).await.unwrap().record.notes, None);
        }

        #[tokio::test]
        async fn did_not_happen_puts_the_job_back_and_reopens_its_closed_appointment() {
            let db = test_db().await;
            let v = seed_vehicle(&db).await;
            let appt = appointment::create(
                &db,
                v,
                NewAppointment {
                    purpose: "Brakes".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .id;
            let j = planned(&db, v, "Pads", Some(appt)).await;
            let svc = completing(&db, v, vec![j]).await;
            appointment::close(&db, v, appt).await.unwrap();

            let out = update(&db, v, svc, jobs(vec![], Some(CompletedWork::DidNotHappen)))
                .await
                .unwrap();
            let back = job::get(&db, v, j).await.unwrap();
            assert_eq!(back.status, JobStatus::Planned);
            assert_eq!(back.completed_service_id, None);
            assert_eq!(out.reopened_appointment_ids, vec![appt]);
            crate::test_support::assert_empty(&out.completed_work_items);
        }

        #[tokio::test]
        async fn done_another_way_keeps_the_job_done_without_the_service() {
            let db = test_db().await;
            let v = seed_vehicle(&db).await;
            let j = planned(&db, v, "Pads", None).await;
            let svc = completing(&db, v, vec![j]).await;

            update(
                &db,
                v,
                svc,
                jobs(vec![], Some(CompletedWork::DoneAnotherWay)),
            )
            .await
            .unwrap();
            let kept = job::get(&db, v, j).await.unwrap();
            assert_eq!(kept.status, JobStatus::Done);
            assert_eq!(kept.completed_service_id, None);
        }

        #[tokio::test]
        async fn a_job_added_to_the_list_is_completed_and_a_kept_one_is_left_alone() {
            let db = test_db().await;
            let v = seed_vehicle(&db).await;
            let first = planned(&db, v, "Pads", None).await;
            let second = planned(&db, v, "Rotors", None).await;
            let svc = completing(&db, v, vec![first]).await;

            let out = update(&db, v, svc, jobs(vec![first, second], None))
                .await
                .unwrap();
            let ids: Vec<i32> = out.completed_work_items.iter().map(|w| w.id).collect();
            assert_eq!(ids, vec![first, second]);
            assert_eq!(
                job::get(&db, v, second).await.unwrap().completed_service_id,
                Some(svc)
            );

            // A job another service completed is not planned, so it is refused.
            let elsewhere = planned(&db, v, "Wipers", None).await;
            completing(&db, v, vec![elsewhere]).await;
            let err = update(&db, v, svc, jobs(vec![first, second, elsewhere], None))
                .await
                .unwrap_err();
            assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        }

        #[tokio::test]
        async fn a_job_of_another_vehicle_reads_as_missing() {
            let db = test_db().await;
            let v = seed_vehicle(&db).await;
            let other = VehicleFixture::new().name("Other").insert_id(&db).await;
            let theirs = planned(&db, other, "Pads", None).await;
            let svc = completing(&db, v, vec![]).await;

            let wrong = update(&db, v, svc, jobs(vec![theirs], None))
                .await
                .unwrap_err();
            let missing = update(&db, v, svc, jobs(vec![99_999], None))
                .await
                .unwrap_err();
            assert_eq!(
                wrong.to_string().replace(&theirs.to_string(), "N"),
                missing.to_string().replace("99999", "N")
            );
        }

        #[tokio::test]
        async fn a_part_and_a_recall_done_another_way_keep_their_state() {
            let db = test_db().await;
            let v = seed_vehicle(&db).await;
            let finding = seed_recall(&db, v, "24V555").await;
            let p = part_svc::create(
                &db,
                v,
                crate::inputs::part::NewPart {
                    name: "Pads".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .id;
            let svc = create(
                &db,
                v,
                NewServiceRecord {
                    service_date: date("2026-11-01"),
                    part_ids: Some(vec![p]),
                    recall_finding_ids: Some(vec![finding]),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .record
            .id;

            let drop_both = || UpdateServiceRecord {
                part_ids: Some(vec![]),
                recall_finding_ids: Some(vec![]),
                ..Default::default()
            };
            let err = update(&db, v, svc, drop_both()).await.unwrap_err();
            assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");

            update(
                &db,
                v,
                svc,
                UpdateServiceRecord {
                    completed_work: Some(CompletedWork::DoneAnotherWay),
                    ..drop_both()
                },
            )
            .await
            .unwrap();
            let part = part_svc::get(&db, v, p).await.unwrap();
            assert_eq!(part.status, PartStatus::Installed);
            assert_eq!(part.installed_service_id, None);
            assert_eq!(
                recall_status(&db, v).await,
                vec![(recall::ShownRecallStatus::Done, None)]
            );
        }

        async fn job_from(db: &DatabaseConnection, v: i32, incident: i32, finding: i32) -> i32 {
            job::create(
                db,
                v,
                NewWorkItem {
                    title: "Replace N80 valve".into(),
                    incident_id: Some(incident),
                    research_finding_id: Some(finding),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .id
        }

        async fn resolved(db: &DatabaseConnection, v: i32, id: i32) -> bool {
            incident::get(db, v, id).await.unwrap().incident.resolved
        }

        #[tokio::test]
        async fn a_job_added_resolves_its_incident_and_performs_its_recall() {
            let db = test_db().await;
            let v = seed_vehicle(&db).await;
            let incident = observation(&db, v, None).await;
            let finding = seed_recall(&db, v, "24V777").await;
            let j = job_from(&db, v, incident, finding).await;
            let svc = completing(&db, v, vec![]).await;

            let out = update(&db, v, svc, jobs(vec![j], None)).await.unwrap();
            assert!(resolved(&db, v, incident).await);
            let performed: Vec<i32> = out.performed_recalls.iter().map(|r| r.finding_id).collect();
            assert_eq!(performed, vec![finding]);
        }

        #[tokio::test]
        async fn a_job_taken_off_as_not_done_takes_its_incident_recall_and_parts_with_it() {
            use crate::entities::job_need;
            let db = test_db().await;
            let v = seed_vehicle(&db).await;
            let incident = observation(&db, v, None).await;
            let finding = seed_recall(&db, v, "24V777").await;
            let j = job_from(&db, v, incident, finding).await;
            let p = part_svc::create(
                &db,
                v,
                crate::inputs::part::NewPart {
                    name: "N80 valve".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .id;
            job_need::ActiveModel {
                work_item_id: Set(j),
                name: Set("Valve".into()),
                quantity: Set(1),
                part_id: Set(Some(p)),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
            let svc = create(
                &db,
                v,
                NewServiceRecord {
                    service_date: date("2026-11-01"),
                    work_item_ids: Some(vec![j]),
                    part_ids: Some(vec![p]),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .record
            .id;
            assert!(resolved(&db, v, incident).await);

            let err = update(&db, v, svc, jobs(vec![], None)).await.unwrap_err();
            let DomainError::BadRequest(message) = err else {
                panic!("{err:?}");
            };
            for named in [
                format!("job {j}"),
                format!("part {p}"),
                format!("incident {incident}"),
                format!("recall {finding}"),
            ] {
                assert!(message.contains(&named), "{named} in {message}");
            }

            let out = update(&db, v, svc, jobs(vec![], Some(CompletedWork::DidNotHappen)))
                .await
                .unwrap();
            assert!(!resolved(&db, v, incident).await);
            crate::test_support::assert_empty(&out.performed_recalls);
            crate::test_support::assert_empty(&out.part_ids);
            assert_eq!(
                part_svc::get(&db, v, p).await.unwrap().status,
                PartStatus::Purchased
            );
        }

        #[tokio::test]
        async fn a_sent_schedule_list_wins_over_an_added_jobs_schedule_item() {
            let db = test_db().await;
            let v = seed_vehicle(&db).await;
            let item = crate::services::schedule::create_for_vehicle(
                &db,
                v,
                crate::inputs::schedule::NewScheduleItem {
                    name: "Brake fluid".into(),
                    interval_months: Some(24),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .id;
            let j = job::create(
                &db,
                v,
                NewWorkItem {
                    title: "Flush brake fluid".into(),
                    schedule_item_id: Some(item),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .id;
            let svc = completing(&db, v, vec![]).await;

            let out = update(
                &db,
                v,
                svc,
                UpdateServiceRecord {
                    schedule_item_ids: Some(vec![]),
                    ..jobs(vec![j], None)
                },
            )
            .await
            .unwrap();
            crate::test_support::assert_empty(&out.schedule_item_ids);
        }

        #[tokio::test]
        async fn an_incident_a_kept_job_shares_stays_resolved() {
            let db = test_db().await;
            let v = seed_vehicle(&db).await;
            let incident = observation(&db, v, None).await;
            let finding = seed_recall(&db, v, "24V777").await;
            let a = job_from(&db, v, incident, finding).await;
            let b = job_from(&db, v, incident, finding).await;
            let svc = completing(&db, v, vec![a, b]).await;

            let out = update(
                &db,
                v,
                svc,
                jobs(vec![b], Some(CompletedWork::DidNotHappen)),
            )
            .await
            .unwrap();
            assert!(resolved(&db, v, incident).await);
            let performed: Vec<i32> = out.performed_recalls.iter().map(|r| r.finding_id).collect();
            assert_eq!(performed, vec![finding]);
        }

        #[tokio::test]
        async fn a_wrong_id_is_not_found_before_any_answer_is_asked_for() {
            let db = test_db().await;
            let v = seed_vehicle(&db).await;
            let j = planned(&db, v, "Pads", None).await;
            let svc = completing(&db, v, vec![j]).await;

            let err = update(&db, v, svc, jobs(vec![99_999], None))
                .await
                .unwrap_err();
            assert!(matches!(err, DomainError::NotFound(_)), "{err:?}");
        }
    }
}
