//! The shop directory: one record per shop that services and appointments
//! name, with merging for duplicates and each shop's history.

use std::collections::HashMap;

use sea_orm::{sea_query::Expr, *};
use serde::Serialize;

use crate::{
    entities::{appointment, service_record, shop, vehicle},
    error::{DomainError, DomainResult},
    inputs::shop::{NewShop, UpdateShop},
};

use super::{appointment::AppointmentSummary, trim_to_none};

/// The form of a shop's name or address that the directory compares: ASCII
/// case and surrounding whitespace do not matter.
fn text_key(text: &str) -> String {
    text.trim().to_ascii_lowercase()
}

/// A shop with how much of the records names it.
#[derive(Debug, Serialize)]
pub struct ShopListing {
    #[serde(flatten)]
    pub shop: shop::Model,
    pub service_count: u64,
    pub appointment_count: u64,
    /// The date of the newest service at this shop; `None` when it has none.
    pub last_service_on: Option<chrono::NaiveDate>,
}

/// What a shop has done across every vehicle.
#[derive(Debug, Serialize)]
pub struct ShopHistory {
    #[serde(flatten)]
    pub shop: shop::Model,
    /// The shop's services, newest first.
    pub services: Vec<ShopService>,
    /// The shop's appointments, soonest planned date first, then those with
    /// no date.
    pub appointments: Vec<ShopAppointment>,
    /// The sum of the services' `total_cost_cents`.
    pub total_cost_cents: i64,
    /// The owner's share of those totals.
    pub out_of_pocket_cents: i64,
}

/// One service in a shop's history.
#[derive(Debug, Serialize)]
pub struct ShopService {
    pub id: i32,
    pub vehicle_id: i32,
    pub vehicle_name: String,
    pub service_date: chrono::NaiveDate,
    pub description: Option<String>,
    pub total_cost_cents: Option<i32>,
    /// The owner's share of the total; `None` when the service has no total.
    pub out_of_pocket_cents: Option<i64>,
}

/// One appointment in a shop's history, as every appointment read summarizes
/// it, with its vehicle's name.
#[derive(Debug, Serialize)]
pub struct ShopAppointment {
    #[serde(flatten)]
    pub summary: AppointmentSummary,
    pub vehicle_name: String,
}

/// The result of merging shops into one.
#[derive(Debug, Serialize)]
pub struct MergedShops {
    /// The shop that was kept, with the fields it took from the others.
    pub shop: shop::Model,
    pub merged_shop_ids: Vec<i32>,
    /// The services that now name the kept shop.
    pub moved_service_ids: Vec<i32>,
    /// The appointments that now name the kept shop.
    pub moved_appointment_ids: Vec<i32>,
    /// Values of the merged shops that the kept shop did not take, because it
    /// already had a different one.
    pub discarded: Vec<DiscardedValue>,
}

/// A field value a merge left behind.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DiscardedValue {
    pub shop_id: i32,
    pub field: &'static str,
    pub value: String,
}

fn not_found(id: i32) -> DomainError {
    DomainError::NotFound(format!("Shop {id} not found"))
}

/// Loads a shop.
///
/// # Errors
/// `NotFound` when no shop has this id.
pub async fn get(db: &impl ConnectionTrait, id: i32) -> DomainResult<shop::Model> {
    shop::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| not_found(id))
}

/// Refuses a shop id that names no shop, with `NotFound`; `None` passes.
// The shop directory is global, so an id is checked for existence only. A
// missing shop would otherwise surface as an opaque foreign-key failure.
pub(crate) async fn require_if_named(
    db: &impl ConnectionTrait,
    shop_id: Option<i32>,
) -> DomainResult<()> {
    if let Some(id) = shop_id {
        get(db, id).await?;
    }
    Ok(())
}

// Two shops are the same when their names match by `text_key` and their
// addresses match the same way or either has none. Two locations of one
// chain, each with its address, are two shops.
async fn refuse_duplicate(
    db: &impl ConnectionTrait,
    name: &str,
    address: Option<&str>,
    except_id: Option<i32>,
) -> DomainResult<()> {
    let key = text_key(name);
    let address_key = address.map(text_key);
    let same = shop::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .filter(|s| Some(s.id) != except_id && text_key(&s.name) == key)
        .find(|s| match (&address_key, &s.address) {
            (Some(a), Some(b)) => *a == text_key(b),
            _ => true,
        });
    match same {
        Some(s) => Err(DomainError::BadRequest(format!(
            "Shop {} is already {}{}. Use that shop, or give this one a different address",
            s.id,
            s.name,
            s.address.as_deref().map_or_default(|a| format!(" at {a}"))
        ))),
        None => Ok(()),
    }
}

