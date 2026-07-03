use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A planned shop trip or DIY session grouping work items (2hea unit G).
/// Completing a visit creates the linked service record
/// (`service_record_id`, set by `services::visit::complete`).
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "visits")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub vehicle_id: i32,
    pub planned_date: Option<String>,
    pub shop_name: Option<String>,
    pub shop_id: Option<i32>,
    pub notes: Option<String>,
    pub status: String,
    pub service_record_id: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::vehicle::Entity",
        from = "Column::VehicleId",
        to = "super::vehicle::Column::Id"
    )]
    Vehicle,
    #[sea_orm(has_many = "super::work_item::Entity")]
    WorkItem,
}

impl Related<super::vehicle::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Vehicle.def()
    }
}

impl Related<super::work_item::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::WorkItem.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
