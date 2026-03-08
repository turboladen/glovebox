use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

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
    pub status: String,
    pub linked_entity_type: Option<String>,
    pub linked_entity_id: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
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

impl ActiveModelBehavior for ActiveModel {}
