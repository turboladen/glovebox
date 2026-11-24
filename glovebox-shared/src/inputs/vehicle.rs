#[derive(Default)]
pub struct NewVehicle {
    pub name: String,
    pub model_template_id: Option<i32>,
    pub year: Option<i32>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub trim_level: Option<String>,
    pub body_style: Option<String>,
    pub engine: Option<String>,
    pub transmission: Option<String>,
    pub drivetrain: Option<String>,
    pub vin: Option<String>,
    pub license_plate: Option<String>,
    pub color: Option<String>,
    pub purchase_date: Option<chrono::NaiveDate>,
    pub purchase_price_cents: Option<i32>,
    pub purchase_mileage: Option<i32>,
    pub notes: Option<String>,
    /// Warranty expiry: date and/or mileage.
    pub warranty_expires_on: Option<chrono::NaiveDate>,
    pub warranty_expires_miles: Option<i32>,
}

#[derive(Default)]
pub struct UpdateVehicle {
    pub name: Option<String>,
    pub model_template_id: Option<Option<i32>>,
    pub year: Option<Option<i32>>,
    pub make: Option<Option<String>>,
    pub model: Option<Option<String>>,
    pub trim_level: Option<Option<String>>,
    pub body_style: Option<Option<String>>,
    pub engine: Option<Option<String>>,
    pub transmission: Option<Option<String>>,
    pub drivetrain: Option<Option<String>>,
    pub vin: Option<Option<String>>,
    pub license_plate: Option<Option<String>>,
    pub color: Option<Option<String>>,
    pub purchase_date: Option<Option<chrono::NaiveDate>>,
    pub purchase_price_cents: Option<Option<i32>>,
    pub purchase_mileage: Option<Option<i32>>,
    pub sold_date: Option<Option<chrono::NaiveDate>>,
    pub sold_price_cents: Option<Option<i32>>,
    pub sold_mileage: Option<Option<i32>>,
    pub notes: Option<Option<String>>,
    pub warranty_expires_on: Option<Option<chrono::NaiveDate>>,
    pub warranty_expires_miles: Option<Option<i32>>,
}

/// What a sold car's sale records. Each field left out keeps the value the
/// vehicle already holds.
#[derive(Default)]
pub struct VehicleSale {
    pub sold_date: Option<chrono::NaiveDate>,
    pub sold_price_cents: Option<i32>,
    pub sold_mileage: Option<i32>,
}
