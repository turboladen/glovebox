use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use super::part::Supplier;

crate::vocab::vocabulary! {
    /// Whether a need is filled like for like or with something better.
    pub enum Approach {
        Replacement => "replacement",
        Upgrade => "upgrade",
    }
}

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
    pub supplied_by: Option<Supplier>,
    pub approach: Option<Approach>,
    pub chosen_option_id: Option<i32>,
    pub decision_reason: Option<String>,
    pub decided_on: Option<chrono::NaiveDate>,
    pub part_id: Option<i32>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
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

super::stamp_on_insert!(created_at, updated_at);
