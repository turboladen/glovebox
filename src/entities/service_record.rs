use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "service_records")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub vehicle_id: i32,
    pub service_date: String,
    pub mileage: Option<i32>,
    pub description: Option<String>,
    pub parts_cost_cents: Option<i32>,
    pub parts_cost_currency: Option<String>,
    pub labor_cost_cents: Option<i32>,
    pub labor_cost_currency: Option<String>,
    pub total_cost_cents: Option<i32>,
    pub total_cost_currency: Option<String>,
    pub shop_name: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub shop_id: Option<i32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::vehicle::Entity",
        from = "Column::VehicleId",
        to = "super::vehicle::Column::Id"
    )]
    Vehicle,
    #[sea_orm(
        belongs_to = "super::shop::Entity",
        from = "Column::ShopId",
        to = "super::shop::Column::Id"
    )]
    Shop,
}

impl Related<super::shop::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Shop.def()
    }
}

impl Related<super::vehicle::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Vehicle.def()
    }
}

impl Related<super::maintenance_schedule_item::Entity> for Entity {
    fn to() -> RelationDef {
        super::service_schedule_link::Relation::MaintenanceScheduleItem.def()
    }
    fn via() -> Option<RelationDef> {
        Some(super::service_schedule_link::Relation::ServiceRecord.def().rev())
    }
}

impl ActiveModelBehavior for ActiveModel {}
