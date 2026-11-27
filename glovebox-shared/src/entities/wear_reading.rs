use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

crate::vocab::vocabulary! {
    /// The part of the car a wear reading measures.
    pub enum WearComponent {
        BrakePads => "brake_pads",
        BrakeRotors => "brake_rotors",
        TireTread => "tire_tread",
    }
}

crate::vocab::vocabulary! {
    /// The axle a reading was taken on, or a brake part is fitted to.
    pub enum Axle {
        Front => "front",
        Rear => "rear",
    }
}

crate::vocab::vocabulary! {
    /// The unit a reading or a limit was written in.
    pub enum LengthUnit {
        Millimeter => "mm",
        // Thirty-seconds of an inch, the unit of US tread gauges.
        ThirtySecondInch => "in_32",
        Inch => "in",
    }
}

/// One measured thickness or tread depth, kept as the shop wrote it.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "wear_readings")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub vehicle_id: i32,
    pub service_id: Option<i32>,
    pub component: WearComponent,
    pub axle: Axle,
    pub value: f64,
    pub unit: LengthUnit,
    pub limit_value: Option<f64>,
    pub limit_unit: Option<LengthUnit>,
    pub measured_on: chrono::NaiveDate,
    pub odometer: Option<i32>,
    pub notes: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
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
        from = "Column::ServiceId",
        to = "super::service_record::Column::Id"
    )]
    ServiceRecord,
}

impl Related<super::vehicle::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Vehicle.def()
    }
}

impl Related<super::service_record::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ServiceRecord.def()
    }
}

super::stamp_on_insert!(created_at, updated_at);
