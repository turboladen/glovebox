use sea_orm::*;

use crate::{
    entities::{part, service_record},
    error::{DomainError, DomainResult},
    inputs::part::{NewPart, PartFilter, UpdatePart},
};

/// Allowed values for `parts.supplied_by`.
pub const VALID_SUPPLIED_BY: [&str; 2] = ["self", "shop"];

/// Allowed values for `parts.status`.
pub const VALID_STATUSES: [&str; 4] = ["purchased", "installed", "replaced", "returned"];

const RETURNED: &str = "returned";

/// Whether `p` was sent back from its need.
pub(crate) fn is_returned(p: &part::Model) -> bool {
    p.status == RETURNED
}

/// Marks `p` returned today for `reason`, remembering the need it was
/// returned from.
pub(crate) async fn mark_returned(
    db: &impl ConnectionTrait,
    p: part::Model,
    reason: String,
    need_id: i32,
) -> DomainResult<part::Model> {
    let mut active: part::ActiveModel = p.into();
    active.status = Set(RETURNED.to_string());
    active.returned_on = Set(Some(super::today_label()));
    active.return_reason = Set(Some(reason));
    active.returned_from_need_id = Set(Some(need_id));
    active.updated_at = Set(super::now_stamp());
    Ok(active.update(db).await?)
}

// Checks a status a caller asks for. `returned` is in the vocabulary but is
// only reachable through the return action on a need, which records the
// reason and unlinks the part in the same write.
fn validate_requested_status(status: &str) -> DomainResult<()> {
    if !VALID_STATUSES.contains(&status) {
        return Err(DomainError::Invalid {
            field: "status".into(),
            message: format!(
                "'{status}' is not valid. Must be one of: {}",
                VALID_STATUSES.join(", ")
            ),
        });
    }
    if status == RETURNED {
        return Err(DomainError::BadRequest(
            "A part is marked returned by returning it from its need (return_part)".into(),
        ));
    }
    Ok(())
}

// A returned part may only go back to `purchased`.
fn validate_status_change(existing: &part::Model, status: &str) -> DomainResult<()> {
    validate_requested_status(status)?;
    if is_returned(existing) && status != "purchased" {
        return Err(DomainError::BadRequest(format!(
            "Part {} was returned; its status can only change back to purchased",
            existing.id
        )));
    }
    Ok(())
}

// A returned part is not on the car, so it takes no install details unless the
// same update makes it `purchased` again.
fn validate_install_details(existing: &part::Model, input: &UpdatePart) -> DomainResult<()> {
    let goes_back = input.status.as_deref() == Some("purchased");
    let sets_install = matches!(input.installed_service_id, Some(Some(_)))
        || matches!(input.installed_date, Some(Some(_)))
        || matches!(input.installed_odometer, Some(Some(_)));
    if is_returned(existing) && sets_install && !goes_back {
        return Err(DomainError::BadRequest(format!(
            "Part {} was returned and cannot be given install details",
            existing.id
        )));
    }
    Ok(())
}

fn validate_supplied_by(supplied_by: &str) -> DomainResult<()> {
    if VALID_SUPPLIED_BY.contains(&supplied_by) {
        return Ok(());
    }
    Err(DomainError::Invalid {
        field: "supplied_by".into(),
        message: format!(
            "'{supplied_by}' is not valid. Must be one of: {}",
            VALID_SUPPLIED_BY.join(", ")
        ),
    })
}

pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    filter: PartFilter,
) -> DomainResult<Vec<part::Model>> {
    super::vehicle::require(db, vehicle_id).await?;
    let mut query = part::Entity::find().filter(part::Column::VehicleId.eq(vehicle_id));

    if let Some(status) = filter.status {
        query = query.filter(part::Column::Status.eq(status));
    }

    Ok(query.order_by_desc(part::Column::CreatedAt).all(db).await?)
}

