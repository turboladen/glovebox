use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

crate::vocab::vocabulary! {
    /// Where a finding stands: still to act on, acted on, or set aside.
    pub enum FindingStatus {
        New => "new",
        Completed => "completed",
        Dismissed => "dismissed",
    }
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "research_findings")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub report_id: i32,
    pub category: String,
    pub title: String,
    pub description: Option<String>,
    pub source_url: Option<String>,
    pub severity: Option<String>,
    pub status: FindingStatus,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::research_report::Entity",
        from = "Column::ReportId",
        to = "super::research_report::Column::Id"
    )]
    Report,
}

impl Related<super::research_report::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Report.def()
    }
}

super::stamp_on_insert!(created_at, updated_at);
