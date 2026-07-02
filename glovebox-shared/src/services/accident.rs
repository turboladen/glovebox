use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{accident, accident_correspondence, accident_service_link},
    error::{DomainError, DomainResult},
    inputs::accident::{NewAccident, NewCorrespondence, UpdateAccident},
};

#[derive(Debug, Serialize)]
pub struct AccidentWithDetails {
    #[serde(flatten)]
    pub accident: accident::Model,
    pub correspondence: Vec<accident_correspondence::Model>,
    pub service_record_ids: Vec<i32>,
}

/// Load correspondence and service link IDs for a single accident.
async fn load_accident_details(
    db: &impl ConnectionTrait,
    accident_id: i32,
) -> DomainResult<(Vec<accident_correspondence::Model>, Vec<i32>)> {
    let correspondence = accident_correspondence::Entity::find()
        .filter(accident_correspondence::Column::AccidentId.eq(accident_id))
        .order_by_asc(accident_correspondence::Column::OccurredAt)
        .all(db)
        .await?;

    let links = accident_service_link::Entity::find()
        .filter(accident_service_link::Column::AccidentId.eq(accident_id))
        .all(db)
        .await?;
    let service_record_ids = links.into_iter().map(|l| l.service_record_id).collect();

    Ok((correspondence, service_record_ids))
}

pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<Vec<AccidentWithDetails>> {
    let accidents = accident::Entity::find()
        .filter(accident::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(accident::Column::OccurredAt)
        .all(db)
        .await?;

    // Batch-load all correspondence and service links (avoids N+1)
    let accident_ids: Vec<i32> = accidents.iter().map(|a| a.id).collect();

    let all_correspondence = if accident_ids.is_empty() {
        vec![]
    } else {
        accident_correspondence::Entity::find()
            .filter(accident_correspondence::Column::AccidentId.is_in(accident_ids.clone()))
            .order_by_asc(accident_correspondence::Column::OccurredAt)
            .all(db)
            .await?
    };

    let all_links = if accident_ids.is_empty() {
        vec![]
    } else {
        accident_service_link::Entity::find()
            .filter(accident_service_link::Column::AccidentId.is_in(accident_ids))
            .all(db)
            .await?
    };

    let results = accidents
        .into_iter()
        .map(|acc| {
            let correspondence: Vec<_> = all_correspondence
                .iter()
                .filter(|c| c.accident_id == acc.id)
                .cloned()
                .collect();
            let service_record_ids = all_links
                .iter()
                .filter(|l| l.accident_id == acc.id)
                .map(|l| l.service_record_id)
                .collect();
            AccidentWithDetails {
                accident: acc,
                correspondence,
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
) -> DomainResult<AccidentWithDetails> {
    let acc = accident::Entity::find_by_id(id)
        .filter(accident::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Accident {id} not found")))?;

    let (correspondence, service_record_ids) = load_accident_details(db, acc.id).await?;

    Ok(AccidentWithDetails {
        accident: acc,
        correspondence,
        service_record_ids,
    })
}

pub async fn create<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    input: NewAccident,
) -> DomainResult<AccidentWithDetails> {
    let txn = db.begin().await?;

    let model = accident::ActiveModel {
        vehicle_id: Set(vehicle_id),
        occurred_at: Set(input.occurred_at),
        odometer: Set(input.odometer),
        description: Set(input.description),
        fault: Set(input.fault),
        other_party_name: Set(input.other_party_name),
        other_party_phone: Set(input.other_party_phone),
        other_party_email: Set(input.other_party_email),
        other_party_insurance: Set(input.other_party_insurance),
        other_party_policy_number: Set(input.other_party_policy_number),
        insurance_claim_number: Set(input.insurance_claim_number),
        insurance_adjuster: Set(input.insurance_adjuster),
        insurance_adjuster_phone: Set(input.insurance_adjuster_phone),
        notes: Set(input.notes),
        ..Default::default()
    };
    let acc = model.insert(&txn).await?;

    let service_record_ids = input.service_record_ids.unwrap_or_default();
    for sid in &service_record_ids {
        let link = accident_service_link::ActiveModel {
            accident_id: Set(acc.id),
            service_record_id: Set(*sid),
        };
        link.insert(&txn).await?;
    }

    txn.commit().await?;

    Ok(AccidentWithDetails {
        accident: acc,
        correspondence: vec![],
        service_record_ids,
    })
}

#[allow(clippy::too_many_lines)]
pub async fn update<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    input: UpdateAccident,
) -> DomainResult<AccidentWithDetails> {
    let existing = accident::Entity::find_by_id(id)
        .filter(accident::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Accident {id} not found")))?;

    let txn = db.begin().await?;

    let mut active: accident::ActiveModel = existing.into();

    if let Some(v) = input.occurred_at {
        active.occurred_at = Set(v);
    }
    if let Some(v) = input.odometer {
        active.odometer = Set(v);
    }
    if let Some(v) = input.description {
        active.description = Set(v);
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
    if let Some(v) = input.resolved {
        active.resolved = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }

    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

    let acc = active.update(&txn).await?;

    let service_record_ids = if let Some(sids) = input.service_record_ids {
        accident_service_link::Entity::delete_many()
            .filter(accident_service_link::Column::AccidentId.eq(acc.id))
            .exec(&txn)
            .await?;
        for sid in &sids {
            let link = accident_service_link::ActiveModel {
                accident_id: Set(acc.id),
                service_record_id: Set(*sid),
            };
            link.insert(&txn).await?;
        }
        sids
    } else {
        let links = accident_service_link::Entity::find()
            .filter(accident_service_link::Column::AccidentId.eq(acc.id))
            .all(&txn)
            .await?;
        links.into_iter().map(|l| l.service_record_id).collect()
    };

    let correspondence = accident_correspondence::Entity::find()
        .filter(accident_correspondence::Column::AccidentId.eq(acc.id))
        .order_by_asc(accident_correspondence::Column::OccurredAt)
        .all(&txn)
        .await?;

    txn.commit().await?;

    Ok(AccidentWithDetails {
        accident: acc,
        correspondence,
        service_record_ids,
    })
}

pub async fn list_correspondence(
    db: &impl ConnectionTrait,
    accident_id: i32,
) -> DomainResult<Vec<accident_correspondence::Model>> {
    Ok(accident_correspondence::Entity::find()
        .filter(accident_correspondence::Column::AccidentId.eq(accident_id))
        .order_by_asc(accident_correspondence::Column::OccurredAt)
        .all(db)
        .await?)
}

pub async fn create_correspondence(
    db: &impl ConnectionTrait,
    accident_id: i32,
    input: NewCorrespondence,
) -> DomainResult<accident_correspondence::Model> {
    accident::Entity::find_by_id(accident_id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Accident {accident_id} not found")))?;

    let model = accident_correspondence::ActiveModel {
        accident_id: Set(accident_id),
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

    fn new_accident(service_record_ids: Option<Vec<i32>>) -> NewAccident {
        NewAccident {
            occurred_at: "2024-05-01".into(),
            odometer: Some(42_000),
            description: "Rear-ended".into(),
            fault: Some("other".into()),
            other_party_name: None,
            other_party_phone: None,
            other_party_email: None,
            other_party_insurance: None,
            other_party_policy_number: None,
            insurance_claim_number: None,
            insurance_adjuster: None,
            insurance_adjuster_phone: None,
            notes: None,
            service_record_ids,
        }
    }

    #[tokio::test]
    async fn create_links_service_and_get_round_trips() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc_id = seed_service(&db, vid).await;

        let created = create(&db, vid, new_accident(Some(vec![svc_id])))
            .await
            .unwrap();
        assert_eq!(created.service_record_ids, vec![svc_id]);
        assert!(created.correspondence.is_empty());

        let fetched = get(&db, vid, created.accident.id).await.unwrap();
        assert_eq!(fetched.accident.description, "Rear-ended");
        assert_eq!(fetched.service_record_ids, vec![svc_id]);
    }

    #[tokio::test]
    async fn correspondence_create_and_list_relate_to_accident() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let acc = create(&db, vid, new_accident(None)).await.unwrap();

        let c = create_correspondence(
            &db,
            acc.accident.id,
            NewCorrespondence {
                occurred_at: "2024-05-02".into(),
                contact_method: Some("phone".into()),
                contact_with: Some("Adjuster".into()),
                summary: "Filed claim".into(),
                notes: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(c.accident_id, acc.accident.id);

        let listed = list_correspondence(&db, acc.accident.id).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].summary, "Filed claim");

        // The accident get() now surfaces the correspondence too
        let fetched = get(&db, vid, acc.accident.id).await.unwrap();
        assert_eq!(fetched.correspondence.len(), 1);
    }

    #[tokio::test]
    async fn correspondence_on_missing_accident_is_not_found() {
        let db = test_db().await;
        let err = create_correspondence(
            &db,
            999,
            NewCorrespondence {
                occurred_at: "2024-05-02".into(),
                contact_method: None,
                contact_with: None,
                summary: "x".into(),
                notes: None,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)));
    }

    #[tokio::test]
    async fn update_replaces_service_links_and_sets_resolved() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc_a = seed_service(&db, vid).await;
        let svc_b = seed_service(&db, vid).await;
        let acc = create(&db, vid, new_accident(Some(vec![svc_a])))
            .await
            .unwrap();

        let updated = update(
            &db,
            vid,
            acc.accident.id,
            UpdateAccident {
                resolved: Some(true),
                service_record_ids: Some(vec![svc_b]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(updated.accident.resolved);
        assert_eq!(updated.service_record_ids, vec![svc_b]);
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
}
