use sea_orm::*;

use crate::{
    entities::document,
    error::{DomainError, DomainResult},
    inputs::document::{DocumentFilter, NewDocument},
};

pub async fn list(
    db: &impl ConnectionTrait,
    filter: DocumentFilter,
) -> DomainResult<Vec<document::Model>> {
    let mut select = document::Entity::find();

    if let Some(vid) = filter.vehicle_id {
        select = select.filter(document::Column::VehicleId.eq(vid));
    }
    if let Some(ref etype) = filter.linked_entity_type {
        select = select.filter(document::Column::LinkedEntityType.eq(etype.as_str()));
    }
    if let Some(eid) = filter.linked_entity_id {
        select = select.filter(document::Column::LinkedEntityId.eq(eid));
    }

    Ok(select
        .order_by_desc(document::Column::CreatedAt)
        .all(db)
        .await?)
}

pub async fn get(db: &impl ConnectionTrait, id: i32) -> DomainResult<document::Model> {
    document::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Document {id} not found")))
}

/// Persist a document row. The file bytes must already be written to disk by
/// the caller; `input.file_path` is the stored relative path.
pub async fn create(db: &impl ConnectionTrait, input: NewDocument) -> DomainResult<document::Model> {
    let model = document::ActiveModel {
        vehicle_id: Set(input.vehicle_id),
        title: Set(input.title),
        file_path: Set(input.file_path),
        file_name: Set(input.file_name),
        mime_type: Set(input.mime_type),
        file_size_bytes: Set(input.file_size_bytes),
        doc_type: Set(input.doc_type),
        linked_entity_type: Set(input.linked_entity_type),
        linked_entity_id: Set(input.linked_entity_id),
        notes: Set(input.notes),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

pub async fn delete(db: &impl ConnectionTrait, id: i32) -> DomainResult<()> {
    document::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_db;

    fn sample(title: &str) -> NewDocument {
        NewDocument {
            vehicle_id: None,
            title: title.into(),
            file_path: format!("general/other/{title}.pdf"),
            file_name: format!("{title}.pdf"),
            mime_type: Some("application/pdf".into()),
            file_size_bytes: Some(1024),
            doc_type: Some("invoice".into()),
            linked_entity_type: None,
            linked_entity_id: None,
            notes: None,
        }
    }

    #[tokio::test]
    async fn create_then_get_round_trips() {
        let db = test_db().await;
        let created = create(&db, sample("receipt")).await.unwrap();
        let fetched = get(&db, created.id).await.unwrap();
        assert_eq!(fetched.title, "receipt");
        assert_eq!(fetched.file_size_bytes, Some(1024));
    }

    #[tokio::test]
    async fn list_filters_by_linked_entity() {
        let db = test_db().await;
        create(&db, sample("a")).await.unwrap();
        let mut linked = sample("b");
        linked.linked_entity_type = Some("service_record".into());
        linked.linked_entity_id = Some(5);
        create(&db, linked).await.unwrap();

        assert_eq!(list(&db, DocumentFilter::default()).await.unwrap().len(), 2);
        let filtered = list(
            &db,
            DocumentFilter {
                linked_entity_type: Some("service_record".into()),
                linked_entity_id: Some(5),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "b");
    }

    #[tokio::test]
    async fn delete_removes_row() {
        let db = test_db().await;
        let created = create(&db, sample("gone")).await.unwrap();
        delete(&db, created.id).await.unwrap();
        assert!(matches!(
            get(&db, created.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn get_missing_is_not_found() {
        let db = test_db().await;
        assert!(matches!(
            get(&db, 999).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }
}
