use sea_orm::*;
use serde::Serialize;

use crate::dates::DayOrTime;

use crate::{
    entities::{
        incident::{self, IncidentCategory},
        incident_service_link::{self, LinkRole},
        job_need, mileage_log,
        part::{self, PartStatus},
        service_record::{self, Payer, Performer},
        service_record_line_item, service_schedule_link,
        work_item::{self as job, JobStatus},
    },
    error::{DomainError, DomainResult},
    inputs::{
        part::{Install, NewFitting, NewPart},
        service_record::{NewLineItem, NewServiceRecord, UpdateServiceRecord},
        shop::ShopChoice,
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
/// `shop` is the argument that names the service's shop, when it has one.
fn validate_who_and_when(
    service_date: chrono::NaiveDate,
    end_date: Option<chrono::NaiveDate>,
    performed_by: Performer,
    shop: Option<&'static str>,
) -> DomainResult<()> {
    if end_date.is_some_and(|end| end < service_date) {
        return Err(DomainError::invalid(
            "end_date",
            "must not be before service_date",
        ));
    }
    if performed_by == Performer::Owner
        && let Some(field) = shop
    {
        return Err(DomainError::invalid(
            field,
            "a self-performed service has no shop",
        ));
    }
    Ok(())
}

fn validate_mileage(mileage: Option<i32>) -> DomainResult<()> {
    if mileage.is_some_and(|miles| miles < 0) {
        return Err(DomainError::invalid("mileage", "must not be negative"));
    }
    Ok(())
}

/// Who paid for a line: its own `paid_by` when it has one, otherwise the
/// service's.
pub(crate) fn effective_payer(line_paid_by: Option<Payer>, service_paid_by: Payer) -> Payer {
    line_paid_by.unwrap_or(service_paid_by)
}

#[derive(Debug, Serialize)]
pub struct ServiceRecordWithLinks {
    #[serde(flatten)]
    pub record: service_record::Model,
    /// The name of the shop `shop_id` names, read from the shop directory.
    pub shop_name: Option<String>,
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
    /// Ids of the schedule items that stopped counting from this service
    /// because a correction took the job that brought each one off it or
    /// left the item out of `schedule_item_ids`, in ascending order. Filled
    /// only by `update`; omitted from JSON when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub released_schedule_item_ids: Vec<i32>,
    /// Ids of the incidents that stay resolved by this service although a
    /// correction took the job filed from each one off it, because the
    /// invoice names them, in ascending order. Filled only by `update`;
    /// omitted from JSON when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incidents_kept_resolved: Vec<i32>,
    /// Ids sent in `resolved_incident_ids` that a correction did not store,
    /// because the same call took the job filed from each one off the
    /// service. Filled only by `update`; omitted from JSON when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incident_names_ignored: Vec<i32>,
    /// The vehicle's incidents this service could have fixed and did not:
    /// unresolved once the call's work is recorded, dated no later than the
    /// service's last day, and neither noticed during it nor brought by one
    /// of its jobs. Newest first and without notes, so the caller can ask
    /// whether the work fixed any of them. `Some`, which may be empty, from a
    /// `create` that writes a record and from an `update` that sends
    /// `work_item_ids` or `resolved_incident_ids`; `None`, and omitted from
    /// JSON, when the call did not look.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_incidents: Option<Vec<OpenIncident>>,
}

/// An unresolved incident as a service reply lists it.
#[derive(Clone, Debug, Serialize)]
pub struct OpenIncident {
    pub id: i32,
    pub title: String,
    pub category: IncidentCategory,
    pub occurred_at: DayOrTime,
    /// Ids of the planned jobs filed from the incident, in ascending order.
    /// Completing one with a service resolves the incident.
    pub planned_work_item_ids: Vec<i32>,
}

/// The unresolved incidents `service` could have fixed, each with the planned
/// jobs filed from it; `ServiceRecordWithLinks::open_incidents` states which
/// ones are left out.
async fn open_incidents(
    db: &impl ConnectionTrait,
    service: &ServiceRecordWithLinks,
) -> DomainResult<Vec<OpenIncident>> {
    let last_day = service
        .record
        .end_date
        .unwrap_or(service.record.service_date);
    // An incident a job of this service brings stays in `resolved_incidents`
    // after the owner reopens it, so it is left out here and never reads as
    // both.
    let linked: std::collections::HashSet<i32> = service
        .noticed_incidents
        .iter()
        .chain(&service.resolved_incidents)
        .map(|i| i.id)
        .collect();
    let open: Vec<incident::Model> = super::upcoming::open_incidents(db, service.record.vehicle_id)
        .await?
        .into_iter()
        .filter(|i| i.occurred_at.day() <= last_day && !linked.contains(&i.id))
        .collect();
    if open.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<i32> = open.iter().map(|i| i.id).collect();
    let mut planned: std::collections::HashMap<i32, Vec<i32>> = std::collections::HashMap::new();
    let jobs = job::Entity::find()
        .filter(job::Column::IncidentId.is_in(ids))
        .filter(job::Column::Status.eq(JobStatus::Planned))
        .order_by_asc(job::Column::Id)
        .all(db)
        .await?;
    for j in jobs {
        if let Some(incident_id) = j.incident_id {
            planned.entry(incident_id).or_default().push(j.id);
        }
    }
    Ok(open
        .into_iter()
        .map(|i| OpenIncident {
            planned_work_item_ids: planned.remove(&i.id).unwrap_or_default(),
            id: i.id,
            title: i.title,
            category: i.category,
            occurred_at: i.occurred_at,
        })
        .collect())
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
    /// Under `resolved_incidents`, whether the service holds a stored link
    /// to the incident, which is how an invoice's naming of it is kept. When
    /// false, the service resolves it only through a job filed from it. Left
    /// out under `noticed_incidents`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub named_by_invoice: Option<bool>,
}

/// The incident links of a batch of service records, keyed by record id.
#[derive(Default)]
struct IncidentLinks {
    noticed: std::collections::HashMap<i32, Vec<IncidentRef>>,
    resolved: std::collections::HashMap<i32, Vec<IncidentRef>>,
}

/// Loads the noticed and resolved incidents of each record with nine batched
/// queries however many records there are.
async fn load_incident_links(
    db: &impl ConnectionTrait,
    record_ids: &[i32],
) -> DomainResult<IncidentLinks> {
    let mut out = IncidentLinks::default();
    if record_ids.is_empty() {
        return Ok(out);
    }
    // A resolving link is stored or read from a job the service completes,
    // so both kinds are gathered as link rows.
    let resolved = |(incident_id, service_record_id)| incident_service_link::Model {
        incident_id,
        service_record_id,
        role: LinkRole::Resolved,
    };
    let noticed = incident_service_link::Entity::find()
        .filter(incident_service_link::Column::Role.eq(LinkRole::Noticed));
    let mut mine = noticed
        .clone()
        .filter(incident_service_link::Column::ServiceRecordId.is_in(record_ids.to_vec()))
        .all(db)
        .await?;
    let resolving = incident_svc::resolvers_of_services(db, record_ids).await?;
    mine.extend(resolving.into_iter().map(resolved));
    if mine.is_empty() {
        return Ok(out);
    }
    let named = incident_svc::named_resolved_by(db, record_ids).await?;
    let incident_ids: Vec<i32> = mine.iter().map(|l| l.incident_id).collect();
    let mut all_links = noticed
        .filter(incident_service_link::Column::IncidentId.is_in(incident_ids.clone()))
        .all(db)
        .await?;
    let resolving = incident_svc::resolvers_of_incidents(db, &incident_ids).await?;
    all_links.extend(resolving.into_iter().map(resolved));
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
                named_by_invoice: (link.role == LinkRole::Resolved)
                    .then(|| named.contains(&(link.incident_id, link.service_record_id))),
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
            shop_name: None,
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
            released_schedule_item_ids: Vec::new(),
            incidents_kept_resolved: Vec::new(),
            incident_names_ignored: Vec::new(),
            open_incidents: None,
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
    let schedule_item_ids = schedule::links_of_services(db, &[record.id])
        .await?
        .into_iter()
        .map(|(_, item_id)| item_id)
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

    let shop_name = super::shop::name_of(db, record.shop_id).await?;
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
    links.shop_name = shop_name;
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
        source: Set(Some(mileage_log::SERVICE_SOURCE.to_owned())),
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

    let all_links = schedule::links_of_services(db, &record_ids).await?;

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
    let shop_names = super::shop::names(db, records.iter().filter_map(|r| r.shop_id)).await?;

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
                .filter(|(service_id, _)| *service_id == record.id)
                .map(|(_, item_id)| *item_id)
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
            let shop_name = record.shop_id.and_then(|id| shop_names.get(&id).cloned());
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
            links.shop_name = shop_name;
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

/// Links the record to the incidents its invoice says it fixed and the ones
/// it noticed. A completed job that came from an incident fixes that incident
/// too, so the incident is marked resolved.
async fn link_incidents(
    txn: &impl ConnectionTrait,
    vehicle_id: i32,
    record_id: i32,
    resolved_ids: Vec<i32>,
    noticed_ids: Vec<i32>,
    completed: &[crate::entities::work_item::Model],
) -> DomainResult<()> {
    // No link is stored for a job's incident, because the job names the
    // service, and a stored link then always means the invoice named it.
    let from_jobs: Vec<i32> = completed.iter().filter_map(|w| w.incident_id).collect();
    incident_svc::mark_resolved(txn, vehicle_id, &super::dedup_ids(&from_jobs)).await?;
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

    validate_mileage(input.mileage)?;
    let performed_by = input.performed_by.unwrap_or(Performer::Shop);
    // A self-performed service is refused before its shop is looked up, so
    // the refusal names the real problem.
    validate_who_and_when(
        input.service_date,
        input.end_date,
        performed_by,
        input
            .shop
            .as_ref()
            .filter(|c| !c.is_blank())
            .map(ShopChoice::field),
    )?;
    let shop_id = super::shop::resolve(db, input.shop.as_ref()).await?;
    let line_items_input = input.line_items.unwrap_or_default();
    if let Some(appointment_id) = input.appointment_id {
        appointment::require_assignable(db, vehicle_id, appointment_id).await?;
    }

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
        labor_cost_cents: Set(input.labor_cost_cents),
        total_cost_cents: Set(input.total_cost_cents),
        shop_id: Set(shop_id),
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
    created.open_incidents = Some(open_incidents(&txn, &created).await?);

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
    let changes_resolved_work =
        input.work_item_ids.is_some() || input.resolved_incident_ids.is_some();
    // What the correction takes off the service is known before anything is
    // written, so a refusal names the work it would change and writes nothing.
    let recorded = if input.work_item_ids.is_some()
        || input.part_ids.is_some()
        || input.recall_finding_ids.is_some()
        || input.resolved_incident_ids.is_some()
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
    for &incident_id in input.resolved_incident_ids.iter().flatten() {
        incident_svc::require_incident(db, vehicle_id, incident_id).await?;
    }
    let completed_before = recorded.job_ids();
    let recalls_before: Vec<i32> = recorded.recalls.iter().map(|r| r.finding_id).collect();
    // A call that sends no job list keeps the jobs the service completed.
    let kept_jobs = if input.work_item_ids.is_some() {
        listed_jobs
    } else {
        recorded.jobs.clone()
    };
    let removal = recorded
        .left_out_by(db, existing.id, &input, &kept_jobs)
        .await?;
    let choice = removal.choice(existing.id, ChoiceFor::Correction, input.completed_work)?;

    let named_shop = match &input.shop {
        Some(Some(choice)) => (!choice.is_blank()).then(|| choice.field()),
        Some(None) => None,
        None => existing.shop_id.map(|_| "shop_id"),
    };
    validate_mileage(input.mileage.flatten())?;
    validate_who_and_when(
        input.service_date.unwrap_or(existing.service_date),
        input.end_date.unwrap_or(existing.end_date),
        input.performed_by.unwrap_or(existing.performed_by),
        named_shop,
    )?;
    let shop_id = super::shop::resolve_change(db, input.shop.as_ref()).await?;
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
    if let Some(v) = input.labor_cost_cents {
        active.labor_cost_cents = Set(v);
    }
    if let Some(v) = input.total_cost_cents {
        active.total_cost_cents = Set(v);
    }
    if let Some(v) = shop_id {
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
    let named: Vec<i32> = removal
        .jobs
        .iter()
        .filter_map(|j| j.schedule_item_id)
        .collect();
    let released_items: Vec<i32> = schedule::effective_ids(&txn, vehicle_id, &named)
        .await?
        .into_values()
        .collect();
    let satisfied_before = schedule::links_of_services(&txn, &[record.id]).await?;
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
    if let Some(named) = &removal.names_to_store {
        incident_svc::replace_named(&txn, vehicle_id, record.id, named).await?;
    }
    // The names the call sent that the invoice does not store, which the
    // reply lists in `incident_names_ignored`.
    let mut names_ignored: Vec<i32> = input
        .resolved_incident_ids
        .iter()
        .flatten()
        .copied()
        .filter(|id| !removal.names_to_store.iter().flatten().any(|n| n == id))
        .collect();
    names_ignored.sort_unstable();
    names_ignored.dedup();
    // These are the incidents the correction took off, which no other job of
    // this service, no link of its invoice and no other service still
    // resolves.
    let incident_ids: Vec<i32> = removal.incidents.iter().map(|i| i.id).collect();
    incident_svc::release_resolved(&txn, incident_ids, choice).await?;

    if let Some(item_ids) = input.schedule_item_ids {
        // A read lists a job's item beside the named ones, so a list sent
        // with the call that takes the job off would store the item the job
        // is taking with it. Both sides name the item the vehicle tracks.
        let mut item_ids = schedule::tracked_ids(&txn, vehicle_id, &item_ids).await?;
        item_ids.retain(|id| !released_items.contains(id));
        schedule::insert_service_links(&txn, record.id, &item_ids, LinkMode::Replace).await?;
    }

    let parts_listed = input.part_ids.is_some();
    part_svc::release_from_service(&txn, removal.parts, choice).await?;
    if let Some(new_part_ids) = input.part_ids {
        for &part_id in &new_part_ids {
            part_svc::install(&txn, vehicle_id, part_id, &record, record.mileage.map(Some)).await?;
        }
    }
    if !parts_listed && (record.service_date != date_before || record.mileage != mileage_before) {
        // The parts this service installed carry its date and mileage as
        // their install date and odometer.
        part_svc::follow_service(&txn, &record).await?;
    }

    // A sent list replaces every recall link, and the service still performs
    // the recall of each job it gains and of each job it keeps whose recall
    // it performed before the call, whether or not the list names it. A kept
    // job whose recall the service did not perform gains no link. Without a
    // list only the recalls of the jobs taken off lose their link and only
    // the jobs added bring one.
    let removed_recalls: Vec<i32> = removal.recalls.iter().map(|r| r.finding_id).collect();
    let mut from_jobs: Vec<i32> = added.iter().filter_map(|w| w.research_finding_id).collect();
    if input.recall_finding_ids.is_some() {
        from_jobs.extend(
            kept_jobs
                .iter()
                .filter_map(|w| w.research_finding_id)
                .filter(|id| recalls_before.contains(id)),
        );
    }
    let mut relink = recall::recall_ids_among(&txn, &from_jobs).await?;
    match input.recall_finding_ids {
        Some(recall_ids) => {
            relink.extend(recall_ids);
            recall::unlink_from_service(&txn, record.id, None).await?;
        }
        None => recall::unlink_from_service(&txn, record.id, Some(&removed_recalls)).await?,
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
    let mut released: Vec<i32> = satisfied_before
        .into_iter()
        .map(|(_, item_id)| item_id)
        .filter(|item_id| !updated.schedule_item_ids.contains(item_id))
        .collect();
    released.sort_unstable();
    updated.released_schedule_item_ids = released;
    updated.incident_names_ignored = names_ignored;
    if changes_resolved_work {
        updated.open_incidents = Some(open_incidents(&txn, &updated).await?);
    }
    updated.incidents_kept_resolved = updated
        .resolved_incidents
        .iter()
        .filter(|i| i.resolved && removal.incidents_kept.contains(&i.id))
        .map(|i| i.id)
        .collect();

    txn.commit().await?;

    Ok(updated)
}

/// The work a service records as done: the jobs it completed that still read
/// done, the parts it installed, the incidents only it resolves (leaving out
/// one resolved by hand) and the recalls it performed. Deleting the service
/// releases all of it, and a correction releases what its replacement lists
/// leave out.
#[derive(Default)]
struct RecordedWork {
    jobs: Vec<job::Model>,
    parts: Vec<part::Model>,
    incidents: Vec<incident::Model>,
    recalls: Vec<RecallRef>,
    /// Filled by `left_out_by`: the incidents of the jobs taken off that
    /// the invoice still names.
    incidents_kept: Vec<i32>,
    /// Filled by `left_out_by` when the call sends `resolved_incident_ids`:
    /// the names to store, which leave out the incident of a job the same
    /// call takes off unless the invoice already names it.
    names_to_store: Option<Vec<i32>>,
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
            incidents: incident_svc::resolved_by_service(db, service_id).await?,
            recalls,
            incidents_kept: Vec::new(),
            names_to_store: None,
        })
    }

    /// The part of this work that `input` takes off the service: the jobs,
    /// parts and recalls its replacement lists leave out, and with each job
    /// taken off the work that hangs off it (the incident and the recall it
    /// came from, and the parts fitted to its needs) unless a list the call
    /// sends names that work, and each incident the call's
    /// `resolved_incident_ids` stops naming. An incident or a recall that a
    /// job in `kept_jobs` came from is never taken off, and neither is an
    /// incident the invoice of `service_id` names once the call is applied.
    /// `kept_jobs` are the jobs the service keeps or gains.
    async fn left_out_by(
        mut self,
        db: &impl ConnectionTrait,
        service_id: i32,
        input: &UpdateServiceRecord,
        kept_jobs: &[job::Model],
    ) -> DomainResult<Self> {
        if let Some(list) = &input.work_item_ids {
            self.jobs.retain(|j| !list.contains(&j.id));
        } else {
            self.jobs.clear();
        }
        let job_ids = self.job_ids();
        let released = |id: Option<i32>, of: fn(&job::Model) -> Option<i32>| {
            id.filter(|id| !kept_jobs.iter().any(|j| of(j) == Some(*id)))
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
        // An incident the invoice names stays resolved by this service
        // whatever happens to the job filed from it. The names are the ones
        // the call sends, or the stored ones when it sends none, and a name
        // the call takes off releases the incident like a job taken off does.
        let stored: Vec<i32> = incident_svc::named_resolved_by(db, &[service_id])
            .await?
            .into_iter()
            .map(|(incident_id, _)| incident_id)
            .collect();
        // A read lists a job's incident beside the named ones, so a list
        // sent with the call that takes the job off would name the incident
        // the job is taking with it. Such an id is ignored.
        let named: Vec<i32> = match &input.resolved_incident_ids {
            Some(sent) => sent
                .iter()
                .copied()
                .filter(|id| stored.contains(id) || !incident_ids.contains(id))
                .collect(),
            None => stored.clone(),
        };
        self.names_to_store = input.resolved_incident_ids.is_some().then(|| named.clone());
        let from_names_taken_off: Vec<i32> = stored
            .iter()
            .filter_map(|id| released(Some(*id), |l| l.incident_id))
            .collect();
        self.incidents_kept = incident_ids
            .iter()
            .copied()
            .filter(|id| named.contains(id))
            .collect();
        self.incidents.retain(|i| {
            let taken_off = incident_ids.contains(&i.id) || from_names_taken_off.contains(&i.id);
            taken_off && !named.contains(&i.id)
        });
        match &input.recall_finding_ids {
            Some(list) => self.recalls.retain(|r| {
                released(Some(r.finding_id), |l| l.research_finding_id).is_some()
                    && !list.contains(&r.finding_id)
            }),
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
    /// Ids of the schedule items the service satisfied, in ascending order.
    /// Their reminders count from another service or from none.
    pub released_schedule_item_ids: Vec<i32>,
}

/// Deletes a service record and its owned rows.
///
/// A service that recorded work as done (jobs it completed, parts it
/// installed, incidents only it resolves or recalls it performed) needs
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
    let released_schedule_item_ids = schedule::links_of_services(&txn, &[existing.id])
        .await?
        .into_iter()
        .map(|(_, item_id)| item_id)
        .collect();
    let settled = work_item::release_completed(&txn, &work.job_ids(), choice).await?;
    let finding_ids = work.recalls.iter().map(|r| r.finding_id).collect();
    let incident_ids: Vec<i32> = work.incidents.iter().map(|i| i.id).collect();
    incident_svc::release_resolved(&txn, incident_ids, choice).await?;
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
        released_schedule_item_ids,
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

    recall::unlink_from_service(&txn, existing.id, None).await?;

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

#[cfg(test)]
mod tests;
