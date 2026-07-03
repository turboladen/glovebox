use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "builds")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub vehicle_id: i32,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub target_date: Option<String>,
    pub completed_at: Option<String>,
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
    #[sea_orm(has_many = "super::service_record::Entity")]
    ServiceRecord,
    #[sea_orm(has_many = "super::part::Entity")]
    Part,
    #[sea_orm(has_many = "super::observation::Entity")]
    Observation,
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

impl Related<super::part::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Part.def()
    }
}

impl Related<super::observation::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Observation.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
