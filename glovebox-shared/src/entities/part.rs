use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "parts")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub vehicle_id: i32,
    pub name: String,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub oe_part_number_replaced: Option<String>,
    pub seller: Option<String>,
    pub purchase_date: Option<String>,
    pub cost_cents: Option<i32>,
    pub cost_currency: Option<String>,
    pub status: String,
    pub installed_date: Option<String>,
    pub installed_odometer: Option<i32>,
    pub installed_service_id: Option<i32>,
    pub replaced_date: Option<String>,
    pub replaced_odometer: Option<i32>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub manufacturer_url: Option<String>,
    pub retailer_url: Option<String>,
    pub location: Option<String>,
    pub warranty_expires_on: Option<String>,
    pub warranty_expires_miles: Option<i32>,
    /// Who bought the part: `self` or `shop`.
    pub supplied_by: String,
    pub received_on: Option<String>,
    pub returned_on: Option<String>,
    pub return_reason: Option<String>,
    pub order_ref: Option<String>,
    pub returned_from_need_id: Option<i32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::job_need::Entity",
        from = "Column::ReturnedFromNeedId",
        to = "super::job_need::Column::Id"
    )]
    ReturnedFromNeed,
    #[sea_orm(
        belongs_to = "super::vehicle::Entity",
        from = "Column::VehicleId",
        to = "super::vehicle::Column::Id"
    )]
    Vehicle,
    #[sea_orm(
        belongs_to = "super::service_record::Entity",
        from = "Column::InstalledServiceId",
        to = "super::service_record::Column::Id"
    )]
    ServiceRecord,
}

impl Related<super::vehicle::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Vehicle.def()
    }
}

impl Related<super::service_record::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ServiceRecord.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
