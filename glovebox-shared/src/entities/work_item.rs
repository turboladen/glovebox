use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

crate::vocab::vocabulary! {
    /// Where a job stands.
    pub enum JobStatus {
        Planned => "planned",
        Done => "done",
        // The owner decided not to do it.
        Dropped => "dropped",
    }
}

impl JobStatus {
    /// Whether nothing more will happen on the job: it is done or dropped.
    #[must_use]
    pub fn is_finished(self) -> bool {
        match self {
            JobStatus::Planned => false,
            JobStatus::Done | JobStatus::Dropped => true,
        }
    }
}

/// A planned piece of work. It can come from a schedule item, a research
/// finding (such as a recall), an incident, or be ad hoc. The service layer
/// checks each source id against the vehicle. A source id,
/// `completed_service_id`, `appointment_id` and `quote_service_id` each clear
/// when the row they name is deleted.
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
    pub status: JobStatus,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    pub revisit_by_date: Option<chrono::NaiveDate>,
    pub revisit_by_miles: Option<i32>,
    pub completed_service_id: Option<i32>,
    pub appointment_id: Option<i32>,
    /// Who gave the quote this job came from, such as a shop.
    pub quoted_by: Option<String>,
    /// The day the quote was given.
    pub quoted_on: Option<chrono::NaiveDate>,
    /// The service whose visit produced the quote.
    pub quote_service_id: Option<i32>,
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

super::stamp_on_insert!(created_at, updated_at);
