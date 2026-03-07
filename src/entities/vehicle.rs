use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "vehicles")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub model_template_id: Option<i32>,
    pub name: String,
    pub year: Option<i32>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub trim_level: Option<String>,
    pub body_style: Option<String>,
    pub engine: Option<String>,
    pub transmission: Option<String>,
    pub drivetrain: Option<String>,
    pub vin: Option<String>,
    pub license_plate: Option<String>,
    pub color: Option<String>,
    pub purchase_date: Option<String>,
    pub purchase_price_cents: Option<i32>,
    pub purchase_price_currency: Option<String>,
    pub purchase_mileage: Option<i32>,
    pub sold_date: Option<String>,
    pub sold_price_cents: Option<i32>,
    pub sold_price_currency: Option<String>,
    pub sold_mileage: Option<i32>,
    pub photo_path: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::model_template::Entity",
        from = "Column::ModelTemplateId",
        to = "super::model_template::Column::Id"
    )]
    ModelTemplate,
    #[sea_orm(has_many = "super::vehicle_attribute::Entity")]
    VehicleAttribute,
    #[sea_orm(has_many = "super::mileage_log::Entity")]
    MileageLog,
    #[sea_orm(has_many = "super::service_record::Entity")]
    ServiceRecord,
    #[sea_orm(has_many = "super::maintenance_schedule_item::Entity")]
    MaintenanceScheduleItem,
}

impl Related<super::model_template::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ModelTemplate.def()
    }
}

impl Related<super::vehicle_attribute::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::VehicleAttribute.def()
    }
}

impl Related<super::mileage_log::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MileageLog.def()
    }
}

impl Related<super::service_record::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ServiceRecord.def()
    }
}

impl Related<super::maintenance_schedule_item::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MaintenanceScheduleItem.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
