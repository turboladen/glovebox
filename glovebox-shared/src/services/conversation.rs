use sea_orm::*;

use crate::{
    entities::{chat_message, conversation},
    error::{DomainError, DomainResult},
};

pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<Vec<conversation::Model>> {
    Ok(conversation::Entity::find()
        .filter(conversation::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(conversation::Column::UpdatedAt)
        .all(db)
        .await?)
}

pub async fn create(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    title: Option<String>,
) -> DomainResult<conversation::Model> {
    let model = conversation::ActiveModel {
        vehicle_id: Set(Some(vehicle_id)),
        title: Set(title.unwrap_or_else(|| "New Chat".to_string())),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

pub async fn rename(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
    title: String,
) -> DomainResult<conversation::Model> {
    let existing = conversation::Entity::find_by_id(id)
        .filter(conversation::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Conversation {id} not found")))?;

    let mut active: conversation::ActiveModel = existing.into();
    active.title = Set(title);
    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

    Ok(active.update(db).await?)
}

pub async fn delete(db: &impl ConnectionTrait, vehicle_id: i32, id: i32) -> DomainResult<()> {
    // Delete associated chat messages first (no FK cascade on ALTER TABLE ADD COLUMN in SQLite)
    chat_message::Entity::delete_many()
        .filter(chat_message::Column::ConversationId.eq(id))
        .exec(db)
        .await?;

    let result = conversation::Entity::delete_many()
        .filter(conversation::Column::Id.eq(id))
        .filter(conversation::Column::VehicleId.eq(vehicle_id))
        .exec(db)
        .await?;

    if result.rows_affected == 0 {
        return Err(DomainError::NotFound(format!("Conversation {id} not found")));
    }

    Ok(())
}

pub async fn messages(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<Vec<chat_message::Model>> {
    // Verify conversation belongs to this vehicle
    conversation::Entity::find_by_id(id)
        .filter(conversation::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Conversation {id} not found")))?;

    Ok(chat_message::Entity::find()
        .filter(chat_message::Column::ConversationId.eq(id))
        .order_by_asc(chat_message::Column::CreatedAt)
        .all(db)
        .await?)
}

pub async fn add_message(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
    role: String,
    content: String,
) -> DomainResult<chat_message::Model> {
    // Verify conversation belongs to vehicle
    conversation::Entity::find_by_id(id)
        .filter(conversation::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Conversation {id} not found")))?;

    if role != "user" && role != "assistant" {
        return Err(DomainError::invalid(
            "role",
            "role must be 'user' or 'assistant'",
        ));
    }

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let msg = chat_message::ActiveModel {
        vehicle_id: Set(Some(vehicle_id)),
        conversation_id: Set(Some(id)),
        role: Set(role),
        content: Set(content),
        created_at: Set(now.clone()),
        ..Default::default()
    };
    let saved = msg.insert(db).await?;

    // Touch conversation updated_at
    let mut convo_active = conversation::ActiveModel {
        id: Set(id),
        ..Default::default()
    };
    convo_active.updated_at = Set(now);
    convo_active.update(db).await?;

    Ok(saved)
}

/// Recent chat messages, optionally scoped to a vehicle (most recent 100, oldest-first).
pub async fn chat_history(
    db: &impl ConnectionTrait,
    vehicle_id: Option<i32>,
) -> DomainResult<Vec<chat_message::Model>> {
    let mut q = chat_message::Entity::find();
    if let Some(vid) = vehicle_id {
        q = q.filter(chat_message::Column::VehicleId.eq(vid));
    }
    Ok(q.order_by_asc(chat_message::Column::CreatedAt)
        .limit(100)
        .all(db)
        .await?)
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

    #[tokio::test]
    async fn create_default_title_and_rename() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let c = create(&db, vid, None).await.unwrap();
        assert_eq!(c.title, "New Chat");
        let renamed = rename(&db, vid, c.id, "Brakes".into()).await.unwrap();
        assert_eq!(renamed.title, "Brakes");
    }

    #[tokio::test]
    async fn add_message_validates_role_and_lists() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let c = create(&db, vid, None).await.unwrap();
        assert!(matches!(
            add_message(&db, vid, c.id, "system".into(), "x".into())
                .await
                .unwrap_err(),
            DomainError::Invalid { .. }
        ));
        add_message(&db, vid, c.id, "user".into(), "hello".into())
            .await
            .unwrap();
        let msgs = messages(&db, vid, c.id).await.unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "hello");
    }

    #[tokio::test]
    async fn delete_removes_conversation_and_messages() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let c = create(&db, vid, None).await.unwrap();
        add_message(&db, vid, c.id, "user".into(), "hi".into())
            .await
            .unwrap();
        delete(&db, vid, c.id).await.unwrap();
        assert!(matches!(
            messages(&db, vid, c.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(chat_history(&db, Some(vid)).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_missing_is_not_found() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        assert!(matches!(
            delete(&db, vid, 999).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }
}
