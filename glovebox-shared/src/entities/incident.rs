use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

crate::vocabulary! {
    /// What kind of thing an incident is.
    pub enum IncidentCategory {
        General => "general",
        Noise => "noise",
        Leak => "leak",
        WarningLight => "warning_light",
        Cosmetic => "cosmetic",
        Performance => "performance",
        ObdCode => "obd_code",
        Damage => "damage",
        Accident => "accident",
        Note => "note",
        // Something a shop flagged during a service but did not fix.
        ShopObservation => "shop_observation",
    }
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "incidents")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub vehicle_id: i32,
    pub category: IncidentCategory,
    pub title: String,
    pub description: Option<String>,
    pub odometer: Option<i32>,
    pub occurred_at: crate::dates::DayOrTime,
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
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::vehicle::Entity",
        from = "Column::VehicleId",
        to = "super::vehicle::Column::Id"
    )]
    Vehicle,
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

super::stamp_on_insert!(created_at, updated_at);
