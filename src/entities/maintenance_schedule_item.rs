use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "maintenance_schedule_items")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub platform_id: Option<i32>,
    pub model_template_id: Option<i32>,
    pub vehicle_id: Option<i32>,
    pub overrides_item_id: Option<i32>,
    pub name: String,
    pub description: Option<String>,
    pub interval_miles: Option<i32>,
    pub interval_months: Option<i32>,
    pub labor_categories: Option<String>,
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
    #[sea_orm(
        belongs_to = "super::model_template::Entity",
        from = "Column::ModelTemplateId",
        to = "super::model_template::Column::Id"
    )]
    ModelTemplate,
    #[sea_orm(
        belongs_to = "super::vehicle::Entity",
        from = "Column::VehicleId",
        to = "super::vehicle::Column::Id"
    )]
    Vehicle,
}

impl Related<super::platform::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Platform.def()
    }
}

impl Related<super::model_template::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ModelTemplate.def()
    }
}

impl Related<super::vehicle::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Vehicle.def()
    }
}

impl Related<super::service_record::Entity> for Entity {
    fn to() -> RelationDef {
        super::service_schedule_link::Relation::ServiceRecord.def()
    }
    fn via() -> Option<RelationDef> {
        Some(super::service_schedule_link::Relation::MaintenanceScheduleItem.def().rev())
    }
}

impl ActiveModelBehavior for ActiveModel {}
