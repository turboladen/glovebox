use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use super::service_record::Payer;

crate::vocab::vocabulary! {
    /// What kind of work a line of a service is.
    pub enum WorkKind {
        Maintenance => "maintenance",
        Mod => "mod",
        Repair => "repair",
        Inspection => "inspection",
        // A recall line is work done under a recall campaign.
        Recall => "recall",
        // A fee is a charge on the invoice that is not work, such as tax,
        // shop supplies or disposal.
        Fee => "fee",
    }
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "service_record_line_items")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub service_record_id: i32,
    pub description: String,
    pub quantity: Option<f64>,
    pub unit_cost_cents: Option<i32>,
    pub cost_cents: Option<i32>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    pub work_kind: WorkKind,
    /// Who paid this line when it differs from the service's `paid_by`;
    /// `None` means the line follows the service.
    pub paid_by: Option<Payer>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
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

super::stamp_on_insert!(created_at, updated_at);
