use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "model_templates")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub platform_id: Option<i32>,
    pub platform_ref: Option<String>,
    pub year: Option<i32>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub trim_level: Option<String>,
    pub body_style: Option<String>,
    pub engine: Option<String>,
    pub transmission: Option<String>,
    pub drivetrain: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::platform::Entity",
        from = "Column::PlatformId",
        to = "super::platform::Column::Id"
    )]
    Platform,
    #[sea_orm(has_many = "super::vehicle::Entity")]
    Vehicle,
    #[sea_orm(has_many = "super::maintenance_schedule_item::Entity")]
    MaintenanceScheduleItem,
}

impl Related<super::platform::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Platform.def()
    }
}

impl Related<super::vehicle::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Vehicle.def()
    }
}

impl Related<super::maintenance_schedule_item::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MaintenanceScheduleItem.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
