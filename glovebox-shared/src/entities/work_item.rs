use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A planned piece of work — "something I'm actually gonna do" (2hea unit G).
/// Sourced from a schedule item, a research finding (e.g. a recall), an
/// incident, a build, or ad-hoc; optionally grouped into a visit. The source
/// FKs and `visit_id` are plain nullable INTs enforced by the service layer.
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
    pub build_id: Option<i32>,
    pub est_cost_cents: Option<i32>,
    pub status: String,
    pub visit_id: Option<i32>,
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
        belongs_to = "super::visit::Entity",
        from = "Column::VisitId",
        to = "super::visit::Column::Id"
    )]
    Visit,
}

impl Related<super::vehicle::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Vehicle.def()
    }
}

impl Related<super::visit::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Visit.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
