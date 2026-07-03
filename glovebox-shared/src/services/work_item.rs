//! The `work_item` planning primitive (2hea unit G, decision ⑦): "the list
//! of things I'm actually gonna do." Items are ad-hoc or sourced from a
//! schedule item / research finding (recall) / incident / build, and can be
//! grouped into a visit. Every source link is vehicle-scoped (paxy
//! discipline): a cross-vehicle reference must be indistinguishable from a
//! nonexistent one.

use sea_orm::*;

use crate::{
    entities::{incident, research_finding, research_report, visit, work_item},
    error::{DomainError, DomainResult},
    inputs::work_item::{NewWorkItem, UpdateWorkItem},
};

/// Lifecycle whitelist for `work_items.status`.
const VALID_STATUSES: [&str; 4] = ["planned", "scheduled", "done", "dropped"];

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

/// Verify a referenced research finding belongs to the vehicle (via its
/// report). A cross-vehicle finding must be indistinguishable from a
/// nonexistent one.
async fn require_finding_owned(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    finding_id: i32,
) -> DomainResult<()> {
    let not_found = || DomainError::NotFound(format!("Research finding {finding_id} not found"));
    let finding = research_finding::Entity::find_by_id(finding_id)
        .one(db)
        .await?
        .ok_or_else(not_found)?;
    research_report::Entity::find_by_id(finding.report_id)
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(not_found)?;
    Ok(())
}

