use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "platforms")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub website_url: Option<String>,
    pub api_base_url: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::model_template::Entity")]
    ModelTemplate,
    #[sea_orm(has_many = "super::maintenance_schedule_item::Entity")]
    MaintenanceScheduleItem,
}

impl Related<super::model_template::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ModelTemplate.def()
    }
}

impl Related<super::maintenance_schedule_item::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MaintenanceScheduleItem.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
