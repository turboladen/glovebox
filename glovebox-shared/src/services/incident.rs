//! The unified incident primitive (2hea unit B): observations + accidents
//! merged into one vehicle-scoped record with constrained categories,
//! followups (generalized correspondence), M2M service links, and recurrence
//! chains. Replaces the retired `observation` and `accident` services.

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{incident, incident_followup, incident_service_link, service_record, work_item},
    error::{DomainError, DomainResult},
    inputs::{
        document::DocumentDisposition,
        incident::{NewFollowup, NewIncident, UpdateIncident},
    },
};

/// Category whitelist — the lossless union of every category the two retired
/// tables actually held (no remapping of existing data) plus `accident`
/// (migrated accidents) and `note` (the `save_note` MCP alias).
pub const VALID_CATEGORIES: [&str; 10] = [
    "general",
    "noise",
    "leak",
    "warning_light",
    "cosmetic",
    "performance",
    "obd_code",
    "damage",
    "accident",
    "note",
];

fn validate_category(category: &str) -> DomainResult<()> {
    if VALID_CATEGORIES.contains(&category) {
        return Ok(());
    }
    Err(DomainError::BadRequest(format!(
        "Invalid category '{}'. Must be one of: {}",
        category,
        VALID_CATEGORIES.join(", ")
    )))
}

