use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// One item inside a part bought as a kit, such as the filter in an oil
/// change kit, with the number printed on its own packaging.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "part_contents")]
pub struct Model {
    // Replies carry the id, which names an item to remove, and the fields a
    // caller sends when adding one. The bookkeeping columns are left out.
    #[sea_orm(primary_key)]
    pub id: i32,
    #[serde(skip_serializing)]
    pub part_id: i32,
    /// The item's place in the kit's list, from 0.
    #[serde(skip_serializing)]
    pub position: i32,
    pub name: String,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    /// How many of it one kit holds.
    pub quantity: i32,
    #[serde(skip_serializing)]
    pub created_at: DateTimeUtc,
    #[serde(skip_serializing)]
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::part::Entity",
        from = "Column::PartId",
        to = "super::part::Column::Id"
    )]
    Part,
}

impl Related<super::part::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Part.def()
    }
}

super::stamp_on_insert!(created_at, updated_at);