fn trimmed(value: Option<&str>) -> Option<String> {
    value.and_then(trim_to_none)
}

fn required_name(name: &str) -> DomainResult<String> {
    trim_to_none(name).ok_or_else(|| DomainError::invalid("name", "must not be blank"))
}

/// Adds a shop to the directory. Blank optional fields are stored as absent.
///
/// # Errors
/// `Invalid` for a blank name; `BadRequest` when the directory already holds
/// this shop: the same name, ignoring case and surrounding spaces, at the same
/// address or with either address unknown.
pub async fn create<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    input: NewShop,
) -> DomainResult<shop::Model> {
    let name = required_name(&input.name)?;
    let address = trimmed(input.address.as_deref());
    let txn = super::begin_write(db).await?;
    refuse_duplicate(&txn, &name, address.as_deref(), None).await?;
    let created = shop::ActiveModel {
        name: Set(name),
        address: Set(address),
        phone: Set(trimmed(input.phone.as_deref())),
        website: Set(trimmed(input.website.as_deref())),
        specialty: Set(trimmed(input.specialty.as_deref())),
        notes: Set(trimmed(input.notes.as_deref())),
        ..Default::default()
    }
    .insert(&txn)
    .await?;
    txn.commit().await?;
    Ok(created)
}

/// Changes a shop's details. Blank optional fields are stored as absent.
///
/// # Errors
/// `NotFound` for a shop that does not exist; `Invalid` for a blank name;
/// `BadRequest` when the change would make it a duplicate of another shop,
/// as [`create`] judges one.
pub async fn update<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    id: i32,
    input: UpdateShop,
) -> DomainResult<shop::Model> {
    let name = input.name.as_deref().map(required_name).transpose()?;
    let address = input.address.map(|a| trimmed(a.as_deref()));
    let txn = super::begin_write(db).await?;
    let existing = get(&txn, id).await?;
    if name.is_some() || address.is_some() {
        let new_name = name.as_deref().unwrap_or(&existing.name);
        let new_address = address
            .as_ref()
            .map_or(existing.address.as_deref(), Option::as_deref);
        refuse_duplicate(&txn, new_name, new_address, Some(id)).await?;
    }
    let mut active: shop::ActiveModel = existing.into();
    if let Some(v) = name {
        active.name = Set(v);
    }
    if let Some(v) = address {
        active.address = Set(v);
    }
    if let Some(v) = input.phone {
        active.phone = Set(trimmed(v.as_deref()));
    }
    if let Some(v) = input.website {
        active.website = Set(trimmed(v.as_deref()));
    }
    if let Some(v) = input.specialty {
        active.specialty = Set(trimmed(v.as_deref()));
    }
    if let Some(v) = input.notes {
        active.notes = Set(trimmed(v.as_deref()));
    }
    active.updated_at = Set(super::now());
    let updated = active.update(&txn).await?;
    txn.commit().await?;
    Ok(updated)
}

/// Every shop in name order, with its service and appointment counts and the
/// date of its newest service.
pub async fn list(db: &impl ConnectionTrait) -> DomainResult<Vec<ShopListing>> {
    let shops = shop::Entity::find()
        .order_by_asc(Expr::cust("name COLLATE NOCASE"))
        .order_by_asc(shop::Column::Id)
        .all(db)
        .await?;
    let services: HashMap<i32, (i64, Option<chrono::NaiveDate>)> = service_record::Entity::find()
        .select_only()
        .column(service_record::Column::ShopId)
        .column_as(service_record::Column::Id.count(), "count")
        .column_as(service_record::Column::ServiceDate.max(), "last")
        .filter(service_record::Column::ShopId.is_not_null())
        .group_by(service_record::Column::ShopId)
        .into_tuple::<(i32, i64, Option<chrono::NaiveDate>)>()
        .all(db)
        .await?
        .into_iter()
        .map(|(id, count, last)| (id, (count, last)))
        .collect();
    let appointments: HashMap<i32, i64> = appointment::Entity::find()
        .select_only()
        .column(appointment::Column::ShopId)
        .column_as(appointment::Column::Id.count(), "count")
        .filter(appointment::Column::ShopId.is_not_null())
        .group_by(appointment::Column::ShopId)
        .into_tuple::<(i32, i64)>()
        .all(db)
        .await?
        .into_iter()
        .collect();
    Ok(shops
        .into_iter()
        .map(|shop| {
            let (service_count, last_service_on) =
                services.get(&shop.id).copied().unwrap_or((0, None));
            ShopListing {
                service_count: service_count.unsigned_abs(),
                appointment_count: appointments
                    .get(&shop.id)
                    .copied()
                    .unwrap_or(0)
                    .unsigned_abs(),
                last_service_on,
                shop,
            }
        })
        .collect())
}

