pub struct NewLineItem {
    pub description: String,
    pub category: Option<String>,
    pub quantity: Option<f64>,
    pub unit_cost_cents: Option<i32>,
    pub cost_cents: Option<i32>,
}

pub struct NewServiceRecord {
    pub service_date: String,
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
    pub build_id: Option<i32>,
    /// Who paid: `self` (default), `insurance`, or `third_party`.
    pub paid_by: Option<String>,
    pub payer_note: Option<String>,
    pub schedule_item_ids: Option<Vec<i32>>,
    pub part_ids: Option<Vec<i32>>,
    pub line_items: Option<Vec<NewLineItem>>,
    /// Invoice/receipt number. When present, the record is hard-idempotent on
    /// `(vehicle_id, invoice_ref)`: re-recording the same ref returns the
    /// existing record instead of creating a duplicate.
    pub invoice_ref: Option<String>,
}

#[derive(Default)]
pub struct UpdateServiceRecord {
    pub service_date: Option<String>,
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
    pub build_id: Option<Option<i32>>,
    /// Plain option: the column is non-nullable, so there is no "clear" case.
    pub paid_by: Option<String>,
    pub payer_note: Option<Option<String>>,
    pub schedule_item_ids: Option<Vec<i32>>,
    pub part_ids: Option<Vec<i32>>,
    pub line_items: Option<Vec<NewLineItem>>,
}
