#[derive(Default)]
pub struct NewPart {
    pub name: String,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub oe_part_number_replaced: Option<String>,
    pub seller: Option<String>,
    pub purchase_date: Option<String>,
    pub cost_cents: Option<i32>,
    pub cost_currency: Option<String>,
    pub manufacturer_url: Option<String>,
    pub retailer_url: Option<String>,
    pub status: Option<String>,
    pub installed_date: Option<String>,
    pub installed_odometer: Option<i32>,
    pub installed_service_id: Option<i32>,
    pub notes: Option<String>,
    pub location: Option<String>,
    /// Warranty expiry: date (`YYYY-MM-DD`) and/or mileage.
    pub warranty_expires_on: Option<String>,
    pub warranty_expires_miles: Option<i32>,
    /// `self` (default) or `shop`.
    pub supplied_by: Option<String>,
    /// The seller's order or confirmation number.
    pub order_ref: Option<String>,
    /// A need this part fills, honored only for a part created inline on a
    /// service invoice (`NewServiceRecord::parts`): the part is linked to the
    /// need in the same transaction. The need must be on the same vehicle and
    /// have no part linked. Plain part creation ignores this field.
    pub need_id: Option<i32>,
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
    pub manufacturer_url: Option<Option<String>>,
    pub retailer_url: Option<Option<String>>,
    pub status: Option<String>,
    pub installed_date: Option<Option<String>>,
    pub installed_odometer: Option<Option<i32>>,
    pub installed_service_id: Option<Option<i32>>,
    pub replaced_date: Option<Option<String>>,
    pub replaced_odometer: Option<Option<i32>>,
    pub notes: Option<Option<String>>,
    pub location: Option<Option<String>>,
    pub warranty_expires_on: Option<Option<String>>,
    pub warranty_expires_miles: Option<Option<i32>>,
    /// Plain option: the column is non-nullable, so there is no "clear" case.
    pub supplied_by: Option<String>,
    pub order_ref: Option<Option<String>>,
    /// The date the part arrived, `YYYY-MM-DD`.
    pub received_on: Option<Option<String>>,
}

/// Filter for listing parts within a vehicle.
#[derive(Default)]
pub struct PartFilter {
    pub status: Option<String>,
}