async fn vehicle_names(
    db: &impl ConnectionTrait,
    ids: impl IntoIterator<Item = i32>,
) -> DomainResult<HashMap<i32, String>> {
    let ids: std::collections::BTreeSet<i32> = ids.into_iter().collect();
    Ok(vehicle::Entity::find()
        .select_only()
        .column(vehicle::Column::Id)
        .column(vehicle::Column::Name)
        .filter(vehicle::Column::Id.is_in(ids))
        .into_tuple::<(i32, String)>()
        .all(db)
        .await?
        .into_iter()
        .collect())
}

/// A shop with every service and appointment that names it, across all
/// vehicles, and what those services cost. `today` sets each appointment's
/// phase.
///
/// # Errors
/// `NotFound` for a shop that does not exist.
pub async fn history(
    db: &impl ConnectionTrait,
    id: i32,
    today: chrono::NaiveDate,
) -> DomainResult<ShopHistory> {
    let shop = get(db, id).await?;
    let services = service_record::Entity::find()
        .filter(service_record::Column::ShopId.eq(id))
        .order_by_desc(service_record::Column::ServiceDate)
        .order_by_desc(service_record::Column::Id)
        .all(db)
        .await?;
    let appointments = super::appointment::at_shop(db, id, today).await?;
    let names = vehicle_names(
        db,
        services
            .iter()
            .map(|s| s.vehicle_id)
            .chain(appointments.iter().map(|a| a.appointment.vehicle_id)),
    )
    .await?;
    let name_of = |vehicle_id: i32| names.get(&vehicle_id).cloned().unwrap_or_default();
    let out_of_pocket = super::costs::out_of_pocket(db, &services).await?;
    let total_cost_cents = services
        .iter()
        .filter_map(|s| s.total_cost_cents)
        .map(i64::from)
        .sum();
    Ok(ShopHistory {
        shop,
        total_cost_cents,
        out_of_pocket_cents: out_of_pocket.values().sum(),
        services: services
            .into_iter()
            .map(|s| ShopService {
                out_of_pocket_cents: out_of_pocket.get(&s.id).copied(),
                vehicle_name: name_of(s.vehicle_id),
                id: s.id,
                vehicle_id: s.vehicle_id,
                service_date: s.service_date,
                description: s.description,
                total_cost_cents: s.total_cost_cents,
            })
            .collect(),
        appointments: appointments
            .into_iter()
            .map(|summary| ShopAppointment {
                vehicle_name: name_of(summary.appointment.vehicle_id),
                summary,
            })
            .collect(),
    })
}

// Folds the merged shops' details into the kept one: an empty field takes
// the first merged value, notes are joined, and every other value that
// differs is reported, so nothing the owner typed goes missing unsaid.
fn fold_details(kept: &mut shop::Model, merged: &[shop::Model]) -> Vec<DiscardedValue> {
    let mut discarded = Vec::new();
    for other in merged {
        let mut take = |field: &'static str, slot: &mut Option<String>, value: &Option<String>| {
            let Some(value) = value.clone() else {
                return;
            };
            match slot {
                None => *slot = Some(value),
                Some(current) if text_key(current) == text_key(&value) => {}
                Some(_) => discarded.push(DiscardedValue {
                    shop_id: other.id,
                    field,
                    value,
                }),
            }
        };
        take("address", &mut kept.address, &other.address);
        take("phone", &mut kept.phone, &other.phone);
        take("website", &mut kept.website, &other.website);
        take("specialty", &mut kept.specialty, &other.specialty);
        if let Some(notes) = &other.notes {
            kept.notes = Some(match kept.notes.take() {
                Some(current) if current.contains(notes.as_str()) => current,
                Some(current) => format!("{current}\n\n{notes}"),
                None => notes.clone(),
            });
        }
    }
    discarded
}

