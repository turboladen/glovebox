use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "documents")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub vehicle_id: Option<i32>,
    pub title: String,
    pub file_path: String,
    pub file_name: String,
    pub mime_type: Option<String>,
    pub file_size_bytes: Option<i32>,
    pub doc_type: Option<String>,
    pub linked_entity_type: Option<String>,
    pub linked_entity_id: Option<i32>,
    pub notes: Option<String>,
    pub extracted_text: Option<String>,
    pub created_at: String,
    /// Hex SHA-256 of the stored file bytes. Content-hash dedup keys on
    /// `(vehicle_id, content_sha256)`. NULL only for pre-idempotency rows.
    pub content_sha256: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::vehicle::Entity",
        from = "Column::VehicleId",
        to = "super::vehicle::Column::Id"
    )]
    Vehicle,
}

impl Related<super::vehicle::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Vehicle.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
