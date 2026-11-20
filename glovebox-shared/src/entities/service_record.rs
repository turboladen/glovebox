use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

crate::vocab::vocabulary! {
    /// Who paid for a service or for one of its lines.
    pub enum Payer {
        // The tools call the owner `self`.
        Owner => "self",
        Insurance => "insurance",
        ThirdParty => "third_party",
        Manufacturer => "manufacturer",
    }
}

impl Payer {
    /// Whether someone other than the owner paid, so the amount counts as
    /// covered and not as out of pocket.
    #[must_use]
    pub fn is_covered(self) -> bool {
        self != Payer::Owner
    }
}

crate::vocab::vocabulary! {
    /// Who did the work of a service.
    pub enum Performer {
        Shop => "shop",
        // The tools call the owner `self`.
        Owner => "self",
    }
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "service_records")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub vehicle_id: i32,
    pub service_date: chrono::NaiveDate,
    pub mileage: Option<i32>,
    pub description: Option<String>,
    pub parts_cost_cents: Option<i32>,
    pub parts_cost_currency: Option<String>,
    pub labor_cost_cents: Option<i32>,
    pub labor_cost_currency: Option<String>,
    pub total_cost_cents: Option<i32>,
    pub total_cost_currency: Option<String>,
    // Always NULL: a CHECK keeps it empty, and reads print the name of the
    // shop `shop_id` names instead.
    #[serde(skip_serializing)]
    pub shop_name: Option<String>,
    pub notes: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    pub shop_id: Option<i32>,
    pub paid_by: Payer,
    pub payer_note: Option<String>,
    /// Invoice/receipt number off the scan. Unique per vehicle when present;
    /// re-recording the same ref is idempotent (returns the existing record).
    pub invoice_ref: Option<String>,
    pub performed_by: Performer,
    /// Last day of a multi-day job; `service_date` is the first day.
    pub end_date: Option<chrono::NaiveDate>,
    pub appointment_id: Option<i32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::appointment::Entity",
        from = "Column::AppointmentId",
        to = "super::appointment::Column::Id"
    )]
    Appointment,
    #[sea_orm(
        belongs_to = "super::vehicle::Entity",
        from = "Column::VehicleId",
        to = "super::vehicle::Column::Id"
    )]
    Vehicle,
    #[sea_orm(
        belongs_to = "super::shop::Entity",
        from = "Column::ShopId",
        to = "super::shop::Column::Id"
    )]
    Shop,
    #[sea_orm(has_many = "super::service_record_line_item::Entity")]
    LineItems,
}

impl Related<super::shop::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Shop.def()
    }
}

impl Related<super::vehicle::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Vehicle.def()
    }
}

impl Related<super::service_record_line_item::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::LineItems.def()
    }
}

impl Related<super::maintenance_schedule_item::Entity> for Entity {
    fn to() -> RelationDef {
        super::service_schedule_link::Relation::MaintenanceScheduleItem.def()
    }
    fn via() -> Option<RelationDef> {
        Some(
            super::service_schedule_link::Relation::ServiceRecord
                .def()
                .rev(),
        )
    }
}

super::stamp_on_insert!(created_at, updated_at);
