use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// One candidate for a job need: a specific part or approach with its price,
/// source and fitment status.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "need_options")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub need_id: i32,
    pub description: String,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub seller: Option<String>,
    pub price_cents: Option<i32>,
    pub url: Option<String>,
    /// Whether the option is known to fit; `unconfirmed` until someone checks.
    pub fitment: String,
    pub fitment_note: Option<String>,
    pub rejected_reason: Option<String>,
    pub rejected_on: Option<chrono::NaiveDate>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    /// The reason a need gave when it chose this option, kept after the
    /// choice is cleared by a rejection.
    pub chosen_reason: Option<String>,
    /// The date that choice was made.
    pub chosen_on: Option<chrono::NaiveDate>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::job_need::Entity",
        from = "Column::NeedId",
        to = "super::job_need::Column::Id"
    )]
    Need,
}

impl Related<super::job_need::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Need.def()
    }
}

super::stamp_on_insert!(created_at, updated_at);
