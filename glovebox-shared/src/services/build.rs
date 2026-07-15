use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{build, incident, part, service_record},
    error::{DomainError, DomainResult},
    inputs::build::{NewBuild, UpdateBuild},
};

/// Lifecycle whitelist for `builds.status`.
const VALID_STATUSES: [&str; 5] = ["planned", "active", "on_hold", "completed", "abandoned"];

fn validate_status(status: &str) -> DomainResult<()> {
    if VALID_STATUSES.contains(&status) {
        return Ok(());
    }
    Err(DomainError::BadRequest(format!(
        "Invalid status '{}'. Must be one of: {}",
        status,
        VALID_STATUSES.join(", ")
    )))
}

/// Verify a referenced build belongs to the vehicle. A cross-vehicle build
/// must be indistinguishable from a nonexistent one.
///
/// Shared guard for sub-resource services (`service_record`, `part`,
/// `incident`) that accept a `build_id` link.
pub async fn require_owned(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    build_id: i32,
) -> DomainResult<()> {
    build::Entity::find_by_id(build_id)
        .filter(build::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Build {build_id} not found")))?;
    Ok(())
}

/// Derived progress view: the build plus rollups computed at query time from
/// the typed records linked via their `build_id` FKs. Nothing here is stored.
#[derive(Debug, Serialize)]
pub struct BuildProgress {
    #[serde(flatten)]
    pub build: build::Model,
    pub services_count: usize,
    pub parts_total: usize,
    pub parts_installed: usize,
    pub incidents_count: usize,
    /// Sum of linked service records' `total_cost_cents` plus linked parts'
    /// `cost_cents`. Integer cents only (no float math).
    pub total_cost_cents: i64,
    /// `total_cost_cents` restricted to what was actually paid: covered
    /// services (`paid_by != "self"`) and their invoices' parts components
    /// are excluded; parts themselves have no payer and stay out-of-pocket.
    pub out_of_pocket_cents: i64,
    pub linked: LinkedRecords,
}

#[derive(Debug, Serialize)]
pub struct LinkedRecords {
    pub service_record_ids: Vec<i32>,
    pub part_ids: Vec<i32>,
    pub incident_ids: Vec<i32>,
}

pub async fn list(db: &impl ConnectionTrait, vehicle_id: i32) -> DomainResult<Vec<build::Model>> {
    crate::services::vehicle::require(db, vehicle_id).await?;
    Ok(build::Entity::find()
        .filter(build::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(build::Column::CreatedAt)
        .order_by_desc(build::Column::Id)
        .all(db)
        .await?)
}

/// All builds across all vehicles, one query. Resource-enumeration helper
/// for surfaces that list addressable per-build URIs (the MCP server's
/// `resources/list`); not a UI listing.
pub async fn list_all(db: &impl ConnectionTrait) -> DomainResult<Vec<build::Model>> {
    Ok(build::Entity::find()
        .order_by_asc(build::Column::VehicleId)
        .order_by_desc(build::Column::CreatedAt)
        .order_by_desc(build::Column::Id)
        .all(db)
        .await?)
}

pub async fn get(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<build::Model> {
    build::Entity::find_by_id(id)
        .filter(build::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Build {id} not found")))
}

pub async fn create(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    input: NewBuild,
) -> DomainResult<build::Model> {
    crate::services::vehicle::require(db, vehicle_id).await?;
    let model = build::ActiveModel {
        vehicle_id: Set(vehicle_id),
        name: Set(input.name),
        description: Set(input.description),
        target_date: Set(input.target_date),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

pub async fn update(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
    input: UpdateBuild,
) -> DomainResult<build::Model> {
    let existing = get(db, vehicle_id, id).await?;
    let prior_status = existing.status.clone();
    let mut active: build::ActiveModel = existing.into();

    if let Some(v) = input.name {
        active.name = Set(v);
    }
    if let Some(v) = input.description {
        active.description = Set(v);
    }
    if let Some(v) = input.target_date {
        active.target_date = Set(v);
    }
    if let Some(status) = input.status {
        validate_status(&status)?;
        // Lifecycle stamp only on actual transitions: entering "completed"
        // records when; leaving it clears. A completed→completed update (e.g. a
        // full-object PUT that renames the build) must not move the historical
        // completion date.
        if status == "completed" && prior_status != "completed" {
            active.completed_at = Set(Some(
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            ));
        } else if status != "completed" && prior_status == "completed" {
            active.completed_at = Set(None);
        }
        active.status = Set(status);
    }

    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
    Ok(active.update(db).await?)
}

/// Delete a build, nulling out `build_id` on any linked records first so a
/// deleted build cannot leave dangling links. Transactional.
pub async fn delete<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<()> {
    let existing = get(db, vehicle_id, id).await?;

    let txn = db.begin().await?;

    service_record::Entity::update_many()
        .set(service_record::ActiveModel {
            build_id: Set(None),
            ..Default::default()
        })
        .filter(service_record::Column::BuildId.eq(existing.id))
        .exec(&txn)
        .await?;
    part::Entity::update_many()
        .set(part::ActiveModel {
            build_id: Set(None),
            ..Default::default()
        })
        .filter(part::Column::BuildId.eq(existing.id))
        .exec(&txn)
        .await?;
    incident::Entity::update_many()
        .set(incident::ActiveModel {
            build_id: Set(None),
            ..Default::default()
        })
        .filter(incident::Column::BuildId.eq(existing.id))
        .exec(&txn)
        .await?;

    existing.delete(&txn).await?;

    txn.commit().await?;
    Ok(())
}

/// Compute the derived progress view for a build: one batch query per linked
/// record type (no per-record queries).
pub async fn progress(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<BuildProgress> {
    let build = get(db, vehicle_id, id).await?;

    let services = service_record::Entity::find()
        .filter(service_record::Column::BuildId.eq(build.id))
        .order_by_asc(service_record::Column::Id)
        .all(db)
        .await?;
    let parts = part::Entity::find()
        .filter(part::Column::BuildId.eq(build.id))
        .order_by_asc(part::Column::Id)
        .all(db)
        .await?;
    let incidents = incident::Entity::find()
        .filter(incident::Column::BuildId.eq(build.id))
        .order_by_asc(incident::Column::Id)
        .all(db)
        .await?;

    let services_cost: i64 = services
        .iter()
        .filter_map(|s| s.total_cost_cents)
        .map(i64::from)
        .sum();
    let services_parts_cost: i64 = services
        .iter()
        .filter_map(|s| s.parts_cost_cents)
        .map(i64::from)
        .sum();
    let parts_cost: i64 = parts
        .iter()
        .filter_map(|p| p.cost_cents)
        .map(i64::from)
        .sum();
    // Mirror services::costs' dedupe: a service invoice's total typically
    // already includes its parts, so only linked-part spend beyond the linked
    // invoices' parts component counts as extra (never negative).
    let extra_parts_cost = (parts_cost - services_parts_cost).max(0);

    // Out-of-pocket: same formula with covered services (and their invoices'
    // parts components) excluded from the service side.
    let self_services_cost: i64 = services
        .iter()
        .filter(|s| s.paid_by == "self")
        .filter_map(|s| s.total_cost_cents)
        .map(i64::from)
        .sum();
    let self_services_parts_cost: i64 = services
        .iter()
        .filter(|s| s.paid_by == "self")
        .filter_map(|s| s.parts_cost_cents)
        .map(i64::from)
        .sum();
    let out_of_pocket_extra_parts = (parts_cost - self_services_parts_cost).max(0);

    Ok(BuildProgress {
        services_count: services.len(),
        parts_total: parts.len(),
        parts_installed: parts.iter().filter(|p| p.status == "installed").count(),
        incidents_count: incidents.len(),
        total_cost_cents: services_cost + extra_parts_cost,
        out_of_pocket_cents: self_services_cost + out_of_pocket_extra_parts,
        linked: LinkedRecords {
            service_record_ids: services.iter().map(|s| s.id).collect(),
            part_ids: parts.iter().map(|p| p.id).collect(),
            incident_ids: incidents.iter().map(|i| i.id).collect(),
        },
        build,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inputs::{
            incident::UpdateIncident,
            part::{NewPart, UpdatePart},
            service_record::{NewServiceRecord, UpdateServiceRecord},
        },
        services::{incident as incident_svc, part as part_svc, service_record as svc_svc},
        test_support::{VehicleFixture, test_db},
    };

    async fn seed_vehicle(db: &impl ConnectionTrait) -> i32 {
        VehicleFixture::new().insert_id(db).await
    }

    fn new_build(name: &str) -> NewBuild {
        NewBuild {
            name: name.into(),
            description: None,
            target_date: None,
        }
    }

    fn minimal_part(name: &str, cost_cents: Option<i32>, status: Option<String>) -> NewPart {
        NewPart {
            name: name.into(),
            manufacturer: None,
            part_number: None,
            oe_part_number_replaced: None,
            seller: None,
            purchase_date: None,
            cost_cents,
            cost_currency: None,
            invoice_url: None,
            manufacturer_url: None,
            retailer_url: None,
            status,
            installed_date: None,
            installed_odometer: None,
            installed_service_id: None,
            notes: None,
            build_id: None,
            location: None,
            warranty_expires_on: None,
            warranty_expires_miles: None,
        }
    }

    fn minimal_service(total_cost_cents: Option<i32>, build_id: Option<i32>) -> NewServiceRecord {
        NewServiceRecord {
            service_date: "2026-01-01".into(),
            mileage: None,
            description: None,
            parts_cost_cents: None,
            parts_cost_currency: None,
            labor_cost_cents: None,
            labor_cost_currency: None,
            total_cost_cents,
            total_cost_currency: None,
            shop_name: None,
            shop_id: None,
            notes: None,
            build_id,
            paid_by: None,
            payer_note: None,
            schedule_item_ids: None,
            part_ids: None,
            line_items: None,
            invoice_ref: None,
        }
    }

    fn minimal_incident(build_id: Option<i32>) -> crate::inputs::incident::NewIncident {
        crate::inputs::incident::NewIncident {
            category: "note".into(),
            title: "Inc".into(),
            build_id,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn create_then_get_and_list_round_trip() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = create(
            &db,
            vid,
            NewBuild {
                name: "Turbo upgrade".into(),
                description: Some("IS38 swap".into()),
                target_date: Some("2026-12-01".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(created.status, "planned");
        assert_eq!(created.completed_at, None);

        let fetched = get(&db, vid, created.id).await.unwrap();
        assert_eq!(fetched.name, "Turbo upgrade");
        assert_eq!(fetched.description.as_deref(), Some("IS38 swap"));
        assert_eq!(fetched.target_date.as_deref(), Some("2026-12-01"));

        let listed = list(&db, vid).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, created.id);
    }

    #[tokio::test]
    async fn create_and_list_require_vehicle() {
        let db = test_db().await;
        assert!(matches!(
            create(&db, 999, new_build("B")).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(matches!(
            list(&db, 999).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn update_rejects_invalid_status() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let b = create(&db, vid, new_build("B")).await.unwrap();
        let err = update(
            &db,
            vid,
            b.id,
            UpdateBuild {
                status: Some("bogus".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)));
        // The rejected update left the build untouched.
        assert_eq!(get(&db, vid, b.id).await.unwrap().status, "planned");
    }

    #[tokio::test]
    async fn completed_status_stamps_and_clears_completed_at() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let b = create(&db, vid, new_build("B")).await.unwrap();

        let completed = update(
            &db,
            vid,
            b.id,
            UpdateBuild {
                status: Some("completed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(completed.status, "completed");
        assert!(completed.completed_at.is_some());

        // Moving back out of completed clears the stamp.
        let reopened = update(
            &db,
            vid,
            b.id,
            UpdateBuild {
                status: Some("active".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(reopened.status, "active");
        assert_eq!(reopened.completed_at, None);

        // A status-less update leaves completed_at alone.
        update(
            &db,
            vid,
            b.id,
            UpdateBuild {
                status: Some("completed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let renamed = update(
            &db,
            vid,
            b.id,
            UpdateBuild {
                name: Some("Renamed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(renamed.name, "Renamed");
        assert!(renamed.completed_at.is_some());

        // A completed→completed update (full-object PUT resending the current
        // status) must NOT move the historical completion date. Backdate the
        // stamp so a same-second re-stamp can't hide the regression.
        let mut backdate: build::ActiveModel = renamed.into();
        backdate.completed_at = Set(Some("2026-01-01 00:00:00".into()));
        backdate.update(&db).await.unwrap();
        let resent = update(
            &db,
            vid,
            b.id,
            UpdateBuild {
                name: Some("Renamed again".into()),
                status: Some("completed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(resent.completed_at.as_deref(), Some("2026-01-01 00:00:00"));
    }

    #[tokio::test]
    async fn wrong_vehicle_is_indistinguishable_from_missing() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let b = create(&db, owner, new_build("B")).await.unwrap();

        assert!(matches!(
            get(&db, other, b.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(matches!(
            update(
                &db,
                other,
                b.id,
                UpdateBuild {
                    name: Some("X".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(matches!(
            delete(&db, other, b.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(matches!(
            progress(&db, other, b.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        // The build survived every wrong-vehicle attempt unchanged.
        assert_eq!(get(&db, owner, b.id).await.unwrap().name, "B");
    }

    #[tokio::test]
    async fn linking_same_vehicle_build_works_and_some_none_clears() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let b = create(&db, vid, new_build("B")).await.unwrap();

        // service record
        let svc = svc_svc::create(&db, vid, minimal_service(None, None))
            .await
            .unwrap();
        let linked = svc_svc::update(
            &db,
            vid,
            svc.record.id,
            UpdateServiceRecord {
                build_id: Some(Some(b.id)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(linked.record.build_id, Some(b.id));
        let cleared = svc_svc::update(
            &db,
            vid,
            svc.record.id,
            UpdateServiceRecord {
                build_id: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.record.build_id, None);

        // part
        let p = part_svc::create(&db, vid, minimal_part("P", None, None))
            .await
            .unwrap();
        let linked = part_svc::update(
            &db,
            vid,
            p.id,
            UpdatePart {
                build_id: Some(Some(b.id)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(linked.build_id, Some(b.id));
        let cleared = part_svc::update(
            &db,
            vid,
            p.id,
            UpdatePart {
                build_id: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.build_id, None);

        // incident
        let o = incident_svc::create(&db, vid, minimal_incident(None))
            .await
            .unwrap();
        let linked = incident_svc::update(
            &db,
            vid,
            o.incident.id,
            UpdateIncident {
                build_id: Some(Some(b.id)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(linked.incident.build_id, Some(b.id));
        let cleared = incident_svc::update(
            &db,
            vid,
            o.incident.id,
            UpdateIncident {
                build_id: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.incident.build_id, None);
    }

    #[tokio::test]
    async fn linking_other_vehicles_build_is_not_found_and_nothing_mutated() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign_build = create(&db, other, new_build("Foreign")).await.unwrap();

        // Creates referencing a foreign build must 404 and create nothing.
        assert!(matches!(
            svc_svc::create(&db, owner, minimal_service(None, Some(foreign_build.id)))
                .await
                .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(matches!(
            part_svc::create(
                &db,
                owner,
                NewPart {
                    build_id: Some(foreign_build.id),
                    ..minimal_part("P", None, None)
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(matches!(
            incident_svc::create(&db, owner, minimal_incident(Some(foreign_build.id)))
                .await
                .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(svc_svc::list(&db, owner).await.unwrap().is_empty());
        assert!(
            part_svc::list(&db, owner, crate::inputs::part::PartFilter::default())
                .await
                .unwrap()
                .is_empty()
        );
        assert!(incident_svc::list(&db, owner).await.unwrap().is_empty());

        // Updates referencing a foreign build must 404 and mutate nothing.
        let svc = svc_svc::create(&db, owner, minimal_service(None, None))
            .await
            .unwrap();
        let p = part_svc::create(&db, owner, minimal_part("P", None, None))
            .await
            .unwrap();
        let o = incident_svc::create(&db, owner, minimal_incident(None))
            .await
            .unwrap();

        assert!(matches!(
            svc_svc::update(
                &db,
                owner,
                svc.record.id,
                UpdateServiceRecord {
                    build_id: Some(Some(foreign_build.id)),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(matches!(
            part_svc::update(
                &db,
                owner,
                p.id,
                UpdatePart {
                    build_id: Some(Some(foreign_build.id)),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(matches!(
            incident_svc::update(
                &db,
                owner,
                o.incident.id,
                UpdateIncident {
                    build_id: Some(Some(foreign_build.id)),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert_eq!(
            svc_svc::get(&db, owner, svc.record.id)
                .await
                .unwrap()
                .record
                .build_id,
            None
        );
        assert_eq!(
            part_svc::get(&db, owner, p.id).await.unwrap().build_id,
            None
        );
        assert_eq!(
            incident_svc::get(&db, owner, o.incident.id)
                .await
                .unwrap()
                .incident
                .build_id,
            None
        );
    }

    #[tokio::test]
    async fn delete_nulls_out_linked_records() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let b = create(&db, vid, new_build("B")).await.unwrap();

        let svc = svc_svc::create(&db, vid, minimal_service(None, Some(b.id)))
            .await
            .unwrap();
        let p = part_svc::create(
            &db,
            vid,
            NewPart {
                build_id: Some(b.id),
                ..minimal_part("P", None, None)
            },
        )
        .await
        .unwrap();
        let o = incident_svc::create(&db, vid, minimal_incident(Some(b.id)))
            .await
            .unwrap();
        assert_eq!(svc.record.build_id, Some(b.id));
        assert_eq!(p.build_id, Some(b.id));
        assert_eq!(o.incident.build_id, Some(b.id));

        delete(&db, vid, b.id).await.unwrap();

        assert!(matches!(
            get(&db, vid, b.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        // Previously-linked records survive with their link nulled.
        assert_eq!(
            svc_svc::get(&db, vid, svc.record.id)
                .await
                .unwrap()
                .record
                .build_id,
            None
        );
        assert_eq!(part_svc::get(&db, vid, p.id).await.unwrap().build_id, None);
        assert_eq!(
            incident_svc::get(&db, vid, o.incident.id)
                .await
                .unwrap()
                .incident
                .build_id,
            None
        );
    }

    #[tokio::test]
    async fn progress_rolls_up_linked_records() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let b = create(&db, vid, new_build("B")).await.unwrap();

        // 2 services with costs, 3 parts (2 installed), 1 incident.
        let s1 = svc_svc::create(&db, vid, minimal_service(Some(10_000), Some(b.id)))
            .await
            .unwrap();
        let s2 = svc_svc::create(&db, vid, minimal_service(Some(2_500), Some(b.id)))
            .await
            .unwrap();
        let p1 = part_svc::create(
            &db,
            vid,
            NewPart {
                build_id: Some(b.id),
                ..minimal_part("P1", Some(5_000), Some("installed".into()))
            },
        )
        .await
        .unwrap();
        let p2 = part_svc::create(
            &db,
            vid,
            NewPart {
                build_id: Some(b.id),
                ..minimal_part("P2", Some(1_500), Some("installed".into()))
            },
        )
        .await
        .unwrap();
        let p3 = part_svc::create(
            &db,
            vid,
            NewPart {
                build_id: Some(b.id),
                ..minimal_part("P3", None, None)
            },
        )
        .await
        .unwrap();
        let o1 = incident_svc::create(&db, vid, minimal_incident(Some(b.id)))
            .await
            .unwrap();

        // Unlinked noise must not leak into the rollup.
        svc_svc::create(&db, vid, minimal_service(Some(99_999), None))
            .await
            .unwrap();
        part_svc::create(&db, vid, minimal_part("Noise", Some(99_999), None))
            .await
            .unwrap();

        let view = progress(&db, vid, b.id).await.unwrap();
        assert_eq!(view.build.id, b.id);
        assert_eq!(view.services_count, 2);
        assert_eq!(view.parts_total, 3);
        assert_eq!(view.parts_installed, 2);
        assert_eq!(view.incidents_count, 1);
        // 10_000 + 2_500 service cents; the linked invoices carry no
        // parts_cost_cents, so the full 5_000 + 1_500 part spend is extra.
        assert_eq!(view.total_cost_cents, 19_000);
        assert_eq!(
            view.linked.service_record_ids,
            vec![s1.record.id, s2.record.id]
        );
        assert_eq!(view.linked.part_ids, vec![p1.id, p2.id, p3.id]);
        assert_eq!(view.linked.incident_ids, vec![o1.incident.id]);
    }

    #[tokio::test]
    async fn progress_does_not_double_count_parts_included_in_service_invoices() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let b = create(&db, vid, new_build("Turbo build")).await.unwrap();

        // $800 invoice that already includes a $500 parts component…
        svc_svc::create(
            &db,
            vid,
            NewServiceRecord {
                parts_cost_cents: Some(50_000),
                ..minimal_service(Some(80_000), Some(b.id))
            },
        )
        .await
        .unwrap();
        // …and the $500 turbo also tracked as a linked part row.
        part_svc::create(
            &db,
            vid,
            NewPart {
                build_id: Some(b.id),
                ..minimal_part("Turbo", Some(50_000), Some("installed".into()))
            },
        )
        .await
        .unwrap();

        // Mirrors services::costs: the part is already inside the invoice
        // total, so the build costs $800 — not $1,300.
        let view = progress(&db, vid, b.id).await.unwrap();
        assert_eq!(view.total_cost_cents, 80_000);
    }

    #[tokio::test]
    async fn progress_excludes_covered_services_from_out_of_pocket() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let b = create(&db, vid, new_build("Crash repair + upgrades"))
            .await
            .unwrap();

        // $100 self-paid service.
        svc_svc::create(&db, vid, minimal_service(Some(10_000), Some(b.id)))
            .await
            .unwrap();
        // $150 insurance-paid service whose invoice includes a $50 parts component.
        svc_svc::create(
            &db,
            vid,
            NewServiceRecord {
                paid_by: Some("insurance".into()),
                parts_cost_cents: Some(5_000),
                ..minimal_service(Some(15_000), Some(b.id))
            },
        )
        .await
        .unwrap();
        // A $50 linked part row (parts have no payer: counts as out-of-pocket;
        // the covered invoice's parts component is excluded from the dedupe).
        part_svc::create(
            &db,
            vid,
            NewPart {
                build_id: Some(b.id),
                ..minimal_part("Fender", Some(5_000), Some("installed".into()))
            },
        )
        .await
        .unwrap();

        let view = progress(&db, vid, b.id).await.unwrap();
        // total_cost_cents keeps the existing formula for continuity:
        // 25_000 services + (5_000 parts - 5_000 invoice parts).max(0) = 25_000.
        assert_eq!(view.total_cost_cents, 25_000);
        // Out of pocket: self services (10_000) + parts beyond self invoices'
        // parts component (5_000 - 0) = 15_000. The covered $150 is excluded.
        assert_eq!(view.out_of_pocket_cents, 15_000);
    }

    #[tokio::test]
    async fn progress_missing_build_is_not_found() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        assert!(matches!(
            progress(&db, vid, 999).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }
}
