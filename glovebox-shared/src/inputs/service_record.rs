use crate::{
    entities::{
        service_record::{Payer, Performer},
        service_record_line_item::WorkKind,
    },
    inputs::part::NewPart,
};

pub struct NewLineItem {
    pub description: String,
    pub work_kind: WorkKind,
    /// `None` means the line follows the service's payer.
    pub paid_by: Option<Payer>,
    pub quantity: Option<f64>,
    pub unit_cost_cents: Option<i32>,
    pub cost_cents: Option<i32>,
}

#[derive(Default)]
pub struct NewServiceRecord {
    pub service_date: chrono::NaiveDate,
    /// Last day of the work for multi-day jobs; never before `service_date`.
    pub end_date: Option<chrono::NaiveDate>,
    /// Defaults to the shop.
    pub performed_by: Option<Performer>,
    pub mileage: Option<i32>,
    pub description: Option<String>,
    pub parts_cost_cents: Option<i32>,
    pub parts_cost_currency: Option<String>,
    pub labor_cost_cents: Option<i32>,
    pub labor_cost_currency: Option<String>,
    pub total_cost_cents: Option<i32>,
    pub total_cost_currency: Option<String>,
    pub shop_name: Option<String>,
    pub shop_id: Option<i32>,
    pub notes: Option<String>,
    /// Defaults to the owner.
    pub paid_by: Option<Payer>,
    pub payer_note: Option<String>,
    pub schedule_item_ids: Option<Vec<i32>>,
    pub part_ids: Option<Vec<i32>>,
    /// Parts created and installed by this record.
    pub parts: Option<Vec<NewPart>>,
    pub line_items: Option<Vec<NewLineItem>>,
    /// Invoice/receipt number. When present, the record is hard-idempotent on
    /// `(vehicle_id, invoice_ref)`: re-recording the same ref returns the
    /// existing record instead of creating a duplicate.
    pub invoice_ref: Option<String>,
    /// To-do items this service completes. Each must be an open item on the
    /// same vehicle; its schedule item, if any, is linked so the reminder
    /// clears.
    pub work_item_ids: Option<Vec<i32>>,
    /// Same-vehicle incidents this service fixed. Each is marked resolved.
    pub resolved_incident_ids: Option<Vec<i32>>,
    /// Same-vehicle incidents first noticed during this service. An incident
    /// can be noticed during only one service.
    pub noticed_incident_ids: Option<Vec<i32>>,
    /// Same-vehicle recalls (research findings with category `recall`) this
    /// service performed.
    pub recall_finding_ids: Option<Vec<i32>>,
    /// The appointment this invoice settles. It must belong to the same
    /// vehicle and be neither cancelled nor closed. Naming it installs and
    /// completes nothing; `work_item_ids` and `part_ids` do that.
    pub appointment_id: Option<i32>,
}

#[derive(Default)]
pub struct UpdateServiceRecord {
    pub service_date: Option<chrono::NaiveDate>,
    pub end_date: Option<Option<chrono::NaiveDate>>,
    /// Plain option: the column is non-nullable, so there is no "clear" case.
    pub performed_by: Option<Performer>,
    pub mileage: Option<Option<i32>>,
    pub description: Option<Option<String>>,
    pub parts_cost_cents: Option<Option<i32>>,
    pub parts_cost_currency: Option<Option<String>>,
    pub labor_cost_cents: Option<Option<i32>>,
    pub labor_cost_currency: Option<Option<String>>,
    pub total_cost_cents: Option<Option<i32>>,
    pub total_cost_currency: Option<Option<String>>,
    pub shop_name: Option<Option<String>>,
    pub shop_id: Option<Option<i32>>,
    pub notes: Option<Option<String>>,
    /// Plain option: the column is non-nullable, so there is no "clear" case.
    pub paid_by: Option<Payer>,
    pub payer_note: Option<Option<String>>,
    pub schedule_item_ids: Option<Vec<i32>>,
    pub part_ids: Option<Vec<i32>>,
    pub line_items: Option<Vec<NewLineItem>>,
    /// Set, change or clear the invoice number. A blank value clears it.
    pub invoice_ref: Option<Option<String>>,
    /// When present, replaces the recalls this service performed.
    pub recall_finding_ids: Option<Vec<i32>>,
    /// When present, replaces the jobs this service completed. A job added
    /// must be planned.
    pub work_item_ids: Option<Vec<i32>>,
    /// When present, replaces the same-vehicle incidents this service's
    /// invoice names as fixed. Each one listed is marked resolved, and one
    /// named beside the job filed from it stays resolved by this service
    /// when that job is taken off.
    pub resolved_incident_ids: Option<Vec<i32>>,
    /// What happened to the work this update takes off the service: the
    /// jobs, fitted parts, recalls and incidents its lists leave out.
    /// Required when it takes any off, and ignored otherwise.
    pub completed_work: Option<crate::services::service_record::CompletedWork>,
    /// Set, change or clear the appointment this invoice settles. The same
    /// rules as `NewServiceRecord::appointment_id` apply to a new value.
    pub appointment_id: Option<Option<i32>>,
}
