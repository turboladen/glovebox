//! Inputs for the vehicle and mileage tools, and the row `list_vehicles`
//! prints.

use glovebox_shared::{
    dates,
    entities::vehicle,
    error::DomainResult,
    inputs::{
        mileage::NewMileageEntry,
        vehicle::{NewVehicle, UpdateVehicle, VehicleSale},
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::nullable;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListVehiclesInput {
    /// `true` also lists the archived (sold) cars. Omit to list only the garage.
    #[serde(default)]
    pub include_archived: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArchiveVehicleInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// The day the car was sold, `YYYY-MM-DD`. Omit to keep the stored one.
    pub sold_date: Option<String>,
    /// The sale price in cents. Omit to keep the stored one.
    pub sold_price_cents: Option<i32>,
    /// The odometer reading at the sale. Omit to keep the stored one.
    pub sold_mileage: Option<i32>,
}

impl ArchiveVehicleInput {
    pub fn into_domain(self) -> DomainResult<(i32, VehicleSale)> {
        Ok((
            self.vehicle_id,
            VehicleSale {
                sold_date: dates::parse_optional_date("sold_date", self.sold_date)?,
                sold_price_cents: self.sold_price_cents,
                sold_mileage: self.sold_mileage,
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LogMileageInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Odometer reading (whole miles), zero or more.
    pub mileage: i32,
    /// When the reading was taken, `YYYY-MM-DD HH:MM:SS` or just `YYYY-MM-DD`, no later than today. Defaults to the current local date and time.
    pub recorded_at: Option<String>,
    /// Free-form notes.
    pub notes: Option<String>,
}

impl LogMileageInput {
    pub fn into_domain(self) -> DomainResult<(i32, NewMileageEntry)> {
        Ok((
            self.vehicle_id,
            NewMileageEntry {
                mileage: self.mileage,
                recorded_at: dates::parse_optional_day_or_time("recorded_at", self.recorded_at)?,
                source: None,
                notes: self.notes,
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteMileageInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Reading id, from `list_mileage`.
    pub mileage_id: i32,
}

/// Compact vehicle row for `list_vehicles` — enough to pick a `vehicle_id`
/// without the full record (`get_vehicle` has the rest).
#[derive(Serialize)]
pub struct VehicleBrief {
    pub id: i32,
    pub name: String,
    pub year: Option<i32>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub archived: bool,
}

impl From<&vehicle::Model> for VehicleBrief {
    fn from(v: &vehicle::Model) -> Self {
        VehicleBrief {
            id: v.id,
            name: v.name.clone(),
            year: v.year,
            make: v.make.clone(),
            model: v.model.clone(),
            archived: v.archived_at.is_some(),
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddVehicleInput {
    /// Display name, e.g. 'Daily Golf R'.
    pub name: String,
    /// Model template id from `list_model_templates`. Gives the car its inherited maintenance schedule and fills year, make, model, trim, body style, engine, transmission and drivetrain left empty (what you send wins, then the template, then a VIN decode).
    pub model_template_id: Option<i32>,
    /// VIN. When given, the vehicle's blank year, make, model, trim, body style, engine, transmission and drivetrain are filled from an NHTSA lookup; values sent here and values from the model template win over it. If the lookup fails the vehicle is still created and the result carries `vin_decode_error`.
    pub vin: Option<String>,
    /// Model year.
    pub year: Option<i32>,
    /// Manufacturer, e.g. 'Volkswagen'.
    pub make: Option<String>,
    /// Model, e.g. 'Golf R'.
    pub model: Option<String>,
    /// Trim level.
    pub trim_level: Option<String>,
    /// Body style, e.g. 'Hatchback'.
    pub body_style: Option<String>,
    /// Engine description.
    pub engine: Option<String>,
    /// Transmission description.
    pub transmission: Option<String>,
    /// Drivetrain, e.g. 'AWD'.
    pub drivetrain: Option<String>,
    /// License plate.
    pub license_plate: Option<String>,
    /// Color.
    pub color: Option<String>,
    /// Purchase date, `YYYY-MM-DD`.
    pub purchase_date: Option<String>,
    /// Purchase price, integer cents.
    pub purchase_price_cents: Option<i32>,
    /// Odometer reading at purchase, zero or more.
    pub purchase_mileage: Option<i32>,
    /// Free-form notes.
    pub notes: Option<String>,
    /// Warranty expiry date, `YYYY-MM-DD`.
    pub warranty_expires_on: Option<String>,
    /// Warranty expiry odometer reading.
    pub warranty_expires_miles: Option<i32>,
}

impl AddVehicleInput {
    pub fn into_domain(self) -> DomainResult<NewVehicle> {
        Ok(NewVehicle {
            name: self.name,
            model_template_id: self.model_template_id,
            year: self.year,
            make: self.make,
            model: self.model,
            trim_level: self.trim_level,
            body_style: self.body_style,
            engine: self.engine,
            transmission: self.transmission,
            drivetrain: self.drivetrain,
            vin: self.vin,
            license_plate: self.license_plate,
            color: self.color,
            purchase_date: dates::parse_optional_date("purchase_date", self.purchase_date)?,
            purchase_price_cents: self.purchase_price_cents,
            purchase_mileage: self.purchase_mileage,
            notes: self.notes,
            warranty_expires_on: dates::parse_optional_date(
                "warranty_expires_on",
                self.warranty_expires_on,
            )?,
            warranty_expires_miles: self.warranty_expires_miles,
        })
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateVehicleInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// New display name. Omit to leave it unchanged.
    pub name: Option<String>,
    /// Model template id from `list_model_templates`. Setting one fills the year, make, model, trim, body style, engine, transmission and drivetrain that are empty on the car; fields the car already has are kept.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub model_template_id: Option<Option<i32>>,
    /// Model year.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub year: Option<Option<i32>>,
    /// Manufacturer.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub make: Option<Option<String>>,
    /// Model.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub model: Option<Option<String>>,
    /// Trim level.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub trim_level: Option<Option<String>>,
    /// Body style.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub body_style: Option<Option<String>>,
    /// Engine description.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub engine: Option<Option<String>>,
    /// Transmission description.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub transmission: Option<Option<String>>,
    /// Drivetrain.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub drivetrain: Option<Option<String>>,
    /// VIN. Setting it runs no NHTSA lookup; call `decode_vin` afterwards to decode it.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub vin: Option<Option<String>>,
    /// License plate.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub license_plate: Option<Option<String>>,
    /// Color.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub color: Option<Option<String>>,
    /// Purchase date, `YYYY-MM-DD`.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub purchase_date: Option<Option<String>>,
    /// Purchase price, integer cents.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub purchase_price_cents: Option<Option<i32>>,
    /// Odometer reading at purchase, zero or more.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub purchase_mileage: Option<Option<i32>>,
    /// Sale date, `YYYY-MM-DD`.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub sold_date: Option<Option<String>>,
    /// Sale price, integer cents.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub sold_price_cents: Option<Option<i32>>,
    /// Odometer reading at sale.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub sold_mileage: Option<Option<i32>>,
    /// Free-form notes.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub notes: Option<Option<String>>,
    /// Warranty expiry date, `YYYY-MM-DD`.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub warranty_expires_on: Option<Option<String>>,
    /// Warranty expiry odometer reading.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub warranty_expires_miles: Option<Option<i32>>,
}

impl UpdateVehicleInput {
    pub fn into_domain(self) -> DomainResult<(i32, UpdateVehicle)> {
        Ok((
            self.vehicle_id,
            UpdateVehicle {
                name: self.name,
                model_template_id: self.model_template_id,
                year: self.year,
                make: self.make,
                model: self.model,
                trim_level: self.trim_level,
                body_style: self.body_style,
                engine: self.engine,
                transmission: self.transmission,
                drivetrain: self.drivetrain,
                vin: self.vin,
                license_plate: self.license_plate,
                color: self.color,
                purchase_date: dates::parse_date_change("purchase_date", self.purchase_date)?,
                purchase_price_cents: self.purchase_price_cents,
                purchase_mileage: self.purchase_mileage,
                sold_date: dates::parse_date_change("sold_date", self.sold_date)?,
                sold_price_cents: self.sold_price_cents,
                sold_mileage: self.sold_mileage,
                notes: self.notes,
                warranty_expires_on: dates::parse_date_change(
                    "warranty_expires_on",
                    self.warranty_expires_on,
                )?,
                warranty_expires_miles: self.warranty_expires_miles,
            },
        ))
    }
}
