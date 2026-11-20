//! Incidents: anything noticed about a vehicle, from a noise or a shop
//! observation to an accident, as one vehicle-scoped record with a
//! constrained category, dated followups, links to the services that noticed
//! and fixed it, and recurrence chains.

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{
        incident, incident_followup, incident_service_link, incident_service_link::LinkRole,
        service_record, work_item,
    },
    error::{DomainError, DomainResult},
    inputs::incident::{NewFollowup, NewIncident, UpdateIncident},
};

/// Fetch an incident scoped to its owning vehicle. A wrong-vehicle lookup is
/// indistinguishable from a nonexistent incident.
pub(crate) async fn require_incident(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<incident::Model> {
    incident::Entity::find_by_id(id)
        .filter(incident::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Incident {id} not found")))
}

/// A service record as seen from an incident: enough to name it without a
/// second lookup.
#[derive(Clone, Debug, Serialize)]
pub struct ServiceRef {
    pub id: i32,
    pub service_date: chrono::NaiveDate,
    pub description: Option<String>,
}

impl From<service_record::Model> for ServiceRef {
    fn from(s: service_record::Model) -> Self {
        Self {
            id: s.id,
            service_date: s.service_date,
            description: s.description,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct IncidentWithDetails {
    #[serde(flatten)]
    pub incident: incident::Model,
    pub followups: Vec<incident_followup::Model>,
    /// The service during which the incident was noticed, if recorded.
    pub noticed_during: Option<ServiceRef>,
    /// The services that fixed the incident, oldest first.
    pub resolved_by: Vec<ServiceRef>,
}

/// Builds the detail shape for each incident with five batched queries
/// (followups, noticing links, resolving links, resolving jobs, services)
/// regardless of how many incidents there are.
async fn with_details(
    db: &impl ConnectionTrait,
    incidents: Vec<incident::Model>,
) -> DomainResult<Vec<IncidentWithDetails>> {
    if incidents.is_empty() {
        return Ok(vec![]);
    }
    let incident_ids: Vec<i32> = incidents.iter().map(|i| i.id).collect();

    let followups = incident_followup::Entity::find()
        .filter(incident_followup::Column::IncidentId.is_in(incident_ids.clone()))
        .order_by_asc(incident_followup::Column::OccurredAt)
        .all(db)
        .await?;
    let noticed = incident_service_link::Entity::find()
        .filter(incident_service_link::Column::IncidentId.is_in(incident_ids.clone()))
        .filter(incident_service_link::Column::Role.eq(LinkRole::Noticed))
        .all(db)
        .await?;
    let resolvers = resolvers_of_incidents(db, &incident_ids).await?;
    let service_ids: Vec<i32> = noticed
        .iter()
        .map(|l| l.service_record_id)
        .chain(resolvers.iter().map(|(_, service_id)| *service_id))
        .collect();
    let services: std::collections::HashMap<i32, service_record::Model> = if service_ids.is_empty()
    {
        std::collections::HashMap::new()
    } else {
        service_record::Entity::find()
            .filter(service_record::Column::Id.is_in(service_ids))
            .all(db)
            .await?
            .into_iter()
            .map(|s| (s.id, s))
            .collect()
    };

    Ok(incidents
        .into_iter()
        .map(|inc| {
            let service = |id: &i32| services.get(id).cloned().map(ServiceRef::from);
            let noticed_during = noticed
                .iter()
                .filter(|l| l.incident_id == inc.id)
                .find_map(|l| service(&l.service_record_id));
            let mut resolved_by: Vec<ServiceRef> = resolvers
                .iter()
                .filter(|(incident_id, _)| *incident_id == inc.id)
                .filter_map(|(_, service_id)| service(service_id))
                .collect();
            resolved_by.sort_by(|a, b| (&a.service_date, a.id).cmp(&(&b.service_date, b.id)));
            IncidentWithDetails {
                followups: followups
                    .iter()
                    .filter(|f| f.incident_id == inc.id)
                    .cloned()
                    .collect(),
                incident: inc,
                noticed_during,
                resolved_by,
            }
        })
        .collect())
}

/// Links an incident to a service record in the given role, inside the
/// caller's transaction when there is one. Both ids must belong to the
/// vehicle; a wrong-vehicle id reads as nonexistent.
///
/// The `resolved` role also marks the incident resolved, and linking the same
/// service twice is a no-op. An incident can be noticed during only one
/// service, so a second `noticed` link is a `BadRequest`.
pub(crate) async fn link_service(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    incident_id: i32,
    service_record_id: i32,
    role: LinkRole,
) -> DomainResult<()> {
    let inc = require_incident(db, vehicle_id, incident_id).await?;
    super::service_record::require(db, vehicle_id, service_record_id).await?;

    let existing = incident_service_link::Entity::find()
        .filter(incident_service_link::Column::IncidentId.eq(incident_id))
        .filter(incident_service_link::Column::Role.eq(role))
        .all(db)
        .await?;
    if existing
        .iter()
        .any(|l| l.service_record_id == service_record_id)
    {
        return Ok(());
    }
    if role == LinkRole::Noticed && !existing.is_empty() {
        return Err(DomainError::BadRequest(format!(
            "Incident {incident_id} was already noticed during another service"
        )));
    }

    incident_service_link::ActiveModel {
        incident_id: Set(incident_id),
        service_record_id: Set(service_record_id),
        role: Set(role),
    }
    .insert(db)
    .await?;

    if role == LinkRole::Resolved && !inc.resolved {
        let mut active: incident::ActiveModel = inc.into();
        set_resolution(&mut active, Resolution::ByService);
        active.updated_at = Set(super::now());
        active.update(db).await?;
    }
    Ok(())
}

/// Each `(incident_id, service_id)` pair in which the service resolved one of
/// `incident_ids`: the stored `resolved` links and the jobs filed from the
/// incident that a service completes. The pairs are sorted and each appears once.
pub(crate) async fn resolvers_of_incidents(
    db: &impl ConnectionTrait,
    incident_ids: &[i32],
) -> DomainResult<Vec<(i32, i32)>> {
    if incident_ids.is_empty() {
        return Ok(Vec::new());
    }
    let stored = incident_service_link::Entity::find()
        .filter(incident_service_link::Column::IncidentId.is_in(incident_ids.to_vec()));
    let jobs = work_item::Entity::find()
        .filter(work_item::Column::IncidentId.is_in(incident_ids.to_vec()));
    resolvers(db, stored, jobs).await
}

/// Each `(incident_id, service_id)` pair in which one of `service_ids`
/// resolved an incident: the stored `resolved` links and the incidents of the
/// jobs the service completes. The pairs are sorted and each appears once.
pub(crate) async fn resolvers_of_services(
    db: &impl ConnectionTrait,
    service_ids: &[i32],
) -> DomainResult<Vec<(i32, i32)>> {
    if service_ids.is_empty() {
        return Ok(Vec::new());
    }
    let stored = incident_service_link::Entity::find()
        .filter(incident_service_link::Column::ServiceRecordId.is_in(service_ids.to_vec()));
    let jobs = work_item::Entity::find()
        .filter(work_item::Column::CompletedServiceId.is_in(service_ids.to_vec()));
    resolvers(db, stored, jobs).await
}

// A job's incident is read from the job and never stored as a link, so a
// stored `resolved` link always means the invoice named the incident, and
// taking the job off the service leaves that link alone.
async fn resolvers(
    db: &impl ConnectionTrait,
    stored: Select<incident_service_link::Entity>,
    jobs: Select<work_item::Entity>,
) -> DomainResult<Vec<(i32, i32)>> {
    let mut pairs: Vec<(i32, i32)> = stored
        .filter(incident_service_link::Column::Role.eq(LinkRole::Resolved))
        .all(db)
        .await?
        .into_iter()
        .map(|l| (l.incident_id, l.service_record_id))
        .collect();
    let from_jobs: Vec<(i32, i32)> = jobs
        .select_only()
        .column(work_item::Column::IncidentId)
        .column(work_item::Column::CompletedServiceId)
        .filter(work_item::Column::IncidentId.is_not_null())
        .filter(work_item::Column::CompletedServiceId.is_not_null())
        .into_tuple()
        .all(db)
        .await?;
    pairs.extend(from_jobs);
    pairs.sort_unstable();
    pairs.dedup();
    Ok(pairs)
}

/// Marks each of the vehicle's incidents in `incident_ids` resolved and
/// stores no link. A wrong-vehicle id reads as nonexistent.
pub(crate) async fn mark_resolved(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    incident_ids: &[i32],
) -> DomainResult<()> {
    for &id in incident_ids {
        let inc = require_incident(db, vehicle_id, id).await?;
        if !inc.resolved {
            let mut active: incident::ActiveModel = inc.into();
            set_resolution(&mut active, Resolution::ByService);
            active.updated_at = Set(super::now());
            active.update(db).await?;
        }
    }
    Ok(())
}

/// Stores a `resolved` link between the incident and the service unless one
/// is stored already. The incident's `resolved` flag is left as it is.
pub(crate) async fn keep_resolver(
    db: &impl ConnectionTrait,
    incident_id: i32,
    service_id: i32,
) -> DomainResult<()> {
    let key = (incident_id, service_id, LinkRole::Resolved);
    if incident_service_link::Entity::find_by_id(key)
        .one(db)
        .await?
        .is_none()
    {
        incident_service_link::ActiveModel {
            incident_id: Set(incident_id),
            service_record_id: Set(service_id),
            role: Set(LinkRole::Resolved),
        }
        .insert(db)
        .await?;
    }
    Ok(())
}

/// Lists a vehicle's incidents newest first, keeping only resolved ones for
/// `Some(true)`, only unresolved ones for `Some(false)`, and all for `None`.
pub async fn list_filtered(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    resolved: Option<bool>,
) -> DomainResult<Vec<IncidentWithDetails>> {
    super::vehicle::require(db, vehicle_id).await?;
    let incidents = list_rows(db, vehicle_id, resolved).await?;
    with_details(db, incidents).await
}

/// The incident rows behind [`list_filtered`], without their followups and
/// service links, for a caller that reads only the incidents themselves. The
/// vehicle is not checked; an unknown one has no incidents.
pub(crate) async fn list_rows(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    resolved: Option<bool>,
) -> DomainResult<Vec<incident::Model>> {
    let mut query = incident::Entity::find().filter(incident::Column::VehicleId.eq(vehicle_id));
    if let Some(resolved) = resolved {
        query = query.filter(incident::Column::Resolved.eq(resolved));
    }
    Ok(query
        .order_by_desc(incident::Column::OccurredAt)
        .order_by_desc(incident::Column::Id)
        .all(db)
        .await?)
}

pub async fn get(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<IncidentWithDetails> {
    let inc = require_incident(db, vehicle_id, id).await?;
    Ok(with_details(db, vec![inc])
        .await?
        .pop()
        .expect("one incident in, one detail out"))
}

/// Guard the cross-record links a create/update may carry: the same-vehicle
/// recurrence target. Wrong-vehicle references must be
/// indistinguishable from nonexistent ones.
async fn require_links_owned(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    recurrence_of_id: Option<i32>,
) -> DomainResult<()> {
    if let Some(recurrence_of_id) = recurrence_of_id {
        require_incident(db, vehicle_id, recurrence_of_id).await?;
    }
    Ok(())
}

/// How an incident came to read resolved. It is the only value behind the
/// `resolved` and `resolved_by_hand` columns, which `set_resolution` writes
/// together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Resolution {
    Open,
    // The owner said so (`update` with `resolved: true`).
    ByHand,
    // A service resolved it, by an invoice link or a completed job.
    ByService,
}

fn set_resolution(active: &mut incident::ActiveModel, resolution: Resolution) {
    active.resolved = Set(resolution != Resolution::Open);
    active.resolved_by_hand = Set(resolution == Resolution::ByHand);
}

pub async fn create<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    input: NewIncident,
) -> DomainResult<IncidentWithDetails> {
    let txn = super::begin_write(db).await?;
    let db = &txn;
    super::vehicle::require(db, vehicle_id).await?;
    require_links_owned(db, vehicle_id, input.recurrence_of_id).await?;

    let model = incident::ActiveModel {
        vehicle_id: Set(vehicle_id),
        category: Set(input.category),
        title: Set(input.title),
        description: Set(input.description),
        odometer: Set(input.odometer),
        obd_codes: Set(input.obd_codes),
        notes: Set(input.notes),
        fault: Set(input.fault),
        other_party_name: Set(input.other_party_name),
        other_party_phone: Set(input.other_party_phone),
        other_party_email: Set(input.other_party_email),
        other_party_insurance: Set(input.other_party_insurance),
        other_party_policy_number: Set(input.other_party_policy_number),
        insurance_claim_number: Set(input.insurance_claim_number),
        insurance_adjuster: Set(input.insurance_adjuster),
        insurance_adjuster_phone: Set(input.insurance_adjuster_phone),
        recurrence_of_id: Set(input.recurrence_of_id),
        occurred_at: Set(input.occurred_at.unwrap_or_else(crate::dates::local_now)),
        ..Default::default()
    };
    let inc = model.insert(&txn).await?;

    if let Some(sid) = input.noticed_during_service_id {
        link_service(&txn, vehicle_id, inc.id, sid, LinkRole::Noticed).await?;
    }
    for sid in super::dedup_ids(&input.resolved_by_service_ids.unwrap_or_default()) {
        link_service(&txn, vehicle_id, inc.id, sid, LinkRole::Resolved).await?;
    }

    let inc = require_incident(&txn, vehicle_id, inc.id).await?;
    let mut created = with_details(&txn, vec![inc]).await?;
    txn.commit().await?;

    Ok(created.pop().expect("one incident in, one detail out"))
}

#[allow(clippy::too_many_lines)]
pub async fn update<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    input: UpdateIncident,
) -> DomainResult<IncidentWithDetails> {
    let txn = super::begin_write(db).await?;
    let db = &txn;
    let existing = require_incident(db, vehicle_id, id).await?;

    require_links_owned(db, vehicle_id, input.recurrence_of_id.flatten()).await?;
    // An incident cannot be a recurrence of itself — directly or through a
    // chain. Walk the proposed target's ancestors; if `id` appears, linking
    // would close a cycle and the first feature that traverses recurrence
    // chains would loop forever. Chains are short (bounded walk, one query
    // per hop), and the walk runs under the write lock so two updates at once
    // cannot close a cycle between them.
    if let Some(target) = input.recurrence_of_id.flatten() {
        if target == id {
            return Err(DomainError::BadRequest(
                "An incident cannot be a recurrence of itself".into(),
            ));
        }
        let mut cursor = Some(target);
        let mut hops = 0;
        while let Some(current) = cursor {
            if current == id {
                return Err(DomainError::BadRequest(
                    "Recurrence link would create a cycle".into(),
                ));
            }
            hops += 1;
            if hops > 100 {
                // Defensive bound: no legitimate chain is this long, and a
                // pre-existing cycle must not hang the guard itself.
                return Err(DomainError::BadRequest(
                    "Recurrence chain is too deep to link against".into(),
                ));
            }
            cursor = incident::Entity::find_by_id(current)
                .one(db)
                .await?
                .and_then(|i| i.recurrence_of_id);
        }
    }

    let mut active: incident::ActiveModel = existing.into();

    if let Some(v) = input.category {
        active.category = Set(v);
    }
    if let Some(v) = input.title {
        active.title = Set(v);
    }
    if let Some(v) = input.description {
        active.description = Set(v);
    }
    if let Some(v) = input.odometer {
        active.odometer = Set(v);
    }
    if let Some(v) = input.occurred_at {
        active.occurred_at = Set(v);
    }
    if let Some(v) = input.obd_codes {
        active.obd_codes = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }
    if let Some(v) = input.fault {
        active.fault = Set(v);
    }
    if let Some(v) = input.other_party_name {
        active.other_party_name = Set(v);
    }
    if let Some(v) = input.other_party_phone {
        active.other_party_phone = Set(v);
    }
    if let Some(v) = input.other_party_email {
        active.other_party_email = Set(v);
    }
    if let Some(v) = input.other_party_insurance {
        active.other_party_insurance = Set(v);
    }
    if let Some(v) = input.other_party_policy_number {
        active.other_party_policy_number = Set(v);
    }
    if let Some(v) = input.insurance_claim_number {
        active.insurance_claim_number = Set(v);
    }
    if let Some(v) = input.insurance_adjuster {
        active.insurance_adjuster = Set(v);
    }
    if let Some(v) = input.insurance_adjuster_phone {
        active.insurance_adjuster_phone = Set(v);
    }
    if let Some(v) = input.total_repair_cost_cents {
        active.total_repair_cost_cents = Set(v);
    }
    if let Some(v) = input.total_repair_cost_currency {
        active.total_repair_cost_currency = Set(v);
    }
    if let Some(v) = input.deductible_cents {
        active.deductible_cents = Set(v);
    }
    if let Some(v) = input.deductible_currency {
        active.deductible_currency = Set(v);
    }
    if let Some(v) = input.insurance_payout_cents {
        active.insurance_payout_cents = Set(v);
    }
    if let Some(v) = input.insurance_payout_currency {
        active.insurance_payout_currency = Set(v);
    }
    if let Some(v) = input.recurrence_of_id {
        active.recurrence_of_id = Set(v);
    }

    active.updated_at = Set(super::now());

    let inc = active.update(&txn).await?;

    // Replacing the resolving services with none leaves nothing that resolved
    // the incident, so it reads as unresolved unless the caller says otherwise
    // or a completed job still resolves it.
    let mut cleared_all_resolvers = false;
    if let Some(sids) = input.resolved_by_service_ids {
        let from_jobs: Vec<i32> = work_item::Entity::find()
            .select_only()
            .column(work_item::Column::CompletedServiceId)
            .filter(work_item::Column::IncidentId.eq(inc.id))
            .filter(work_item::Column::CompletedServiceId.is_not_null())
            .into_tuple()
            .all(&txn)
            .await?;
        cleared_all_resolvers = sids.is_empty() && from_jobs.is_empty();
        // The list cannot take off a service a job brings, so that service's
        // links stay as they are: a stored one keeps saying the invoice named
        // the incident, and none is stored for a list sent back from a read,
        // which names the service because the job does.
        let replaced: Vec<i32> = incident_service_link::Entity::find()
            .filter(incident_service_link::Column::IncidentId.eq(inc.id))
            .filter(incident_service_link::Column::Role.eq(LinkRole::Resolved))
            .all(&txn)
            .await?
            .into_iter()
            .map(|l| l.service_record_id)
            .filter(|sid| !from_jobs.contains(sid))
            .collect();
        if !replaced.is_empty() {
            incident_service_link::Entity::delete_many()
                .filter(incident_service_link::Column::IncidentId.eq(inc.id))
                .filter(incident_service_link::Column::Role.eq(LinkRole::Resolved))
                .filter(incident_service_link::Column::ServiceRecordId.is_in(replaced))
                .exec(&txn)
                .await?;
        }
        for sid in super::dedup_ids(&sids) {
            if !from_jobs.contains(&sid) {
                link_service(&txn, vehicle_id, inc.id, sid, LinkRole::Resolved).await?;
            }
        }
    }

    if let Some(noticed) = input.noticed_during_service_id {
        incident_service_link::Entity::delete_many()
            .filter(incident_service_link::Column::IncidentId.eq(inc.id))
            .filter(incident_service_link::Column::Role.eq(LinkRole::Noticed))
            .exec(&txn)
            .await?;
        if let Some(sid) = noticed {
            link_service(&txn, vehicle_id, inc.id, sid, LinkRole::Noticed).await?;
        }
    }

    // An explicit `resolved` wins over the flag the links just set. It marks
    // the owner's word only when it is what resolves the incident: not when a
    // link of this call or an earlier one already did. With every resolver
    // cleared, the stored flag no longer rests on a service.
    let inc = require_incident(&txn, vehicle_id, inc.id).await?;
    let resolution = match input.resolved {
        Some(false) => Some(Resolution::Open),
        Some(true) if !inc.resolved || cleared_all_resolvers => Some(Resolution::ByHand),
        None if cleared_all_resolvers => Some(Resolution::Open),
        Some(true) | None => None,
    };
    let inc = if let Some(resolution) = resolution {
        let mut active: incident::ActiveModel = inc.into();
        set_resolution(&mut active, resolution);
        active.update(&txn).await?
    } else {
        inc
    };
    let mut updated = with_details(&txn, vec![inc]).await?;
    txn.commit().await?;

    Ok(updated.pop().expect("one incident in, one detail out"))
}

/// Delete an incident and its owned rows (followups, service links). An
/// incident that recurs from this one is kept and stops naming it. A job
/// planned for the incident is kept and stops naming it, without a change to
/// its `updated_at`.
pub async fn delete<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<()> {
    let existing = require_incident(db, vehicle_id, id).await?;

    let txn = super::begin_write(db).await?;

    incident_followup::Entity::delete_many()
        .filter(incident_followup::Column::IncidentId.eq(existing.id))
        .exec(&txn)
        .await?;
    incident_service_link::Entity::delete_many()
        .filter(incident_service_link::Column::IncidentId.eq(existing.id))
        .exec(&txn)
        .await?;
    // The foreign key of `recurrence_of_id` would clear the column as well.
    // This update also moves the recurrence's `updated_at`.
    let stamp = super::now();
    incident::Entity::update_many()
        .set(incident::ActiveModel {
            recurrence_of_id: Set(None),
            updated_at: Set(stamp),
            ..Default::default()
        })
        .filter(incident::Column::RecurrenceOfId.eq(existing.id))
        .exec(&txn)
        .await?;
    // The foreign key of `work_items.incident_id` clears the column of every
    // job planned for this incident. Every connection enforces foreign keys
    // (`db::connect_options`).
    existing.delete(&txn).await?;

    txn.commit().await?;

    Ok(())
}

pub async fn create_followup<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    incident_id: i32,
    input: NewFollowup,
) -> DomainResult<incident_followup::Model> {
    let txn = super::begin_write(db).await?;
    let db = &txn;
    // Verify the incident belongs to this vehicle before attaching a followup.
    require_incident(db, vehicle_id, incident_id).await?;

    let model = incident_followup::ActiveModel {
        incident_id: Set(incident_id),
        occurred_at: Set(input.occurred_at),
        contact_method: Set(input.contact_method),
        contact_with: Set(input.contact_with),
        summary: Set(input.summary),
        notes: Set(input.notes),
        ..Default::default()
    };
    let out = model.insert(db).await?;
    txn.commit().await?;
    Ok(out)
}

/// The incidents the service resolved that still read resolved, oldest first.
/// An incident the owner marked resolved by hand is left out.
pub(crate) async fn resolved_by_service(
    txn: &impl ConnectionTrait,
    service_id: i32,
) -> DomainResult<Vec<incident::Model>> {
    let ids: Vec<i32> = resolvers_of_services(txn, &[service_id])
        .await?
        .into_iter()
        .map(|(incident_id, _)| incident_id)
        .collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(incident::Entity::find()
        .filter(incident::Column::Id.is_in(ids))
        .filter(incident::Column::Resolved.eq(true))
        .filter(incident::Column::ResolvedByHand.eq(false))
        .order_by_asc(incident::Column::Id)
        .all(txn)
        .await?)
}

/// Those of `incident_ids` that the service holds a stored `resolved` link
/// to, which is to say the ones its invoice named.
pub(crate) async fn named_resolved_by(
    db: &impl ConnectionTrait,
    service_id: i32,
    incident_ids: &[i32],
) -> DomainResult<Vec<i32>> {
    if incident_ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(incident_service_link::Entity::find()
        .filter(incident_service_link::Column::ServiceRecordId.eq(service_id))
        .filter(incident_service_link::Column::Role.eq(LinkRole::Resolved))
        .filter(incident_service_link::Column::IncidentId.is_in(incident_ids.to_vec()))
        .all(db)
        .await?
        .into_iter()
        .map(|l| l.incident_id)
        .collect())
}

/// Marks each incident in `resolved_here` unresolved unless a service other
/// than `service_id` also resolved it or the owner marked it resolved by hand. The caller names the incidents that
/// `service_id` stops resolving.
pub(crate) async fn reopen_resolved(
    txn: &impl ConnectionTrait,
    service_id: i32,
    resolved_here: Vec<i32>,
) -> DomainResult<()> {
    let resolved_elsewhere: std::collections::HashSet<i32> =
        resolvers_of_incidents(txn, &resolved_here)
            .await?
            .into_iter()
            .filter(|(_, by)| *by != service_id)
            .map(|(incident_id, _)| incident_id)
            .collect();
    let by_hand: std::collections::HashSet<i32> = incident::Entity::find()
        .select_only()
        .column(incident::Column::Id)
        .filter(incident::Column::Id.is_in(resolved_here.clone()))
        .filter(incident::Column::ResolvedByHand.eq(true))
        .into_tuple()
        .all(txn)
        .await?
        .into_iter()
        .collect();
    let reopen: Vec<i32> = resolved_here
        .into_iter()
        .filter(|id| !resolved_elsewhere.contains(id) && !by_hand.contains(id))
        .collect();
    if reopen.is_empty() {
        return Ok(());
    }
    let mut reopened = incident::ActiveModel {
        updated_at: Set(super::now()),
        ..Default::default()
    };
    set_resolution(&mut reopened, Resolution::Open);
    incident::Entity::update_many()
        .set(reopened)
        .filter(incident::Column::Id.is_in(reopen))
        .exec(txn)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::{incident::IncidentCategory, work_item, work_item::JobStatus},
        test_support::{VehicleFixture, date, test_db},
        vocab::Vocabulary,
    };

    async fn seed_vehicle(db: &impl ConnectionTrait) -> i32 {
        VehicleFixture::new().insert_id(db).await
    }

    async fn seed_service(db: &impl ConnectionTrait, vehicle_id: i32) -> i32 {
        use crate::entities::service_record;
        service_record::ActiveModel {
            vehicle_id: Set(vehicle_id),
            service_date: Set(date("2024-01-01")),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    fn new_incident(title: &str, category: IncidentCategory) -> NewIncident {
        NewIncident {
            category,
            title: title.into(),
            ..Default::default()
        }
    }

    // --- create/get/list -------------------------------------------------

    #[tokio::test]
    async fn create_then_get_round_trips() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = create(
            &db,
            vid,
            NewIncident {
                description: Some("front left".into()),
                odometer: Some(50_000),
                ..new_incident("Squeaky brakes", IncidentCategory::Noise)
            },
        )
        .await
        .unwrap();
        let fetched = get(&db, vid, created.incident.id).await.unwrap();
        assert_eq!(fetched.incident.title, "Squeaky brakes");
        assert_eq!(fetched.incident.category, IncidentCategory::Noise);
        assert!(!fetched.incident.resolved);
        crate::test_support::assert_empty(&fetched.followups);
        assert!(fetched.resolved_by.is_empty() && fetched.noticed_during.is_none());
    }

    #[tokio::test]
    async fn create_accident_category_carries_insurance_fields() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = create(
            &db,
            vid,
            NewIncident {
                fault: Some("other".into()),
                other_party_name: Some("J. Doe".into()),
                insurance_claim_number: Some("CLM-1".into()),
                ..new_incident("Rear-ended at a stoplight", IncidentCategory::Accident)
            },
        )
        .await
        .unwrap();
        let fetched = get(&db, vid, created.incident.id).await.unwrap();
        assert_eq!(fetched.incident.category, IncidentCategory::Accident);
        assert_eq!(fetched.incident.fault.as_deref(), Some("other"));
        assert_eq!(fetched.incident.other_party_name.as_deref(), Some("J. Doe"));
        assert_eq!(
            fetched.incident.insurance_claim_number.as_deref(),
            Some("CLM-1")
        );
    }

    #[tokio::test]
    async fn list_orders_by_occurred_at_desc() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        for (title, at) in [("older", "2026-01-01"), ("newer", "2026-06-01")] {
            create(
                &db,
                vid,
                NewIncident {
                    occurred_at: Some(date(at)),
                    ..new_incident(title, IncidentCategory::Note)
                },
            )
            .await
            .unwrap();
        }
        let listed = list_filtered(&db, vid, None).await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].incident.title, "newer");
        assert_eq!(listed[1].incident.title, "older");
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
    async fn get_wrong_vehicle_is_not_found() {
        let db = test_db().await;
        let v1 = seed_vehicle(&db).await;
        let v2 = seed_vehicle(&db).await;
        let inc = create(&db, v1, new_incident("t", IncidentCategory::General))
            .await
            .unwrap();
        assert!(matches!(
            get(&db, v2, inc.incident.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn every_category_is_stored_and_read_back() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        for category in IncidentCategory::ALL {
            let made = create(&db, vid, new_incident("t", *category))
                .await
                .unwrap();
            assert_eq!(made.incident.category, *category);
        }
        assert_eq!(
            list_filtered(&db, vid, None).await.unwrap().len(),
            IncidentCategory::ALL.len()
        );
    }

    // --- update ------------------------------------------------------------

    #[tokio::test]
    async fn update_toggles_resolved_and_clears_description() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let inc = create(
            &db,
            vid,
            NewIncident {
                description: Some("d".into()),
                ..new_incident("T", IncidentCategory::Noise)
            },
        )
        .await
        .unwrap();
        let updated = update(
            &db,
            vid,
            inc.incident.id,
            UpdateIncident {
                resolved: Some(true),
                description: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(updated.incident.resolved);
        assert_eq!(updated.incident.description, None);

        let reopened = update(
            &db,
            vid,
            inc.incident.id,
            UpdateIncident {
                resolved: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(!reopened.incident.resolved);
    }

    #[tokio::test]
    async fn update_sets_insurance_costs() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let inc = create(&db, vid, new_incident("crash", IncidentCategory::Accident))
            .await
            .unwrap();
        let updated = update(
            &db,
            vid,
            inc.incident.id,
            UpdateIncident {
                total_repair_cost_cents: Some(Some(123_450)),
                deductible_cents: Some(Some(50_000)),
                insurance_payout_cents: Some(Some(73_450)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.incident.total_repair_cost_cents, Some(123_450));
        assert_eq!(updated.incident.deductible_cents, Some(50_000));
        assert_eq!(updated.incident.insurance_payout_cents, Some(73_450));
    }

    // --- service links (M2M + wrong-parent) --------------------------------

    #[tokio::test]
    async fn create_links_services_and_get_round_trips() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc_id = seed_service(&db, vid).await;

        let created = create(
            &db,
            vid,
            NewIncident {
                resolved_by_service_ids: Some(vec![svc_id]),
                ..new_incident("Rear-ended", IncidentCategory::Accident)
            },
        )
        .await
        .unwrap();
        assert_eq!(
            created.resolved_by.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![svc_id]
        );

        let fetched = get(&db, vid, created.incident.id).await.unwrap();
        assert_eq!(
            fetched.resolved_by.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![svc_id]
        );
    }

    #[tokio::test]
    async fn create_rejects_other_vehicles_service_records() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign_svc = seed_service(&db, other).await;

        // Linking another vehicle's service record must 404 and create nothing.
        assert!(matches!(
            create(
                &db,
                owner,
                NewIncident {
                    resolved_by_service_ids: Some(vec![foreign_svc]),
                    ..new_incident("t", IncidentCategory::General)
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        crate::test_support::assert_empty(&list_filtered(&db, owner, None).await.unwrap());
    }

    #[tokio::test]
    async fn update_rejects_other_vehicles_service_records() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let own_svc = seed_service(&db, owner).await;
        let foreign_svc = seed_service(&db, other).await;
        let inc = create(
            &db,
            owner,
            NewIncident {
                resolved_by_service_ids: Some(vec![own_svc]),
                ..new_incident("t", IncidentCategory::General)
            },
        )
        .await
        .unwrap();

        assert!(matches!(
            update(
                &db,
                owner,
                inc.incident.id,
                UpdateIncident {
                    resolved_by_service_ids: Some(vec![foreign_svc]),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        // Existing links survive the rejected update (transaction rolled back).
        let fetched = get(&db, owner, inc.incident.id).await.unwrap();
        assert_eq!(
            fetched.resolved_by.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![own_svc]
        );
    }

    async fn marker(db: &impl ConnectionTrait, vid: i32, id: i32) -> bool {
        get(db, vid, id).await.unwrap().incident.resolved_by_hand
    }

    fn resolved_flag(resolved: bool) -> UpdateIncident {
        UpdateIncident {
            resolved: Some(resolved),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn resolving_by_hand_sets_the_marker_and_reopening_clears_it() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let inc = create(&db, vid, new_incident("t", IncidentCategory::General))
            .await
            .unwrap()
            .incident
            .id;
        assert!(!marker(&db, vid, inc).await);

        let out = update(&db, vid, inc, resolved_flag(true)).await.unwrap();
        assert!(out.incident.resolved && out.incident.resolved_by_hand);

        let out = update(&db, vid, inc, resolved_flag(false)).await.unwrap();
        assert!(!out.incident.resolved && !out.incident.resolved_by_hand);
    }

    #[tokio::test]
    async fn resolved_true_beside_a_service_or_on_a_resolved_incident_sets_no_marker() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc = seed_service(&db, vid).await;
        let inc = create(&db, vid, new_incident("t", IncidentCategory::General))
            .await
            .unwrap()
            .incident
            .id;
        let with_service = UpdateIncident {
            resolved: Some(true),
            resolved_by_service_ids: Some(vec![svc]),
            ..Default::default()
        };
        let out = update(&db, vid, inc, with_service).await.unwrap();
        assert!(out.incident.resolved && !out.incident.resolved_by_hand);

        let out = update(&db, vid, inc, resolved_flag(true)).await.unwrap();
        assert!(out.incident.resolved && !out.incident.resolved_by_hand);
    }

    #[tokio::test]
    async fn a_service_link_leaves_a_hand_marker_alone() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc = seed_service(&db, vid).await;
        let inc = create(&db, vid, new_incident("t", IncidentCategory::General))
            .await
            .unwrap()
            .incident
            .id;
        update(&db, vid, inc, resolved_flag(true)).await.unwrap();
        link_service(&db, vid, inc, svc, LinkRole::Resolved)
            .await
            .unwrap();
        assert!(marker(&db, vid, inc).await);
    }

    #[tokio::test]
    async fn clearing_every_resolver_clears_the_marker() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc = seed_service(&db, vid).await;
        let inc = create(&db, vid, new_incident("t", IncidentCategory::General))
            .await
            .unwrap()
            .incident
            .id;
        update(&db, vid, inc, resolved_flag(true)).await.unwrap();
        let clear = UpdateIncident {
            resolved_by_service_ids: Some(vec![]),
            ..Default::default()
        };
        let out = update(&db, vid, inc, clear).await.unwrap();
        assert!(!out.incident.resolved && !out.incident.resolved_by_hand);

        // Resolved by a service, then the owner says so with every resolver cleared.
        let named = UpdateIncident {
            resolved_by_service_ids: Some(vec![svc]),
            ..Default::default()
        };
        update(&db, vid, inc, named).await.unwrap();
        let both = UpdateIncident {
            resolved: Some(true),
            resolved_by_service_ids: Some(vec![]),
            ..Default::default()
        };
        let out = update(&db, vid, inc, both).await.unwrap();
        assert!(out.incident.resolved && out.incident.resolved_by_hand);
    }

    #[tokio::test]
    async fn a_created_incident_is_marked_by_hand_never() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc = seed_service(&db, vid).await;
        let open = create(&db, vid, new_incident("a", IncidentCategory::General))
            .await
            .unwrap();
        assert!(!open.incident.resolved && !open.incident.resolved_by_hand);
        let fixed = create(
            &db,
            vid,
            NewIncident {
                resolved_by_service_ids: Some(vec![svc]),
                ..new_incident("b", IncidentCategory::General)
            },
        )
        .await
        .unwrap();
        assert!(fixed.incident.resolved && !fixed.incident.resolved_by_hand);
    }

    #[tokio::test]
    async fn update_replaces_service_links() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc_a = seed_service(&db, vid).await;
        let svc_b = seed_service(&db, vid).await;
        let inc = create(
            &db,
            vid,
            NewIncident {
                resolved_by_service_ids: Some(vec![svc_a]),
                ..new_incident("t", IncidentCategory::General)
            },
        )
        .await
        .unwrap();

        let updated = update(
            &db,
            vid,
            inc.incident.id,
            UpdateIncident {
                resolved: Some(true),
                resolved_by_service_ids: Some(vec![svc_b]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(updated.incident.resolved);
        assert_eq!(
            updated.resolved_by.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![svc_b]
        );
    }

    #[tokio::test]
    async fn clearing_every_resolving_service_marks_the_incident_unresolved() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc = seed_service(&db, vid).await;
        let inc = create(
            &db,
            vid,
            NewIncident {
                resolved_by_service_ids: Some(vec![svc]),
                ..new_incident("t", IncidentCategory::General)
            },
        )
        .await
        .unwrap();
        assert!(inc.incident.resolved);

        let cleared = update(
            &db,
            vid,
            inc.incident.id,
            UpdateIncident {
                resolved_by_service_ids: Some(vec![]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(cleared.resolved_by.is_empty());
        assert!(!cleared.incident.resolved);

        // An explicit flag in the same call wins over the derived one.
        let relinked = update(
            &db,
            vid,
            inc.incident.id,
            UpdateIncident {
                resolved_by_service_ids: Some(vec![svc]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(relinked.incident.resolved);
        let explicit = update(
            &db,
            vid,
            inc.incident.id,
            UpdateIncident {
                resolved: Some(true),
                resolved_by_service_ids: Some(vec![]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(explicit.resolved_by.is_empty());
        assert!(explicit.incident.resolved);
    }

    // --- recurrence ---------------------------------------------------------

    #[tokio::test]
    async fn recurrence_chain_round_trips() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let first = create(&db, vid, new_incident("Squeak", IncidentCategory::Noise))
            .await
            .unwrap();
        let second = create(
            &db,
            vid,
            NewIncident {
                recurrence_of_id: Some(first.incident.id),
                ..new_incident("Squeak is back", IncidentCategory::Noise)
            },
        )
        .await
        .unwrap();
        assert_eq!(second.incident.recurrence_of_id, Some(first.incident.id));

        // Closing the loop (first -> second while second -> first) must be
        // rejected: a 2-cycle would hang any future chain traversal.
        let err = update(
            &db,
            vid,
            first.incident.id,
            UpdateIncident {
                recurrence_of_id: Some(Some(second.incident.id)),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)));
        // The rejected link mutated nothing.
        let unchanged = get(&db, vid, first.incident.id).await.unwrap();
        assert_eq!(unchanged.incident.recurrence_of_id, None);

        // Clearing via double-option works too.
        let cleared = update(
            &db,
            vid,
            second.incident.id,
            UpdateIncident {
                recurrence_of_id: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.incident.recurrence_of_id, None);
    }

    #[tokio::test]
    async fn recurrence_of_other_vehicles_incident_is_not_found_and_nothing_mutated() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign = create(&db, other, new_incident("theirs", IncidentCategory::Noise))
            .await
            .unwrap();

        // Create referencing a foreign incident must 404 and create nothing.
        assert!(matches!(
            create(
                &db,
                owner,
                NewIncident {
                    recurrence_of_id: Some(foreign.incident.id),
                    ..new_incident("mine", IncidentCategory::Noise)
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        crate::test_support::assert_empty(&list_filtered(&db, owner, None).await.unwrap());

        // Update referencing a foreign incident must 404 and mutate nothing.
        let mine = create(&db, owner, new_incident("mine", IncidentCategory::Noise))
            .await
            .unwrap();
        assert!(matches!(
            update(
                &db,
                owner,
                mine.incident.id,
                UpdateIncident {
                    recurrence_of_id: Some(Some(foreign.incident.id)),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert_eq!(
            get(&db, owner, mine.incident.id)
                .await
                .unwrap()
                .incident
                .recurrence_of_id,
            None
        );
    }

    #[tokio::test]
    async fn recurrence_of_nonexistent_incident_is_not_found() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        assert!(matches!(
            create(
                &db,
                vid,
                NewIncident {
                    recurrence_of_id: Some(999),
                    ..new_incident("t", IncidentCategory::Noise)
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn incident_cannot_be_recurrence_of_itself() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let inc = create(&db, vid, new_incident("t", IncidentCategory::Noise))
            .await
            .unwrap();
        assert!(matches!(
            update(
                &db,
                vid,
                inc.incident.id,
                UpdateIncident {
                    recurrence_of_id: Some(Some(inc.incident.id)),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err(),
            DomainError::BadRequest(_)
        ));
        assert_eq!(
            get(&db, vid, inc.incident.id)
                .await
                .unwrap()
                .incident
                .recurrence_of_id,
            None
        );
    }

    // --- followups ------------------------------------------------------------

    #[tokio::test]
    async fn a_followup_is_read_with_its_incident() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let inc = create(&db, vid, new_incident("crash", IncidentCategory::Accident))
            .await
            .unwrap();

        let f = create_followup(
            &db,
            vid,
            inc.incident.id,
            NewFollowup {
                occurred_at: date("2024-05-02"),
                contact_method: Some("phone".into()),
                contact_with: Some("Adjuster".into()),
                summary: "Filed claim".into(),
                notes: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(f.incident_id, inc.incident.id);

        let fetched = get(&db, vid, inc.incident.id).await.unwrap();
        assert_eq!(fetched.followups.len(), 1);
        assert_eq!(fetched.followups[0].summary, "Filed claim");
    }

    #[tokio::test]
    async fn followup_on_missing_incident_is_not_found() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        assert!(matches!(
            create_followup(
                &db,
                vid,
                999,
                NewFollowup {
                    occurred_at: date("2024-05-02"),
                    contact_method: None,
                    contact_with: None,
                    summary: "x".into(),
                    notes: None,
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn create_followup_wrong_vehicle_is_not_found_and_adds_nothing() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let inc = create(
            &db,
            owner,
            new_incident("crash", IncidentCategory::Accident),
        )
        .await
        .unwrap();

        assert!(matches!(
            create_followup(
                &db,
                other,
                inc.incident.id,
                NewFollowup {
                    occurred_at: date("2024-05-02"),
                    contact_method: None,
                    contact_with: None,
                    summary: "intruder".into(),
                    notes: None,
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        crate::test_support::assert_empty(
            &get(&db, owner, inc.incident.id).await.unwrap().followups,
        );
    }

    // --- queries over accident-category incidents with links -------------------

    #[tokio::test]
    async fn migrated_shape_accident_with_followups_and_links_lists_correctly() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc = seed_service(&db, vid).await;
        let inc = create(
            &db,
            vid,
            NewIncident {
                description: Some("Sideswiped while parked".into()),
                fault: Some("other".into()),
                resolved_by_service_ids: Some(vec![svc]),
                ..new_incident("Sideswiped while parked", IncidentCategory::Accident)
            },
        )
        .await
        .unwrap();
        create_followup(
            &db,
            vid,
            inc.incident.id,
            NewFollowup {
                occurred_at: date("2026-03-15"),
                contact_method: Some("phone".into()),
                contact_with: Some("insurance_adjuster".into()),
                summary: "Claim opened".into(),
                notes: None,
            },
        )
        .await
        .unwrap();

        let listed = list_filtered(&db, vid, None).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].followups.len(), 1);
        assert_eq!(listed[0].resolved_by[0].id, svc);
    }

    // --- delete ----------------------------------------------------------

    #[tokio::test]
    async fn delete_removes_followups_links_and_unlinks_recurrences() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc = seed_service(&db, vid).await;

        let doomed = create(
            &db,
            vid,
            NewIncident {
                resolved_by_service_ids: Some(vec![svc]),
                ..new_incident("Original rattle", IncidentCategory::Noise)
            },
        )
        .await
        .unwrap();
        create_followup(
            &db,
            vid,
            doomed.incident.id,
            NewFollowup {
                occurred_at: date("2024-02-01"),
                contact_method: None,
                contact_with: None,
                summary: "called shop".into(),
                notes: None,
            },
        )
        .await
        .unwrap();
        let recurrence = create(
            &db,
            vid,
            NewIncident {
                recurrence_of_id: Some(doomed.incident.id),
                ..new_incident("Rattle is back", IncidentCategory::Noise)
            },
        )
        .await
        .unwrap();
        let work_item = work_item::ActiveModel {
            vehicle_id: Set(vid),
            title: Set("Chase the rattle".into()),
            status: Set(JobStatus::Planned),
            incident_id: Set(Some(doomed.incident.id)),
            created_at: Set(date("2024-01-01T00:00:00Z")),
            updated_at: Set(date("2024-01-01T00:00:00Z")),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        delete(&db, vid, doomed.incident.id).await.unwrap();

        assert!(matches!(
            get(&db, vid, doomed.incident.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        let followups = incident_followup::Entity::find()
            .filter(incident_followup::Column::IncidentId.eq(doomed.incident.id))
            .all(&db)
            .await
            .unwrap();
        crate::test_support::assert_empty(&followups);
        let links = incident_service_link::Entity::find()
            .filter(incident_service_link::Column::IncidentId.eq(doomed.incident.id))
            .all(&db)
            .await
            .unwrap();
        crate::test_support::assert_empty(&links);
        // The recurrence survives, unchained.
        let recur = get(&db, vid, recurrence.incident.id).await.unwrap();
        assert_eq!(recur.incident.recurrence_of_id, None);
        // The job planned for the incident survives and stops naming it.
        let wi = work_item::Entity::find_by_id(work_item.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(wi.incident_id, None);
        assert_eq!(
            wi.updated_at,
            date::<chrono::DateTime<chrono::Utc>>("2024-01-01T00:00:00Z")
        );
    }

    #[tokio::test]
    async fn delete_wrong_vehicle_is_byte_identical_to_nonexistent() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let other_vid = seed_vehicle(&db).await;
        let inc = create(&db, vid, new_incident("Mine", IncidentCategory::Noise))
            .await
            .unwrap();

        let err = delete(&db, other_vid, inc.incident.id).await.unwrap_err();
        let DomainError::NotFound(msg) = err else {
            panic!("expected NotFound");
        };
        // Same message a truly nonexistent id yields: no ownership oracle.
        assert_eq!(msg, format!("Incident {} not found", inc.incident.id));
        assert!(get(&db, vid, inc.incident.id).await.is_ok());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn racing_followups_all_succeed() {
        let (_dir, pools) = crate::test_support::race_pools().await;
        let first = &pools[0];
        let v = seed_vehicle(first).await;
        let incident = create(first, v, new_incident("Rattle", IncidentCategory::Noise))
            .await
            .unwrap()
            .incident
            .id;
        let results = crate::test_support::race(&pools, |i, db| async move {
            let followup = NewFollowup {
                occurred_at: crate::test_support::date("2026-11-01"),
                contact_method: None,
                contact_with: None,
                summary: format!("Call {i}"),
                notes: None,
            };
            create_followup(&db, v, incident, followup).await
        })
        .await;
        assert!(results.iter().all(Result::is_ok), "{results:?}");
    }
}
