pub struct NewPart {
    pub name: String,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub oe_part_number_replaced: Option<String>,
    pub seller: Option<String>,
    pub purchase_date: Option<String>,
    pub cost_cents: Option<i32>,
    pub cost_currency: Option<String>,
    pub invoice_url: Option<String>,
    pub manufacturer_url: Option<String>,
    pub retailer_url: Option<String>,
    pub status: Option<String>,
    pub installed_date: Option<String>,
    pub installed_odometer: Option<i32>,
    pub installed_service_id: Option<i32>,
    pub notes: Option<String>,
    pub build_id: Option<i32>,
    pub location: Option<String>,
    /// Warranty expiry (decision ⑩): date (`YYYY-MM-DD`) and/or mileage.
    pub warranty_expires_on: Option<String>,
    pub warranty_expires_miles: Option<i32>,
}

#[derive(Default)]
pub struct UpdatePart {
    pub name: Option<String>,
    pub manufacturer: Option<Option<String>>,
    pub part_number: Option<Option<String>>,
    pub oe_part_number_replaced: Option<Option<String>>,
    pub seller: Option<Option<String>>,
    pub purchase_date: Option<Option<String>>,
    pub cost_cents: Option<Option<i32>>,
    pub cost_currency: Option<Option<String>>,
    pub invoice_url: Option<Option<String>>,
    pub manufacturer_url: Option<Option<String>>,
    pub retailer_url: Option<Option<String>>,
    pub status: Option<String>,
    pub installed_date: Option<Option<String>>,
    pub installed_odometer: Option<Option<i32>>,
    pub installed_service_id: Option<Option<i32>>,
    pub replaced_date: Option<Option<String>>,
    pub replaced_odometer: Option<Option<i32>>,
    pub notes: Option<Option<String>>,
    pub build_id: Option<Option<i32>>,
    pub location: Option<Option<String>>,
    pub warranty_expires_on: Option<Option<String>>,
    pub warranty_expires_miles: Option<Option<i32>>,
}

/// Filter for listing parts within a vehicle.
#[derive(Default)]
pub struct PartFilter {
    pub status: Option<String>,
}
