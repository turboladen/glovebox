use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A planned piece of work. It can come from a schedule item, a research
/// finding (such as a recall), an incident, or be ad hoc. The source ids are
/// plain nullable integers that the service layer checks against the vehicle;
/// `completed_service_id` is a real foreign key that clears when the service goes.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "work_items")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub vehicle_id: i32,
    pub title: String,
    pub notes: Option<String>,
    pub schedule_item_id: Option<i32>,
    pub research_finding_id: Option<i32>,
    pub incident_id: Option<i32>,
    pub est_cost_cents: Option<i32>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub revisit_by_date: Option<String>,
    pub revisit_by_miles: Option<i32>,
    pub completed_service_id: Option<i32>,
    pub appointment_id: Option<i32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::appointment::Entity",
        from = "Column::AppointmentId",
        to = "super::appointment::Column::Id"
    )]
    Appointment,
    #[sea_orm(
        belongs_to = "super::vehicle::Entity",
        from = "Column::VehicleId",
        to = "super::vehicle::Column::Id"
    )]
    Vehicle,
    #[sea_orm(
        belongs_to = "super::service_record::Entity",
        from = "Column::CompletedServiceId",
        to = "super::service_record::Column::Id"
    )]
    CompletedService,
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
