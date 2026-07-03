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
    pub archived_at: Option<String>,
    // Added by migration 20 (ALTER TABLE appends to end)
    pub warranty_expires_on: Option<String>,
    pub warranty_expires_miles: Option<i32>,
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
    #[sea_orm(has_many = "super::incident::Entity")]
    Incident,
    #[sea_orm(has_many = "super::document::Entity")]
    Document,
    #[sea_orm(has_many = "super::part::Entity")]
    Part,
    #[sea_orm(has_many = "super::research_report::Entity")]
    ResearchReport,
    #[sea_orm(has_many = "super::build::Entity")]
    Build,
    #[sea_orm(has_many = "super::work_item::Entity")]
    WorkItem,
    #[sea_orm(has_many = "super::visit::Entity")]
    Visit,
}

impl Related<super::build::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Build.def()
    }
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

impl Related<super::incident::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Incident.def()
    }
}

impl Related<super::document::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Document.def()
    }
}

impl Related<super::part::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Part.def()
    }
}

impl Related<super::research_report::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ResearchReport.def()
    }
}

impl Related<super::work_item::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::WorkItem.def()
    }
}

impl Related<super::visit::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Visit.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