/// Verify a referenced incident belongs to the vehicle.
async fn require_incident_owned(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    incident_id: i32,
) -> DomainResult<()> {
    incident::Entity::find_by_id(incident_id)
        .filter(incident::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Incident {incident_id} not found")))?;
    Ok(())
}

/// Verify a referenced visit belongs to the vehicle.
pub(crate) async fn require_visit_owned(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    visit_id: i32,
) -> DomainResult<visit::Model> {
    visit::Entity::find_by_id(visit_id)
        .filter(visit::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Visit {visit_id} not found")))
}

/// Guard every present source link on a work item (the 5 paxy kinds):
/// schedule item (vehicle scope chain), research finding (via its report),
/// incident, build, and visit — each vehicle-scoped.
async fn require_links_owned(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    schedule_item_id: Option<i32>,
    research_finding_id: Option<i32>,
    incident_id: Option<i32>,
    build_id: Option<i32>,
    visit_id: Option<i32>,
) -> DomainResult<()> {
    if let Some(id) = schedule_item_id {
        crate::services::schedule::require_in_vehicle_scope(db, vehicle_id, id).await?;
    }
    if let Some(id) = research_finding_id {
        require_finding_owned(db, vehicle_id, id).await?;
    }
    if let Some(id) = incident_id {
        require_incident_owned(db, vehicle_id, id).await?;
    }
    if let Some(id) = build_id {
        crate::services::build::require_owned(db, vehicle_id, id).await?;
    }
    if let Some(id) = visit_id {
        require_visit_owned(db, vehicle_id, id).await?;
    }
    Ok(())
}

pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    include_done: bool,
) -> DomainResult<Vec<work_item::Model>> {
    crate::services::vehicle::require(db, vehicle_id).await?;
    let mut query = work_item::Entity::find().filter(work_item::Column::VehicleId.eq(vehicle_id));
    if !include_done {
        query = query.filter(work_item::Column::Status.is_not_in(["done", "dropped"]));
    }
    Ok(query
        .order_by_desc(work_item::Column::CreatedAt)
        .order_by_desc(work_item::Column::Id)
        .all(db)
        .await?)
}

pub async fn get(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<work_item::Model> {
    work_item::Entity::find_by_id(id)
        .filter(work_item::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Work item {id} not found")))
}

pub async fn create(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    input: NewWorkItem,
) -> DomainResult<work_item::Model> {
    if input.title.trim().is_empty() {
        return Err(DomainError::invalid("title", "must not be blank"));
    }
    crate::services::vehicle::require(db, vehicle_id).await?;
    require_links_owned(
        db,
        vehicle_id,
        input.schedule_item_id,
        input.research_finding_id,
        input.incident_id,
        input.build_id,
        input.visit_id,
    )
    .await?;

    // Born attached to a visit → born scheduled (matches visit attach flow).
    let status = if input.visit_id.is_some() {
        "scheduled"
    } else {
        "planned"
    };

    let model = work_item::ActiveModel {
        vehicle_id: Set(vehicle_id),
        title: Set(input.title),
        notes: Set(input.notes),
        schedule_item_id: Set(input.schedule_item_id),
        research_finding_id: Set(input.research_finding_id),
        incident_id: Set(input.incident_id),
        build_id: Set(input.build_id),
        est_cost_cents: Set(input.est_cost_cents),
        status: Set(status.to_string()),
        visit_id: Set(input.visit_id),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

pub async fn update(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
    input: UpdateWorkItem,
) -> DomainResult<work_item::Model> {
    let existing = get(db, vehicle_id, id).await?;

    if let Some(title) = &input.title
        && title.trim().is_empty()
    {
        return Err(DomainError::invalid("title", "must not be blank"));
    }
    if let Some(status) = &input.status {
        validate_status(status)?;
    }
    // Same guards as create, on the links this update actually changes.
    require_links_owned(
        db,
        vehicle_id,
        input.schedule_item_id.flatten(),
        input.research_finding_id.flatten(),
        input.incident_id.flatten(),
        input.build_id.flatten(),
        input.visit_id.flatten(),
    )
    .await?;

    let mut active: work_item::ActiveModel = existing.into();

    if let Some(v) = input.title {
        active.title = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }
    if let Some(v) = input.schedule_item_id {
        active.schedule_item_id = Set(v);
    }
    if let Some(v) = input.research_finding_id {
        active.research_finding_id = Set(v);
    }
    if let Some(v) = input.incident_id {
        active.incident_id = Set(v);
    }
    if let Some(v) = input.build_id {
        active.build_id = Set(v);
    }
    if let Some(v) = input.est_cost_cents {
        active.est_cost_cents = Set(v);
    }
    if let Some(v) = input.status {
        active.status = Set(v);
    }
    if let Some(v) = input.visit_id {
        active.visit_id = Set(v);
    }

    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
    Ok(active.update(db).await?)
}

pub async fn delete(db: &impl ConnectionTrait, vehicle_id: i32, id: i32) -> DomainResult<()> {
    let existing = get(db, vehicle_id, id).await?;
    existing.delete(db).await?;
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

    async fn seed_schedule_item(db: &impl ConnectionTrait, vehicle_id: i32) -> i32 {
        use crate::entities::maintenance_schedule_item;
        maintenance_schedule_item::ActiveModel {
            vehicle_id: Set(Some(vehicle_id)),
            name: Set("Oil change".into()),
            enabled: Set(true),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn seed_finding(db: &impl ConnectionTrait, vehicle_id: i32) -> i32 {
        let report = research_report::ActiveModel {
            vehicle_id: Set(vehicle_id),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        research_finding::ActiveModel {
            report_id: Set(report.id),
            category: Set("recall".into()),
            title: Set("Recall".into()),
            status: Set("new".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn seed_incident(db: &impl ConnectionTrait, vehicle_id: i32) -> i32 {
        incident::ActiveModel {
            vehicle_id: Set(vehicle_id),
            category: Set("noise".into()),
            title: Set("Squeak".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn seed_build(db: &impl ConnectionTrait, vehicle_id: i32) -> i32 {
        use crate::entities::build;
        build::ActiveModel {
            vehicle_id: Set(vehicle_id),
            name: Set("Build".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn seed_visit(db: &impl ConnectionTrait, vehicle_id: i32) -> i32 {
        visit::ActiveModel {
            vehicle_id: Set(vehicle_id),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    fn item(title: &str) -> NewWorkItem {
        NewWorkItem {
            title: title.into(),
            notes: None,
            schedule_item_id: None,
            research_finding_id: None,
            incident_id: None,
            build_id: None,
            est_cost_cents: None,
            visit_id: None,
        }
    }

    #[tokio::test]
    async fn create_defaults_planned_and_round_trips() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = create(
            &db,
            vid,
            NewWorkItem {
                notes: Some("front axle only".into()),
                est_cost_cents: Some(12_500),
                ..item("Replace front brakes")
            },
        )
        .await
        .unwrap();
        assert_eq!(created.status, "planned");
        assert_eq!(created.est_cost_cents, Some(12_500));

        let fetched = get(&db, vid, created.id).await.unwrap();
        assert_eq!(fetched.title, "Replace front brakes");
        assert_eq!(fetched.notes.as_deref(), Some("front axle only"));

        let listed = list(&db, vid, false).await.unwrap();
        assert_eq!(listed.len(), 1);
    }

    #[tokio::test]
    async fn create_with_visit_is_born_scheduled() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let visit_id = seed_visit(&db, vid).await;
        let created = create(
            &db,
            vid,
            NewWorkItem {
                visit_id: Some(visit_id),
                ..item("Alignment")
            },
        )
        .await
        .unwrap();
        assert_eq!(created.status, "scheduled");
        assert_eq!(created.visit_id, Some(visit_id));
    }

    #[tokio::test]
    async fn create_rejects_blank_title() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        assert!(matches!(
            create(&db, vid, item("   ")).await.unwrap_err(),
            DomainError::Invalid { field, .. } if field == "title"
        ));
        assert!(list(&db, vid, true).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_missing_vehicle_is_not_found() {
        let db = test_db().await;
        assert!(matches!(
            create(&db, 999, item("x")).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    // --- the 5 source-link wrong-vehicle probes -----------------------------

    #[tokio::test]
    async fn create_rejects_other_vehicles_schedule_item() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign = seed_schedule_item(&db, other).await;
        assert!(matches!(
            create(
                &db,
                owner,
                NewWorkItem {
                    schedule_item_id: Some(foreign),
                    ..item("t")
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(list(&db, owner, true).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_rejects_other_vehicles_research_finding() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign = seed_finding(&db, other).await;
        assert!(matches!(
            create(
                &db,
                owner,
                NewWorkItem {
                    research_finding_id: Some(foreign),
                    ..item("t")
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(list(&db, owner, true).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_rejects_other_vehicles_incident() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign = seed_incident(&db, other).await;
        assert!(matches!(
            create(
                &db,
                owner,
                NewWorkItem {
                    incident_id: Some(foreign),
                    ..item("t")
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(list(&db, owner, true).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_rejects_other_vehicles_build() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign = seed_build(&db, other).await;
        assert!(matches!(
            create(
                &db,
                owner,
                NewWorkItem {
                    build_id: Some(foreign),
                    ..item("t")
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(list(&db, owner, true).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_rejects_other_vehicles_visit() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign = seed_visit(&db, other).await;
        assert!(matches!(
            create(
                &db,
                owner,
                NewWorkItem {
                    visit_id: Some(foreign),
                    ..item("t")
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(list(&db, owner, true).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn update_rejects_other_vehicles_links_and_mutates_nothing() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign_incident = seed_incident(&db, other).await;
        let created = create(&db, owner, item("t")).await.unwrap();

        assert!(matches!(
            update(
                &db,
                owner,
                created.id,
                UpdateWorkItem {
                    incident_id: Some(Some(foreign_incident)),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert_eq!(get(&db, owner, created.id).await.unwrap().incident_id, None);
    }

    #[tokio::test]
    async fn happy_links_round_trip() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sched = seed_schedule_item(&db, vid).await;
        let finding = seed_finding(&db, vid).await;
        let inc = seed_incident(&db, vid).await;
        let build = seed_build(&db, vid).await;

        let created = create(
            &db,
            vid,
            NewWorkItem {
                schedule_item_id: Some(sched),
                research_finding_id: Some(finding),
                incident_id: Some(inc),
                build_id: Some(build),
                ..item("Everything sourced")
            },
        )
        .await
        .unwrap();
        assert_eq!(created.schedule_item_id, Some(sched));
        assert_eq!(created.research_finding_id, Some(finding));
        assert_eq!(created.incident_id, Some(inc));
        assert_eq!(created.build_id, Some(build));
    }

    #[tokio::test]
    async fn update_status_whitelist_and_double_option_clear() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = create(
            &db,
            vid,
            NewWorkItem {
                est_cost_cents: Some(5_000),
                ..item("t")
            },
        )
        .await
        .unwrap();

        // Bogus status rejected with the house message shape; nothing mutated.
        let err = update(
            &db,
            vid,
            created.id,
            UpdateWorkItem {
                status: Some("bogus".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        match err {
            DomainError::BadRequest(msg) => {
                assert!(
                    msg.contains("Invalid status 'bogus'") && msg.contains("dropped"),
                    "message must name the bad value and list valid ones: {msg}"
                );
            }
            other => panic!("expected BadRequest, got {other:?}"),
        }
        assert_eq!(get(&db, vid, created.id).await.unwrap().status, "planned");

        // Valid status applies; explicit null clears est_cost_cents.
        let updated = update(
            &db,
            vid,
            created.id,
            UpdateWorkItem {
                status: Some("done".into()),
                est_cost_cents: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.status, "done");
        assert_eq!(updated.est_cost_cents, None);
    }

    #[tokio::test]
    async fn list_hides_done_and_dropped_unless_included() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let a = create(&db, vid, item("open")).await.unwrap();
        let b = create(&db, vid, item("finished")).await.unwrap();
        let c = create(&db, vid, item("abandoned")).await.unwrap();
        for (id, status) in [(b.id, "done"), (c.id, "dropped")] {
            update(
                &db,
                vid,
                id,
                UpdateWorkItem {
                    status: Some(status.into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }

        let open = list(&db, vid, false).await.unwrap();
        assert_eq!(
            open.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![a.id],
            "done/dropped hidden by default"
        );
        assert_eq!(list(&db, vid, true).await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn get_and_delete_are_vehicle_scoped() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let created = create(&db, owner, item("mine")).await.unwrap();

        assert!(matches!(
            get(&db, other, created.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(matches!(
            delete(&db, other, created.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));

        delete(&db, owner, created.id).await.unwrap();
        assert!(matches!(
            get(&db, owner, created.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }
}
