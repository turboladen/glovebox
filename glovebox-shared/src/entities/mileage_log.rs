use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// The `source` of the reading a service logs, which names that service in
/// `service_record_id`.
pub const SERVICE_SOURCE: &str = "service";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "mileage_log")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub vehicle_id: i32,
    pub mileage: i32,
    pub recorded_at: crate::dates::DayOrTime,
    pub source: Option<String>,
    pub notes: Option<String>,
    pub created_at: DateTimeUtc,
    // Set on logs created by a service record; the activity feed dedupes on it.
    pub service_record_id: Option<i32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::vehicle::Entity",
        from = "Column::VehicleId",
        to = "super::vehicle::Column::Id"
    )]
    Vehicle,
    #[sea_orm(
        belongs_to = "super::service_record::Entity",
        from = "Column::ServiceRecordId",
        to = "super::service_record::Column::Id"
    )]
    ServiceRecord,
}

impl Related<super::service_record::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ServiceRecord.def()
    }
}

impl Related<super::vehicle::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Vehicle.def()
    }
}

super::stamp_on_insert!(created_at);
