use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A small task for a vehicle that is not a maintenance item, optionally tied
/// to an appointment.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "todos")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub vehicle_id: i32,
    pub appointment_id: Option<i32>,
    pub title: String,
    pub notes: Option<String>,
    pub done_on: Option<String>,
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
    #[sea_orm(
        belongs_to = "super::appointment::Entity",
        from = "Column::AppointmentId",
        to = "super::appointment::Column::Id"
    )]
    Appointment,
}

impl Related<super::vehicle::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Vehicle.def()
    }
}

impl Related<super::appointment::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Appointment.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
