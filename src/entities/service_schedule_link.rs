use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "service_schedule_links")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub service_record_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub schedule_item_id: i32,
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
        belongs_to = "super::maintenance_schedule_item::Entity",
        from = "Column::ScheduleItemId",
        to = "super::maintenance_schedule_item::Column::Id"
    )]
    MaintenanceScheduleItem,
}

impl Related<super::service_record::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ServiceRecord.def()
    }
}

impl Related<super::maintenance_schedule_item::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MaintenanceScheduleItem.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
