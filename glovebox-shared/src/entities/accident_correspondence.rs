use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "accident_correspondence")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub accident_id: i32,
    pub occurred_at: String,
    pub contact_method: Option<String>,
    pub contact_with: Option<String>,
    pub summary: String,
    pub notes: Option<String>,
    pub created_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::accident::Entity",
        from = "Column::AccidentId",
        to = "super::accident::Column::Id"
    )]
    Accident,
}

impl Related<super::accident::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Accident.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