/// Fetch an incident scoped to its owning vehicle. A wrong-vehicle lookup is
/// indistinguishable from a nonexistent incident.
async fn require_incident(
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

/// Verify every referenced service record belongs to the vehicle. Cross-vehicle
/// references must be indistinguishable from nonexistent records.
async fn require_service_records_owned(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    service_record_ids: &[i32],
) -> DomainResult<()> {
    if service_record_ids.is_empty() {
        return Ok(());
    }
    let owned: std::collections::HashSet<i32> = service_record::Entity::find()
        .filter(service_record::Column::Id.is_in(service_record_ids.to_vec()))
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .all(db)
        .await?
        .into_iter()
        .map(|s| s.id)
        .collect();
    if let Some(missing) = service_record_ids.iter().find(|id| !owned.contains(id)) {
        return Err(DomainError::NotFound(format!(
            "Service record {missing} not found"
        )));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct IncidentWithDetails {
    #[serde(flatten)]
    pub incident: incident::Model,
    pub followups: Vec<incident_followup::Model>,
    pub service_record_ids: Vec<i32>,
}

/// Load followups and service link IDs for a single incident.
async fn load_details(
    db: &impl ConnectionTrait,
    incident_id: i32,
) -> DomainResult<(Vec<incident_followup::Model>, Vec<i32>)> {
    let followups = incident_followup::Entity::find()
        .filter(incident_followup::Column::IncidentId.eq(incident_id))
        .order_by_asc(incident_followup::Column::OccurredAt)
        .all(db)
        .await?;

    let links = incident_service_link::Entity::find()
        .filter(incident_service_link::Column::IncidentId.eq(incident_id))
        .all(db)
        .await?;
    let service_record_ids = links.into_iter().map(|l| l.service_record_id).collect();

    Ok((followups, service_record_ids))
}

pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<Vec<IncidentWithDetails>> {
    let incidents = incident::Entity::find()
        .filter(incident::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(incident::Column::OccurredAt)
        .order_by_desc(incident::Column::Id)
        .all(db)
        .await?;

    // Batch-load all followups and service links (avoids N+1).
    let incident_ids: Vec<i32> = incidents.iter().map(|i| i.id).collect();

    let all_followups = if incident_ids.is_empty() {
        vec![]
    } else {
        incident_followup::Entity::find()
            .filter(incident_followup::Column::IncidentId.is_in(incident_ids.clone()))
            .order_by_asc(incident_followup::Column::OccurredAt)
            .all(db)
            .await?
    };

    let all_links = if incident_ids.is_empty() {
        vec![]
    } else {
        incident_service_link::Entity::find()
            .filter(incident_service_link::Column::IncidentId.is_in(incident_ids))
            .all(db)
            .await?
    };

    let results = incidents
        .into_iter()
        .map(|inc| {
            let followups: Vec<_> = all_followups
                .iter()
                .filter(|f| f.incident_id == inc.id)
                .cloned()
                .collect();
            let service_record_ids = all_links
                .iter()
                .filter(|l| l.incident_id == inc.id)
                .map(|l| l.service_record_id)
                .collect();
            IncidentWithDetails {
                incident: inc,
                followups,
                service_record_ids,
            }
        })
        .collect();

    Ok(results)
}

pub async fn get(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<IncidentWithDetails> {
    let inc = require_incident(db, vehicle_id, id).await?;
    let (followups, service_record_ids) = load_details(db, inc.id).await?;
    Ok(IncidentWithDetails {
        incident: inc,
        followups,
        service_record_ids,
    })
}

/// Guard the cross-record links a create/update may carry: build ownership,
/// same-vehicle recurrence target. Wrong-vehicle references must be
/// indistinguishable from nonexistent ones.
async fn require_links_owned(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    build_id: Option<i32>,
    recurrence_of_id: Option<i32>,
) -> DomainResult<()> {
    if let Some(build_id) = build_id {
        crate::services::build::require_owned(db, vehicle_id, build_id).await?;
    }
    if let Some(recurrence_of_id) = recurrence_of_id {
        require_incident(db, vehicle_id, recurrence_of_id).await?;
    }
    Ok(())
}

pub async fn create<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    input: NewIncident,
) -> DomainResult<IncidentWithDetails> {
    validate_category(&input.category)?;
    require_links_owned(db, vehicle_id, input.build_id, input.recurrence_of_id).await?;
    let service_record_ids = input.service_record_ids.unwrap_or_default();
    require_service_records_owned(db, vehicle_id, &service_record_ids).await?;

    let txn = db.begin().await?;

    let mut model = incident::ActiveModel {
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
        build_id: Set(input.build_id),
        ..Default::default()
    };
    if let Some(occurred_at) = input.occurred_at {
        model.occurred_at = Set(occurred_at);
    }
    let inc = model.insert(&txn).await?;

    for sid in &service_record_ids {
        incident_service_link::ActiveModel {
            incident_id: Set(inc.id),
            service_record_id: Set(*sid),
        }
        .insert(&txn)
        .await?;
    }

    txn.commit().await?;

    Ok(IncidentWithDetails {
        incident: inc,
        followups: vec![],
        service_record_ids,
    })
}

#[allow(clippy::too_many_lines)]
pub async fn update<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    input: UpdateIncident,
) -> DomainResult<IncidentWithDetails> {
    let existing = require_incident(db, vehicle_id, id).await?;

    if let Some(category) = &input.category {
        validate_category(category)?;
    }
    require_links_owned(
        db,
        vehicle_id,
        input.build_id.flatten(),
        input.recurrence_of_id.flatten(),
    )
    .await?;
    // An incident cannot be a recurrence of itself — directly or through a
    // chain. Walk the proposed target's ancestors; if `id` appears, linking
    // would close a cycle and the first feature that traverses recurrence
    // chains would loop forever. Chains are short (bounded walk, one query
    // per hop), and the walk happens before the txn so nothing mutates on
    // rejection.
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

    let txn = db.begin().await?;

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
    if let Some(v) = input.resolved {
        active.resolved = Set(v);
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
    if let Some(v) = input.build_id {
        active.build_id = Set(v);
    }

    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

    let inc = active.update(&txn).await?;

    let service_record_ids = if let Some(sids) = input.service_record_ids {
        require_service_records_owned(&txn, vehicle_id, &sids).await?;
        incident_service_link::Entity::delete_many()
            .filter(incident_service_link::Column::IncidentId.eq(inc.id))
            .exec(&txn)
            .await?;
        for sid in &sids {
            incident_service_link::ActiveModel {
                incident_id: Set(inc.id),
                service_record_id: Set(*sid),
            }
            .insert(&txn)
            .await?;
        }
        sids
    } else {
        incident_service_link::Entity::find()
            .filter(incident_service_link::Column::IncidentId.eq(inc.id))
            .all(&txn)
            .await?
            .into_iter()
            .map(|l| l.service_record_id)
            .collect()
    };

    let followups = incident_followup::Entity::find()
        .filter(incident_followup::Column::IncidentId.eq(inc.id))
        .order_by_asc(incident_followup::Column::OccurredAt)
        .all(&txn)
        .await?;

    txn.commit().await?;

    Ok(IncidentWithDetails {
        incident: inc,
        followups,
        service_record_ids,
    })
}

/// Delete an incident and its owned rows (followups, service links).
/// Incidents that pointed at this one via `recurrence_of_id` are unlinked
/// (the DB FK is ON DELETE SET NULL, but explicit keeps the behavior
/// pragma-independent and inside the same transaction), as are work items
/// referencing it (soft link, no FK). Linked documents are handled per
/// `docs`; the returned paths are files the CALLER must remove after commit
/// (see [`super::document::detach_or_delete_for_entity`]).
pub async fn delete<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    docs: DocumentDisposition,
) -> DomainResult<Vec<String>> {
    let existing = require_incident(db, vehicle_id, id).await?;

    let txn = db.begin().await?;

    incident_followup::Entity::delete_many()
        .filter(incident_followup::Column::IncidentId.eq(existing.id))
        .exec(&txn)
        .await?;
    incident_service_link::Entity::delete_many()
        .filter(incident_service_link::Column::IncidentId.eq(existing.id))
        .exec(&txn)
        .await?;
    let stamp = super::now_stamp();
    incident::Entity::update_many()
        .set(incident::ActiveModel {
            recurrence_of_id: Set(None),
            updated_at: Set(stamp.clone()),
            ..Default::default()
        })
        .filter(incident::Column::RecurrenceOfId.eq(existing.id))
        .exec(&txn)
        .await?;
    // Work items reference incidents via a soft link (no FK in migration
    // 000020) — clear it or planning rows point at a deleted incident.
    work_item::Entity::update_many()
        .set(work_item::ActiveModel {
            incident_id: Set(None),
            updated_at: Set(stamp),
            ..Default::default()
        })
        .filter(work_item::Column::IncidentId.eq(existing.id))
        .exec(&txn)
        .await?;

    let doc_files =
        super::document::detach_or_delete_for_entity(&txn, "incident", id, docs).await?;

    existing.delete(&txn).await?;

    txn.commit().await?;

    Ok(doc_files)
}

pub async fn list_followups(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    incident_id: i32,
) -> DomainResult<Vec<incident_followup::Model>> {
    // Verify the incident belongs to this vehicle before exposing followups.
    require_incident(db, vehicle_id, incident_id).await?;

    Ok(incident_followup::Entity::find()
        .filter(incident_followup::Column::IncidentId.eq(incident_id))
        .order_by_asc(incident_followup::Column::OccurredAt)
        .all(db)
        .await?)
}

pub async fn create_followup(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    incident_id: i32,
    input: NewFollowup,
) -> DomainResult<incident_followup::Model> {
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
    Ok(model.insert(db).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{VehicleFixture, test_db};

    async fn seed_vehicle(db: &impl ConnectionTrait) -> i32 {
        VehicleFixture::new().insert_id(db).await
    }

    async fn seed_service(db: &impl ConnectionTrait, vehicle_id: i32) -> i32 {
        use crate::entities::service_record;
        service_record::ActiveModel {
            vehicle_id: Set(vehicle_id),
            service_date: Set("2024-01-01".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    fn new_incident(title: &str, category: &str) -> NewIncident {
        NewIncident {
            category: category.into(),
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
                ..new_incident("Squeaky brakes", "noise")
            },
        )
        .await
        .unwrap();
        let fetched = get(&db, vid, created.incident.id).await.unwrap();
        assert_eq!(fetched.incident.title, "Squeaky brakes");
        assert_eq!(fetched.incident.category, "noise");
        assert!(!fetched.incident.resolved);
        assert!(fetched.followups.is_empty());
        assert!(fetched.service_record_ids.is_empty());
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
                ..new_incident("Rear-ended at a stoplight", "accident")
            },
        )
        .await
        .unwrap();
        let fetched = get(&db, vid, created.incident.id).await.unwrap();
        assert_eq!(fetched.incident.category, "accident");
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
                    occurred_at: Some(at.into()),
                    ..new_incident(title, "note")
                },
            )
            .await
            .unwrap();
        }
        let listed = list(&db, vid).await.unwrap();
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
        let inc = create(&db, v1, new_incident("t", "general")).await.unwrap();
        assert!(matches!(
            get(&db, v2, inc.incident.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    // --- category whitelist ----------------------------------------------

    #[tokio::test]
    async fn create_rejects_unknown_category_with_steering_message() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let err = create(&db, vid, new_incident("t", "bogus"))
            .await
            .unwrap_err();
        match err {
            DomainError::BadRequest(msg) => {
                assert!(
                    msg.contains("Invalid category 'bogus'") && msg.contains("warning_light"),
                    "message must name the bad value and list valid ones: {msg}"
                );
            }
            other => panic!("expected BadRequest, got {other:?}"),
        }
        assert!(list(&db, vid).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn update_rejects_unknown_category_and_mutates_nothing() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let inc = create(&db, vid, new_incident("t", "noise")).await.unwrap();
        assert!(matches!(
            update(
                &db,
                vid,
                inc.incident.id,
                UpdateIncident {
                    category: Some("bogus".into()),
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
                .category,
            "noise"
        );
    }

    #[tokio::test]
    async fn every_whitelisted_category_is_accepted() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        for category in VALID_CATEGORIES {
            create(&db, vid, new_incident("t", category)).await.unwrap();
        }
        assert_eq!(list(&db, vid).await.unwrap().len(), VALID_CATEGORIES.len());
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
                ..new_incident("T", "noise")
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
        let inc = create(&db, vid, new_incident("crash", "accident"))
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
                service_record_ids: Some(vec![svc_id]),
                ..new_incident("Rear-ended", "accident")
            },
        )
        .await
        .unwrap();
        assert_eq!(created.service_record_ids, vec![svc_id]);

        let fetched = get(&db, vid, created.incident.id).await.unwrap();
        assert_eq!(fetched.service_record_ids, vec![svc_id]);
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
                    service_record_ids: Some(vec![foreign_svc]),
                    ..new_incident("t", "general")
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(list(&db, owner).await.unwrap().is_empty());
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
                service_record_ids: Some(vec![own_svc]),
                ..new_incident("t", "general")
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
                    service_record_ids: Some(vec![foreign_svc]),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        // Existing links survive the rejected update (transaction rolled back).
        let fetched = get(&db, owner, inc.incident.id).await.unwrap();
        assert_eq!(fetched.service_record_ids, vec![own_svc]);
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
                service_record_ids: Some(vec![svc_a]),
                ..new_incident("t", "general")
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
                service_record_ids: Some(vec![svc_b]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(updated.incident.resolved);
        assert_eq!(updated.service_record_ids, vec![svc_b]);
    }

    // --- recurrence ---------------------------------------------------------

    #[tokio::test]
    async fn recurrence_chain_round_trips() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let first = create(&db, vid, new_incident("Squeak", "noise"))
            .await
            .unwrap();
        let second = create(
            &db,
            vid,
            NewIncident {
                recurrence_of_id: Some(first.incident.id),
                ..new_incident("Squeak is back", "noise")
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
        let foreign = create(&db, other, new_incident("theirs", "noise"))
            .await
            .unwrap();

        // Create referencing a foreign incident must 404 and create nothing.
        assert!(matches!(
            create(
                &db,
                owner,
                NewIncident {
                    recurrence_of_id: Some(foreign.incident.id),
                    ..new_incident("mine", "noise")
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(list(&db, owner).await.unwrap().is_empty());

        // Update referencing a foreign incident must 404 and mutate nothing.
        let mine = create(&db, owner, new_incident("mine", "noise"))
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
                    ..new_incident("t", "noise")
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
        let inc = create(&db, vid, new_incident("t", "noise")).await.unwrap();
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

    // --- build links ---------------------------------------------------------

    #[tokio::test]
    async fn linking_other_vehicles_build_is_not_found() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign_build = crate::services::build::create(
            &db,
            other,
            crate::inputs::build::NewBuild {
                name: "Foreign".into(),
                description: None,
                target_date: None,
            },
        )
        .await
        .unwrap();

        assert!(matches!(
            create(
                &db,
                owner,
                NewIncident {
                    build_id: Some(foreign_build.id),
                    ..new_incident("t", "general")
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(list(&db, owner).await.unwrap().is_empty());
    }

    // --- followups ------------------------------------------------------------

    #[tokio::test]
    async fn followup_create_and_list_relate_to_incident() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let inc = create(&db, vid, new_incident("crash", "accident"))
            .await
            .unwrap();

        let f = create_followup(
            &db,
            vid,
            inc.incident.id,
            NewFollowup {
                occurred_at: "2024-05-02".into(),
                contact_method: Some("phone".into()),
                contact_with: Some("Adjuster".into()),
                summary: "Filed claim".into(),
                notes: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(f.incident_id, inc.incident.id);

        let listed = list_followups(&db, vid, inc.incident.id).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].summary, "Filed claim");

        // The incident get() now surfaces the followup too.
        let fetched = get(&db, vid, inc.incident.id).await.unwrap();
        assert_eq!(fetched.followups.len(), 1);
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
                    occurred_at: "2024-05-02".into(),
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
    async fn list_followups_wrong_vehicle_is_not_found() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let inc = create(&db, owner, new_incident("crash", "accident"))
            .await
            .unwrap();
        create_followup(
            &db,
            owner,
            inc.incident.id,
            NewFollowup {
                occurred_at: "2024-05-02".into(),
                contact_method: None,
                contact_with: None,
                summary: "private".into(),
                notes: None,
            },
        )
        .await
        .unwrap();

        // Reading through the wrong vehicle must 404, not leak the other
        // vehicle's followups.
        assert!(matches!(
            list_followups(&db, other, inc.incident.id)
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
        let inc = create(&db, owner, new_incident("crash", "accident"))
            .await
            .unwrap();

        assert!(matches!(
            create_followup(
                &db,
                other,
                inc.incident.id,
                NewFollowup {
                    occurred_at: "2024-05-02".into(),
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
        assert!(
            list_followups(&db, owner, inc.incident.id)
                .await
                .unwrap()
                .is_empty()
        );
    }

    // --- migrated-shape queries -----------------------------------------------
    // (The migration itself is proven by test_db() running 000018 plus the
    // populated-DB verification; this covers querying rows shaped like
    // migrated data — e.g. an accident-category incident with links.)

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
                service_record_ids: Some(vec![svc]),
                ..new_incident("Sideswiped while parked", "accident")
            },
        )
        .await
        .unwrap();
        create_followup(
            &db,
            vid,
            inc.incident.id,
            NewFollowup {
                occurred_at: "2026-03-15".into(),
                contact_method: Some("phone".into()),
                contact_with: Some("insurance_adjuster".into()),
                summary: "Claim opened".into(),
                notes: None,
            },
        )
        .await
        .unwrap();

        let listed = list(&db, vid).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].followups.len(), 1);
        assert_eq!(listed[0].service_record_ids, vec![svc]);
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
                service_record_ids: Some(vec![svc]),
                ..new_incident("Original rattle", "noise")
            },
        )
        .await
        .unwrap();
        create_followup(
            &db,
            vid,
            doomed.incident.id,
            NewFollowup {
                occurred_at: "2024-02-01".into(),
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
                ..new_incident("Rattle is back", "noise")
            },
        )
        .await
        .unwrap();
        let work_item = work_item::ActiveModel {
            vehicle_id: Set(vid),
            title: Set("Chase the rattle".into()),
            status: Set("todo".into()),
            incident_id: Set(Some(doomed.incident.id)),
            created_at: Set("2024-01-01 00:00:00".into()),
            updated_at: Set("2024-01-01 00:00:00".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let paths = delete(&db, vid, doomed.incident.id, DocumentDisposition::Keep)
            .await
            .unwrap();
        assert!(paths.is_empty());

        assert!(matches!(
            get(&db, vid, doomed.incident.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        let followups = incident_followup::Entity::find()
            .filter(incident_followup::Column::IncidentId.eq(doomed.incident.id))
            .all(&db)
            .await
            .unwrap();
        assert!(followups.is_empty());
        let links = incident_service_link::Entity::find()
            .filter(incident_service_link::Column::IncidentId.eq(doomed.incident.id))
            .all(&db)
            .await
            .unwrap();
        assert!(links.is_empty());
        // The recurrence survives, unchained.
        let recur = get(&db, vid, recurrence.incident.id).await.unwrap();
        assert_eq!(recur.incident.recurrence_of_id, None);
        // Work items are soft-linked (no FK) — the link must be cleared too.
        let wi = work_item::Entity::find_by_id(work_item.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(wi.incident_id, None);
        assert_ne!(wi.updated_at, "2024-01-01 00:00:00");
    }

    #[tokio::test]
    async fn delete_handles_linked_documents_per_mode() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;

        let kept = create(&db, vid, new_incident("Keep my doc", "noise"))
            .await
            .unwrap();
        let cascaded = create(&db, vid, new_incident("Take my doc", "noise"))
            .await
            .unwrap();
        let kept_doc =
            crate::test_support::seed_linked_document(&db, "incident", kept.incident.id).await;
        let cascaded_doc =
            crate::test_support::seed_linked_document(&db, "incident", cascaded.incident.id).await;

        let paths = delete(&db, vid, kept.incident.id, DocumentDisposition::Keep)
            .await
            .unwrap();
        assert!(paths.is_empty());
        let doc = crate::services::document::get(&db, kept_doc).await.unwrap();
        assert_eq!(doc.linked_entity_type, None);
        assert!(doc.notes.unwrap().contains("Unlinked from incident"));

        let paths = delete(&db, vid, cascaded.incident.id, DocumentDisposition::Delete)
            .await
            .unwrap();
        assert_eq!(
            paths,
            vec![format!(
                "general/other/incident-{}.pdf",
                cascaded.incident.id
            )]
        );
        assert!(matches!(
            crate::services::document::get(&db, cascaded_doc)
                .await
                .unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn delete_wrong_vehicle_is_byte_identical_to_nonexistent() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let other_vid = seed_vehicle(&db).await;
        let inc = create(&db, vid, new_incident("Mine", "noise"))
            .await
            .unwrap();

        let err = delete(&db, other_vid, inc.incident.id, DocumentDisposition::Keep)
            .await
            .unwrap_err();
        let DomainError::NotFound(msg) = err else {
            panic!("expected NotFound");
        };
        // Same message a truly nonexistent id yields: no ownership oracle.
        assert_eq!(msg, format!("Incident {} not found", inc.incident.id));
        assert!(get(&db, vid, inc.incident.id).await.is_ok());
    }
}