/// Folds duplicate shops into one: every service and appointment that names a
/// merged shop names the kept one, the kept shop takes the details it lacks,
/// and the merged shops are deleted, all in one transaction. The kept shop
/// keeps its name.
///
/// # Errors
/// `BadRequest` when `merge_ids` is empty or names the kept shop; `NotFound`
/// for a shop that does not exist, which writes nothing.
pub async fn merge<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    keep_id: i32,
    merge_ids: &[i32],
) -> DomainResult<MergedShops> {
    let merge_ids = super::dedup_ids(merge_ids);
    if merge_ids.is_empty() {
        return Err(DomainError::BadRequest(
            "Name at least one shop to merge into the kept one".into(),
        ));
    }
    if merge_ids.contains(&keep_id) {
        return Err(DomainError::BadRequest(format!(
            "Shop {keep_id} is the one being kept, so it cannot also be merged"
        )));
    }
    let txn = super::begin_write(db).await?;
    let mut kept = get(&txn, keep_id).await?;
    let mut merged = Vec::with_capacity(merge_ids.len());
    for id in &merge_ids {
        merged.push(get(&txn, *id).await?);
    }
    let now = super::now();
    let mut moved_service_ids: Vec<i32> = service_record::Entity::update_many()
        .col_expr(service_record::Column::ShopId, Expr::value(keep_id))
        .col_expr(service_record::Column::UpdatedAt, Expr::value(now))
        .filter(service_record::Column::ShopId.is_in(merge_ids.iter().copied()))
        .exec_with_returning(&txn)
        .await?
        .into_iter()
        .map(|s| s.id)
        .collect();
    moved_service_ids.sort_unstable();
    let mut moved_appointment_ids: Vec<i32> = appointment::Entity::update_many()
        .col_expr(appointment::Column::ShopId, Expr::value(keep_id))
        .col_expr(appointment::Column::UpdatedAt, Expr::value(now))
        .filter(appointment::Column::ShopId.is_in(merge_ids.iter().copied()))
        .exec_with_returning(&txn)
        .await?
        .into_iter()
        .map(|a| a.id)
        .collect();
    moved_appointment_ids.sort_unstable();
    let discarded = fold_details(&mut kept, &merged);
    shop::Entity::delete_many()
        .filter(shop::Column::Id.is_in(merge_ids.iter().copied()))
        .exec(&txn)
        .await?;
    let mut active = shop::ActiveModel::from(kept).reset_all();
    active.updated_at = Set(now);
    let shop = active.update(&txn).await?;
    txn.commit().await?;
    Ok(MergedShops {
        shop,
        merged_shop_ids: merge_ids,
        moved_service_ids,
        moved_appointment_ids,
        discarded,
    })
}

