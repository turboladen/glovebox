use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

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
    pub created_at: String,
    pub updated_at: String,
    /// One of `maintenance`, `mod`, `repair`, `inspection`, `recall`, `fee`.
    pub work_kind: String,
    /// Who paid this line when it differs from the service's `paid_by`;
    /// `None` means the line follows the service.
    pub paid_by: Option<String>,
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

impl ActiveModelBehavior for ActiveModel {}
