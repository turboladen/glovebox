use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "research_reports")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub vehicle_id: i32,
    pub report_type: Option<String>,
    pub summary: Option<String>,
    pub raw_data: Option<String>,
    pub notes: Option<String>,
    pub generated_at: String,
    pub created_at: String,
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

impl ActiveModelBehavior for ActiveModel {}