/// Deletes a shop that no service or appointment names.
///
/// # Errors
/// `NotFound` for a shop that does not exist; `BadRequest` while a service or
/// appointment names it, since deleting it would leave them naming no shop.
pub async fn delete<C: ConnectionTrait + TransactionTrait>(db: &C, id: i32) -> DomainResult<()> {
    let txn = super::begin_write(db).await?;
    let existing = get(&txn, id).await?;
    let services = service_record::Entity::find()
        .filter(service_record::Column::ShopId.eq(id))
        .count(&txn)
        .await?;
    let appointments = appointment::Entity::find()
        .filter(appointment::Column::ShopId.eq(id))
        .count(&txn)
        .await?;
    if services + appointments > 0 {
        return Err(DomainError::BadRequest(format!(
            "{} is named by {services} service(s) and {appointments} appointment(s). Merge it \
             into the shop that replaces it instead, so that history keeps its shop",
            existing.name
        )));
    }
    shop::Entity::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::{service_record::Payer, service_record_line_item},
        test_support::{VehicleFixture, date, race, race_pools, row_count, test_db},
    };

    fn named(name: &str, address: Option<&str>) -> NewShop {
        NewShop {
            name: name.into(),
            address: address.map(Into::into),
            ..Default::default()
        }
    }

    async fn service_at(
        db: &impl ConnectionTrait,
        vehicle_id: i32,
        shop_id: i32,
        on: &str,
        total: Option<i32>,
    ) -> service_record::Model {
        service_record::ActiveModel {
            vehicle_id: Set(vehicle_id),
            service_date: Set(date(on)),
            shop_id: Set(Some(shop_id)),
            total_cost_cents: Set(total),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn appointment_at(
        db: &impl ConnectionTrait,
        vehicle_id: i32,
        shop_id: i32,
    ) -> appointment::Model {
        appointment::ActiveModel {
            vehicle_id: Set(vehicle_id),
            purpose: Set("Brakes".into()),
            shop_id: Set(Some(shop_id)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn get_missing_is_not_found() {
        let db = test_db().await;
        assert!(matches!(
            get(&db, 999).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn create_trims_and_refuses_a_blank_name() {
        let db = test_db().await;
        let shop = create(
            &db,
            NewShop {
                name: "  KW Automotive ".into(),
                phone: Some("  ".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(shop.name, "KW Automotive");
        assert_eq!(shop.phone, None);

        let err = create(&db, named(" ", None)).await.unwrap_err();
        assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "name"));
    }

    #[tokio::test]
    async fn a_second_shop_of_the_same_name_needs_its_own_address() {
        let db = test_db().await;
        let first = create(&db, named("America's Tire", Some("433 E Shaw Ave")))
            .await
            .unwrap();
        create(&db, named("america's tire", Some("833 E Shaw Ave")))
            .await
            .unwrap();
        for address in [None, Some(" 433 e shaw ave ")] {
            let err = create(&db, named("AMERICA'S TIRE ", address))
                .await
                .unwrap_err();
            assert!(
                matches!(err, DomainError::BadRequest(ref m) if m.contains(&format!("Shop {}", first.id))),
                "{err}"
            );
        }
    }

    #[tokio::test]
    async fn update_refuses_becoming_a_duplicate_but_not_staying_itself() {
        let db = test_db().await;
        let kw = create(&db, named("KW Automotive", None)).await.unwrap();
        let other = create(&db, named("Kurata's", None)).await.unwrap();

        let same = update(
            &db,
            kw.id,
            UpdateShop {
                name: Some("kw automotive".into()),
                phone: Some(Some("555-0100".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(same.name, "kw automotive");
        assert_eq!(same.phone.as_deref(), Some("555-0100"));

        let err = update(
            &db,
            other.id,
            UpdateShop {
                name: Some("KW Automotive".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err}");

        let cleared = update(
            &db,
            kw.id,
            UpdateShop {
                phone: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.phone, None);
        assert!(matches!(
            update(&db, 999, UpdateShop::default()).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn two_adds_of_one_shop_at_once_store_one_shop() {
        let (_dir, pools) = race_pools().await;
        let results = race(&pools, |_, db| async move {
            create(&db, named("Tint Pros", None)).await
        })
        .await;
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(row_count::<shop::Entity>(&pools[0]).await, 1);
    }

    #[tokio::test]
    async fn list_counts_what_names_each_shop() {
        let db = test_db().await;
        let vid = VehicleFixture::new().insert_id(&db).await;
        let busy = create(&db, named("busy", None)).await.unwrap();
        let idle = create(&db, named("Idle", None)).await.unwrap();
        service_at(&db, vid, busy.id, "2025-01-02", None).await;
        service_at(&db, vid, busy.id, "2025-06-30", None).await;
        appointment_at(&db, vid, busy.id).await;

        let listed = list(&db).await.unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|l| (
                    l.shop.id,
                    l.service_count,
                    l.appointment_count,
                    l.last_service_on
                ))
                .collect::<Vec<_>>(),
            [
                (busy.id, 2, 1, Some(date("2025-06-30"))),
                (idle.id, 0, 0, None)
            ]
        );
    }

    #[tokio::test]
    async fn history_lists_the_shops_work_and_the_owners_share() {
        let db = test_db().await;
        let golf = VehicleFixture::new().name("Golf").insert_id(&db).await;
        let alltrack = VehicleFixture::new().name("Alltrack").insert_id(&db).await;
        let shop = create(&db, named("Clovis VW", None)).await.unwrap();
        let older = service_at(&db, golf, shop.id, "2025-01-02", Some(10_000)).await;
        let newer = service_at(&db, alltrack, shop.id, "2025-06-30", Some(30_000)).await;
        // A covered line comes off the owner's share.
        service_record_line_item::ActiveModel {
            service_record_id: Set(newer.id),
            description: Set("Recall".into()),
            work_kind: Set(service_record_line_item::WorkKind::Recall),
            cost_cents: Set(Some(20_000)),
            paid_by: Set(Some(Payer::Manufacturer)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let elsewhere = create(&db, named("Elsewhere", None)).await.unwrap();
        service_at(&db, golf, elsewhere.id, "2025-03-01", Some(99)).await;
        let visit = appointment_at(&db, golf, shop.id).await;

        let got = history(&db, shop.id, date("2026-11-13")).await.unwrap();
        assert_eq!(
            got.services
                .iter()
                .map(|s| (s.id, s.vehicle_name.as_str(), s.out_of_pocket_cents))
                .collect::<Vec<_>>(),
            [
                (newer.id, "Alltrack", Some(10_000)),
                (older.id, "Golf", Some(10_000))
            ]
        );
        assert_eq!(got.total_cost_cents, 40_000);
        assert_eq!(got.out_of_pocket_cents, 20_000);
        assert_eq!(
            got.appointments
                .iter()
                .map(|a| a.summary.appointment.id)
                .collect::<Vec<_>>(),
            [visit.id]
        );
        assert!(matches!(
            history(&db, 999, date("2026-11-13")).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn merge_moves_the_records_and_keeps_every_detail() {
        let db = test_db().await;
        let vid = VehicleFixture::new().insert_id(&db).await;
        let kept = create(
            &db,
            NewShop {
                name: "Clovis Volkswagen".into(),
                phone: Some("555-0100".into()),
                notes: Some("Ask for Sam".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let dup = create(
            &db,
            NewShop {
                name: "Volkswagen of Clovis".into(),
                address: Some("Clovis Ave".into()),
                phone: Some("555-0199".into()),
                notes: Some("Loaner cars".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let moved = service_at(&db, vid, dup.id, "2025-01-02", None).await;
        let stays = service_at(&db, vid, kept.id, "2025-02-02", None).await;
        let visit = appointment_at(&db, vid, dup.id).await;

        let got = merge(&db, kept.id, &[dup.id, dup.id]).await.unwrap();
        assert_eq!(got.merged_shop_ids, [dup.id]);
        assert_eq!(got.moved_service_ids, [moved.id]);
        assert_eq!(got.moved_appointment_ids, [visit.id]);
        assert_eq!(got.shop.name, "Clovis Volkswagen");
        assert_eq!(got.shop.address.as_deref(), Some("Clovis Ave"));
        assert_eq!(got.shop.phone.as_deref(), Some("555-0100"));
        assert_eq!(
            got.shop.notes.as_deref(),
            Some("Ask for Sam\n\nLoaner cars")
        );
        assert_eq!(
            got.discarded,
            [DiscardedValue {
                shop_id: dup.id,
                field: "phone",
                value: "555-0199".into()
            }]
        );
        assert_eq!(get(&db, kept.id).await.unwrap(), got.shop);
        assert!(matches!(
            get(&db, dup.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        let history = history(&db, kept.id, date("2026-11-13")).await.unwrap();
        assert_eq!(
            history.services.iter().map(|s| s.id).collect::<Vec<_>>(),
            [stays.id, moved.id]
        );
    }

    #[tokio::test]
    async fn merge_refuses_a_bad_request_and_writes_nothing() {
        let db = test_db().await;
        let vid = VehicleFixture::new().insert_id(&db).await;
        let kept = create(&db, named("A", None)).await.unwrap();
        let dup = create(&db, named("B", None)).await.unwrap();
        service_at(&db, vid, dup.id, "2025-01-02", None).await;

        for ids in [vec![], vec![kept.id]] {
            assert!(matches!(
                merge(&db, kept.id, &ids).await.unwrap_err(),
                DomainError::BadRequest(_)
            ));
        }
        let err = merge(&db, kept.id, &[dup.id, 999]).await.unwrap_err();
        assert_eq!(err.to_string(), "Shop 999 not found");
        assert_eq!(
            history(&db, dup.id, date("2026-11-13"))
                .await
                .unwrap()
                .services
                .len(),
            1
        );
        assert!(matches!(
            merge(&db, 999, &[dup.id]).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn delete_refuses_a_shop_the_records_name() {
        let db = test_db().await;
        let vid = VehicleFixture::new().insert_id(&db).await;
        let used = create(&db, named("Used", None)).await.unwrap();
        let unused = create(&db, named("Unused", None)).await.unwrap();
        appointment_at(&db, vid, used.id).await;

        let err = delete(&db, used.id).await.unwrap_err();
        assert!(
            matches!(err, DomainError::BadRequest(ref m) if m.contains("1 appointment")),
            "{err}"
        );
        delete(&db, unused.id).await.unwrap();
        assert!(matches!(
            delete(&db, unused.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }
}
