use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Records that a service performed the work a recall (a research finding) called for.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "service_recall_links")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub service_record_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub research_finding_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::service_record::Entity",
        from = "Column::ServiceRecordId",
        to = "super::service_record::Column::Id"
    )]
    ServiceRecord,
    #[sea_orm(
        belongs_to = "super::research_finding::Entity",
        from = "Column::ResearchFindingId",
        to = "super::research_finding::Column::Id"
    )]
    ResearchFinding,
}

impl Related<super::service_record::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ServiceRecord.def()
    }
}

impl Related<super::research_finding::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ResearchFinding.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
