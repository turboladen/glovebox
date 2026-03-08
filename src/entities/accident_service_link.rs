use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "accident_service_links")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub accident_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub service_record_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::accident::Entity",
        from = "Column::AccidentId",
        to = "super::accident::Column::Id"
    )]
    Accident,
    #[sea_orm(
        belongs_to = "super::service_record::Entity",
        from = "Column::ServiceRecordId",
        to = "super::service_record::Column::Id"
    )]
    ServiceRecord,
}

impl Related<super::accident::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Accident.def()
    }
}

impl Related<super::service_record::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ServiceRecord.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
