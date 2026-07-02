use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "accidents")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub vehicle_id: i32,
    pub occurred_at: String,
    pub odometer: Option<i32>,
    pub description: String,
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
    pub resolved: bool,
    pub notes: Option<String>,
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
    #[sea_orm(has_many = "super::accident_correspondence::Entity")]
    AccidentCorrespondence,
}

impl Related<super::vehicle::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Vehicle.def()
    }
}

impl Related<super::accident_correspondence::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AccidentCorrespondence.def()
    }
}

impl Related<super::service_record::Entity> for Entity {
    fn to() -> RelationDef {
        super::accident_service_link::Relation::ServiceRecord.def()
    }
    fn via() -> Option<RelationDef> {
        Some(super::accident_service_link::Relation::Accident.def().rev())
    }
}

impl ActiveModelBehavior for ActiveModel {}
