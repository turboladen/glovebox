use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

crate::vocab::vocabulary! {
    /// What a report groups: the recalls one NHTSA check stored, or the
    /// findings filed by hand for the vehicle.
    pub enum ReportType {
        RecallsOnly => "recalls_only",
        ExternalResearch => "external_research",
    }
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "research_reports")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub vehicle_id: i32,
    pub report_type: Option<ReportType>,
    pub generated_at: DateTimeUtc,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::vehicle::Entity",
        from = "Column::VehicleId",
        to = "super::vehicle::Column::Id"
    )]
    Vehicle,
    #[sea_orm(has_many = "super::research_finding::Entity")]
    Findings,
}

impl Related<super::vehicle::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Vehicle.def()
    }
}

impl Related<super::research_finding::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Findings.def()
    }
}

super::stamp_on_insert!(generated_at, created_at);
