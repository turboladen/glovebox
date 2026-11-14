use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Something a work item needs, such as a part or a tool, with the options
/// being weighed for it and the one chosen.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "job_needs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub work_item_id: i32,
    pub name: String,
    pub quantity: i32,
    pub supplied_by: Option<String>,
    pub approach: Option<String>,
    pub chosen_option_id: Option<i32>,
    pub decision_reason: Option<String>,
    pub decided_on: Option<String>,
    pub part_id: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::work_item::Entity",
        from = "Column::WorkItemId",
        to = "super::work_item::Column::Id"
    )]
    WorkItem,
    #[sea_orm(
        belongs_to = "super::need_option::Entity",
        from = "Column::ChosenOptionId",
        to = "super::need_option::Column::Id"
    )]
    ChosenOption,
    #[sea_orm(
        belongs_to = "super::part::Entity",
        from = "Column::PartId",
        to = "super::part::Column::Id"
    )]
    Part,
    #[sea_orm(has_many = "super::need_option::Entity")]
    Options,
}

impl Related<super::work_item::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::WorkItem.def()
    }
}

impl Related<super::part::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Part.def()
    }
}

impl Related<super::need_option::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Options.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