pub async fn get(db: &impl ConnectionTrait, vehicle_id: i32, id: i32) -> DomainResult<part::Model> {
    part::Entity::find_by_id(id)
        .filter(part::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Part {id} not found")))
}

pub async fn create(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    input: NewPart,
) -> DomainResult<part::Model> {
    super::vehicle::require(db, vehicle_id).await?;
    let supplied_by = input.supplied_by.unwrap_or_else(|| "self".to_string());
    validate_supplied_by(&supplied_by)?;
    if let Some(status) = &input.status {
        validate_requested_status(status)?;
    }
    super::validate_optional_date("purchase_date", input.purchase_date.as_deref())?;
    super::validate_optional_date("installed_date", input.installed_date.as_deref())?;
    super::validate_optional_date("warranty_expires_on", input.warranty_expires_on.as_deref())?;
    if let Some(service_id) = input.installed_service_id {
        super::service_record::require(db, vehicle_id, service_id).await?;
    }

    let model = part::ActiveModel {
        vehicle_id: Set(vehicle_id),
        name: Set(input.name),
        manufacturer: Set(input.manufacturer),
        part_number: Set(input.part_number),
        oe_part_number_replaced: Set(input.oe_part_number_replaced),
        seller: Set(input.seller),
        purchase_date: Set(input.purchase_date),
        cost_cents: Set(input.cost_cents),
        cost_currency: Set(input.cost_currency),
        manufacturer_url: Set(input.manufacturer_url),
        retailer_url: Set(input.retailer_url),
        status: Set(input.status.unwrap_or_else(|| "purchased".to_string())),
        installed_date: Set(input.installed_date),
        installed_odometer: Set(input.installed_odometer),
        installed_service_id: Set(input.installed_service_id),
        notes: Set(input.notes),
        location: Set(input.location),
        warranty_expires_on: Set(input.warranty_expires_on),
        warranty_expires_miles: Set(input.warranty_expires_miles),
        supplied_by: Set(supplied_by),
        order_ref: Set(input.order_ref),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

pub async fn update(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
    input: UpdatePart,
) -> DomainResult<part::Model> {
    let existing = get(db, vehicle_id, id).await?;

    if let Some(Some(service_id)) = input.installed_service_id {
        super::service_record::require(db, vehicle_id, service_id).await?;
    }

    if let Some(v) = &input.supplied_by {
        validate_supplied_by(v)?;
    }
    if let Some(status) = &input.status {
        validate_status_change(&existing, status)?;
    }
    validate_install_details(&existing, &input)?;
    for (field, value) in [
        ("purchase_date", &input.purchase_date),
        ("installed_date", &input.installed_date),
        ("replaced_date", &input.replaced_date),
        ("received_on", &input.received_on),
        ("warranty_expires_on", &input.warranty_expires_on),
    ] {
        super::validate_optional_date(field, value.as_ref().and_then(|v| v.as_deref()))?;
    }

    let existing_returned = is_returned(&existing);
    let mut active: part::ActiveModel = existing.into();

    if let Some(v) = input.supplied_by {
        active.supplied_by = Set(v);
    }
    if let Some(v) = input.name {
        active.name = Set(v);
    }
    if let Some(v) = input.manufacturer {
        active.manufacturer = Set(v);
    }
    if let Some(v) = input.part_number {
        active.part_number = Set(v);
    }
    if let Some(v) = input.oe_part_number_replaced {
        active.oe_part_number_replaced = Set(v);
    }
    if let Some(v) = input.seller {
        active.seller = Set(v);
    }
    if let Some(v) = input.purchase_date {
        active.purchase_date = Set(v);
    }
    if let Some(v) = input.cost_cents {
        active.cost_cents = Set(v);
    }
    if let Some(v) = input.cost_currency {
        active.cost_currency = Set(v);
    }
    if let Some(v) = input.manufacturer_url {
        active.manufacturer_url = Set(v);
    }
    if let Some(v) = input.retailer_url {
        active.retailer_url = Set(v);
    }
    if let Some(v) = input.status {
        if existing_returned {
            active.returned_on = Set(None);
            active.return_reason = Set(None);
            active.returned_from_need_id = Set(None);
        }
        active.status = Set(v);
    }
    if let Some(v) = input.installed_date {
        active.installed_date = Set(v);
    }
    if let Some(v) = input.installed_odometer {
        active.installed_odometer = Set(v);
    }
    if let Some(v) = input.installed_service_id {
        active.installed_service_id = Set(v);
    }
    if let Some(v) = input.replaced_date {
        active.replaced_date = Set(v);
    }
    if let Some(v) = input.replaced_odometer {
        active.replaced_odometer = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }
    if let Some(v) = input.location {
        active.location = Set(v);
    }
    if let Some(v) = input.warranty_expires_on {
        active.warranty_expires_on = Set(v);
    }
    if let Some(v) = input.warranty_expires_miles {
        active.warranty_expires_miles = Set(v);
    }
    if let Some(v) = input.order_ref {
        active.order_ref = Set(v);
    }
    if let Some(v) = input.received_on {
        active.received_on = Set(v);
    }

    active.updated_at = Set(super::now_stamp());
    Ok(active.update(db).await?)
}

/// Marks a vehicle's part as installed by `record` on the record's date.
/// `odometer` is written to `installed_odometer` as given, so `NotSet`
/// keeps the reading the part already has. A part of another vehicle reads
/// as nonexistent, and a `returned` part is refused with `BadRequest`.
pub(crate) async fn install(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    part_id: i32,
    record: &service_record::Model,
    odometer: ActiveValue<Option<i32>>,
) -> DomainResult<()> {
    let existing = get(db, vehicle_id, part_id).await?;
    if is_returned(&existing) {
        return Err(DomainError::BadRequest(format!(
            "Part {part_id} was returned and cannot be installed"
        )));
    }
    let mut active: part::ActiveModel = existing.into();
    active.status = Set("installed".to_string());
    active.installed_service_id = Set(Some(record.id));
    active.installed_date = Set(Some(record.service_date.clone()));
    active.installed_odometer = odometer;
    active.updated_at = Set(super::now_stamp());
    active.update(db).await?;
    Ok(())
}

/// Moves the install date of every part `record` installed to the record's
/// date, and their install odometer to its mileage when it has one. A record
/// with no mileage leaves each part's own reading.
pub(crate) async fn follow_service(
    db: &impl ConnectionTrait,
    record: &service_record::Model,
) -> DomainResult<()> {
    let mut changes = part::ActiveModel {
        installed_date: Set(Some(record.service_date.clone())),
        updated_at: Set(super::now_stamp()),
        ..Default::default()
    };
    if let Some(miles) = record.mileage {
        changes.installed_odometer = Set(Some(miles));
    }
    part::Entity::update_many()
        .set(changes)
        .filter(part::Column::InstalledServiceId.eq(record.id))
        .exec(db)
        .await?;
    Ok(())
}

/// What [`uninstall`] does to a part's status.
pub(crate) enum UninstallStatus {
    /// The part reads `purchased` whatever its status was.
    Purchased,
    /// Only an `installed` part goes back to `purchased`; a replaced or
    /// returned part keeps its status.
    PurchasedIfInstalled,
}

/// Detaches a part from the service that installed it, clearing its install
/// fields and setting its status as `status` says.
pub(crate) async fn uninstall(
    db: &impl ConnectionTrait,
    p: part::Model,
    status: UninstallStatus,
) -> DomainResult<()> {
    // A returned part keeps its status in both modes.
    let revert = match status {
        _ if is_returned(&p) => false,
        UninstallStatus::Purchased => true,
        UninstallStatus::PurchasedIfInstalled => p.status == "installed",
    };
    let mut active: part::ActiveModel = p.into();
    if revert {
        active.status = Set("purchased".to_string());
    }
    active.installed_service_id = Set(None);
    active.installed_date = Set(None);
    active.installed_odometer = Set(None);
    active.updated_at = Set(super::now_stamp());
    active.update(db).await?;
    Ok(())
}

/// Delete a part.
pub async fn delete(db: &impl ConnectionTrait, vehicle_id: i32, id: i32) -> DomainResult<()> {
    get(db, vehicle_id, id).await?.delete(db).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{VehicleFixture, row_count, test_db};

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

    fn minimal_part(name: &str, installed_service_id: Option<i32>) -> NewPart {
        NewPart {
            name: name.into(),
            manufacturer: None,
            part_number: None,
            oe_part_number_replaced: None,
            seller: None,
            purchase_date: None,
            cost_cents: None,
            cost_currency: None,
            manufacturer_url: None,
            retailer_url: None,
            status: None,
            installed_date: None,
            installed_odometer: None,
            installed_service_id,
            notes: None,
            location: None,
            warranty_expires_on: None,
            warranty_expires_miles: None,
            supplied_by: None,
            order_ref: None,
            need_id: None,
        }
    }

    #[tokio::test]
    async fn create_defaults_status_and_get_round_trips() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = create(
            &db,
            vid,
            NewPart {
                cost_cents: Some(1_599),
                ..minimal_part("Oil filter", None)
            },
        )
        .await
        .unwrap();
        assert_eq!(created.status, "purchased");
        let fetched = get(&db, vid, created.id).await.unwrap();
        assert_eq!(fetched.name, "Oil filter");
        assert_eq!(fetched.cost_cents, Some(1_599));
    }

    #[tokio::test]
    async fn location_round_trips_and_clears() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let created = create(
            &db,
            vid,
            NewPart {
                location: Some("Front brakes".into()),
                ..minimal_part("Brake pads", None)
            },
        )
        .await
        .unwrap();
        assert_eq!(created.location, Some("Front brakes".into()));

        // Omitted location survives an unrelated update.
        let updated = update(
            &db,
            vid,
            created.id,
            UpdatePart {
                status: Some("installed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.location, Some("Front brakes".into()));

        // Explicit null clears it (double-option convention).
        let cleared = update(
            &db,
            vid,
            created.id,
            UpdatePart {
                location: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.location, None);
    }

    #[tokio::test]
    async fn update_preserves_installed_service_link() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc_id = seed_service(&db, vid).await;
        let p = create(&db, vid, minimal_part("Brake pads", Some(svc_id)))
            .await
            .unwrap();
        assert_eq!(p.installed_service_id, Some(svc_id));

        // Update an unrelated field; the installed_service_id link must survive.
        let updated = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                status: Some("installed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.status, "installed");
        assert_eq!(updated.installed_service_id, Some(svc_id));

        // Explicitly clear the link.
        let cleared = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                installed_service_id: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.installed_service_id, None);
    }

    #[tokio::test]
    async fn create_rejects_other_vehicles_service() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign_svc = seed_service(&db, other).await;

        // Referencing another vehicle's service must 404 and create nothing.
        assert!(matches!(
            create(&db, owner, minimal_part("Part", Some(foreign_svc)))
                .await
                .unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(
            list(&db, owner, PartFilter::default())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn update_rejects_other_vehicles_service() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign_svc = seed_service(&db, other).await;
        let p = create(&db, owner, minimal_part("Part", None))
            .await
            .unwrap();

        assert!(matches!(
            update(
                &db,
                owner,
                p.id,
                UpdatePart {
                    installed_service_id: Some(Some(foreign_svc)),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err(),
            DomainError::NotFound(_)
        ));
        // The part is untouched by the rejected update.
        let fetched = get(&db, owner, p.id).await.unwrap();
        assert_eq!(fetched.installed_service_id, None);
    }

    #[tokio::test]
    async fn list_filters_by_status() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        create(
            &db,
            vid,
            NewPart {
                status: Some("installed".into()),
                ..minimal_part("A", None)
            },
        )
        .await
        .unwrap();
        create(
            &db,
            vid,
            NewPart {
                status: Some("purchased".into()),
                ..minimal_part("B", None)
            },
        )
        .await
        .unwrap();

        assert_eq!(
            list(&db, vid, PartFilter::default()).await.unwrap().len(),
            2
        );
        assert_eq!(
            list(
                &db,
                vid,
                PartFilter {
                    status: Some("purchased".into())
                }
            )
            .await
            .unwrap()
            .len(),
            1
        );
    }

    // --- delete ----------------------------------------------------------

    #[tokio::test]
    async fn delete_wrong_vehicle_is_byte_identical_to_nonexistent() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let other_vid = seed_vehicle(&db).await;
        let part = create(&db, vid, minimal_part("Mine", None)).await.unwrap();

        let wrong_parent = delete(&db, other_vid, part.id).await.unwrap_err();
        // Compare against the nonexistent-id message for the SAME id: no
        // ownership oracle distinguishing "exists elsewhere" from "not at all".
        let DomainError::NotFound(wrong_parent_msg) = wrong_parent else {
            panic!("expected NotFound");
        };
        assert_eq!(wrong_parent_msg, format!("Part {} not found", part.id));
        // The part survives a wrong-parent delete attempt.
        assert!(get(&db, vid, part.id).await.is_ok());
    }

    // --- supplied_by ------------------------------------------------------

    #[tokio::test]
    async fn supplied_by_defaults_to_self_and_is_validated_on_create_and_update() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let made = create(&db, vid, minimal_part("Pads", None)).await.unwrap();
        assert_eq!(made.supplied_by, "self");

        let shop_part = create(
            &db,
            vid,
            NewPart {
                supplied_by: Some("shop".into()),
                ..minimal_part("Fluid", None)
            },
        )
        .await
        .unwrap();
        assert_eq!(shop_part.supplied_by, "shop");

        let err = create(
            &db,
            vid,
            NewPart {
                supplied_by: Some("dealer".into()),
                ..minimal_part("Bad", None)
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "supplied_by"));

        let updated = update(
            &db,
            vid,
            made.id,
            UpdatePart {
                supplied_by: Some("shop".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.supplied_by, "shop");

        let err = update(
            &db,
            vid,
            made.id,
            UpdatePart {
                supplied_by: Some("dealer".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "supplied_by"));
    }

    #[tokio::test]
    async fn a_returned_part_cannot_be_installed() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sid = seed_service(&db, vid).await;
        let record = service_record::Entity::find_by_id(sid)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let p = seed_returned(&db, vid).await;
        let err = install(&db, vid, p.id, &record, NotSet).await.unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        let after = get(&db, vid, p.id).await.unwrap();
        assert_eq!(after.status, "returned");
        assert_eq!(after.installed_service_id, None);
    }

    #[tokio::test]
    async fn order_ref_and_received_on_round_trip() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let p = create(
            &db,
            vid,
            NewPart {
                order_ref: Some("A-1".into()),
                ..minimal_part("Rotors", None)
            },
        )
        .await
        .unwrap();
        assert_eq!(p.order_ref.as_deref(), Some("A-1"));
        let updated = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                received_on: Some(Some("2026-11-05".into())),
                order_ref: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.received_on.as_deref(), Some("2026-11-05"));
        assert_eq!(updated.order_ref, None);
        let err = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                received_on: Some(Some("yesterday".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { .. }), "{err:?}");
    }

    async fn seed_returned(db: &impl ConnectionTrait, vid: i32) -> part::Model {
        part::ActiveModel {
            vehicle_id: Set(vid),
            name: Set("Rotors".into()),
            status: Set("returned".into()),
            supplied_by: Set("self".into()),
            returned_on: Set(Some("2026-11-06".into())),
            return_reason: Set(Some("wrong size".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn create_and_update_reject_an_unknown_status() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let err = create(
            &db,
            vid,
            NewPart {
                status: Some("lost".into()),
                ..minimal_part("Rotors", None)
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "status"));
        let p = create(&db, vid, minimal_part("Rotors", None))
            .await
            .unwrap();
        let err = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                status: Some("lost".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "status"));
        assert_eq!(get(&db, vid, p.id).await.unwrap().status, "purchased");
    }

    #[tokio::test]
    async fn returned_cannot_be_set_through_create_or_update() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let err = create(
            &db,
            vid,
            NewPart {
                status: Some("returned".into()),
                ..minimal_part("Rotors", None)
            },
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&err, DomainError::BadRequest(m) if m.contains("return_part")),
            "{err:?}"
        );
        let p = create(&db, vid, minimal_part("Rotors", None))
            .await
            .unwrap();
        let err = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                status: Some("returned".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&err, DomainError::BadRequest(m) if m.contains("return_part")),
            "{err:?}"
        );
        assert_eq!(get(&db, vid, p.id).await.unwrap().status, "purchased");
        assert_eq!(row_count::<part::Entity>(&db).await, 1);
    }

    #[tokio::test]
    async fn a_returned_part_can_only_go_back_to_purchased_and_loses_its_return_fields() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let p = seed_returned(&db, vid).await;
        for status in ["installed", "replaced", "returned"] {
            let err = update(
                &db,
                vid,
                p.id,
                UpdatePart {
                    status: Some(status.into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
            assert!(
                matches!(err, DomainError::BadRequest(_)),
                "{status}: {err:?}"
            );
        }
        let noted = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                notes: Some(Some("still at the door".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(noted.status, "returned");
        assert_eq!(noted.return_reason.as_deref(), Some("wrong size"));
        let back = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                status: Some("purchased".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(back.status, "purchased");
        assert_eq!(back.returned_on, None);
        assert_eq!(back.return_reason, None);
        assert_eq!(back.returned_from_need_id, None);
    }

    #[tokio::test]
    async fn deleting_a_service_leaves_a_returned_part_returned() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sid = seed_service(&db, vid).await;
        let p = seed_returned(&db, vid).await;
        part::ActiveModel {
            id: Unchanged(p.id),
            installed_service_id: Set(Some(sid)),
            ..Default::default()
        }
        .update(&db)
        .await
        .unwrap();
        super::super::service_record::delete(&db, vid, sid, None)
            .await
            .unwrap();
        let after = get(&db, vid, p.id).await.unwrap();
        assert_eq!(after.status, "returned");
        assert_eq!(after.returned_on.as_deref(), Some("2026-11-06"));
        assert_eq!(after.return_reason.as_deref(), Some("wrong size"));
        assert_eq!(after.installed_service_id, None);
    }

    #[tokio::test]
    async fn a_returned_part_cannot_be_given_install_details() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let sid = seed_service(&db, vid).await;
        let p = seed_returned(&db, vid).await;
        let attempts = [
            UpdatePart {
                installed_service_id: Some(Some(sid)),
                ..Default::default()
            },
            UpdatePart {
                installed_date: Some(Some("2026-11-10".into())),
                ..Default::default()
            },
            UpdatePart {
                installed_odometer: Some(Some(50_000)),
                ..Default::default()
            },
        ];
        for attempt in attempts {
            let err = update(&db, vid, p.id, attempt).await.unwrap_err();
            assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        }
        let cleared = update(
            &db,
            vid,
            p.id,
            UpdatePart {
                installed_date: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.status, "returned");
    }
}
