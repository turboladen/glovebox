use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "incidents")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub vehicle_id: i32,
    pub category: String,
    pub title: String,
    pub description: Option<String>,
    pub odometer: Option<i32>,
    pub occurred_at: String,
    pub obd_codes: Option<String>,
    pub resolved: bool,
    pub notes: Option<String>,
    pub fault: Option<String>,
    pub other_party_name: Option<String>,
    pub other_party_phone: Option<String>,
    pub other_party_email: Option<String>,
    pub other_party_insurance: Option<String>,
    pub other_party_policy_number: Option<String>,
    pub insurance_claim_number: Option<String>,
    pub insurance_adjuster: Option<String>,
    pub insurance_adjuster_phone: Option<String>,
    pub total_repair_cost_cents: Option<i32>,
    pub total_repair_cost_currency: Option<String>,
    pub deductible_cents: Option<i32>,
    pub deductible_currency: Option<String>,
    pub insurance_payout_cents: Option<i32>,
    pub insurance_payout_currency: Option<String>,
    /// Self-FK: this incident is a recurrence of an earlier one (same
    /// vehicle, enforced by the service layer). ON DELETE SET NULL.
    pub recurrence_of_id: Option<i32>,
    pub build_id: Option<i32>,
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
        belongs_to = "super::build::Entity",
        from = "Column::BuildId",
        to = "super::build::Column::Id"
    )]
    Build,
    /// Self-referencing recurrence link (queried by column in the service
    /// layer; the relation exists for completeness).
    #[sea_orm(
        belongs_to = "Entity",
        from = "Column::RecurrenceOfId",
        to = "Column::Id"
    )]
    RecurrenceOf,
    #[sea_orm(has_many = "super::incident_followup::Entity")]
    IncidentFollowup,
}

impl Related<super::vehicle::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Vehicle.def()
    }
}

impl Related<super::build::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Build.def()
    }
}

impl Related<super::incident_followup::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::IncidentFollowup.def()
    }
}

impl Related<super::service_record::Entity> for Entity {
    fn to() -> RelationDef {
        super::incident_service_link::Relation::ServiceRecord.def()
    }
    fn via() -> Option<RelationDef> {
        Some(super::incident_service_link::Relation::Incident.def().rev())
    }
}

impl ActiveModelBehavior for ActiveModel {}
