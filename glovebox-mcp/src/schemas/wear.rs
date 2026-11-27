//! Inputs for the wear reading tools.
//!
//! A field typed `Option<Option<T>>` follows the convention in `schemas`: an
//! absent key leaves the value alone, `null` clears it, and a value sets it.

use glovebox_shared::{
    dates,
    entities::wear_reading::{Axle, WearComponent},
    error::DomainResult,
    inputs::wear::{NewWearReading, UpdateWearReading, WearFilter},
};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{MeasureInput, nullable};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordWearReadingInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// The service that took the reading, from `list_services`. The reading takes its date and mileage for each one left out here. Readings on a new invoice go in `record_service`'s `wear_readings` instead.
    pub service_id: Option<i32>,
    /// `brake_pads`, `brake_rotors` or `tire_tread`.
    pub component: WearComponent,
    /// `front` or `rear`.
    pub axle: Axle,
    /// The measurement as written, e.g. `{"value": 4, "unit": "mm"}` or `{"value": 7, "unit": "in_32"}`. A percentage is not a length: leave it in the notes.
    pub reading: MeasureInput,
    /// The minimum the shop stated beside it, in its own unit. Omit when none was stated.
    pub limit: Option<MeasureInput>,
    /// The day it was measured, `YYYY-MM-DD`. Needed when no `service_id` is given.
    pub measured_on: Option<String>,
    /// The odometer reading when it was measured.
    pub odometer: Option<i32>,
    /// Free text, e.g. "caution".
    pub notes: Option<String>,
}

impl RecordWearReadingInput {
    pub fn into_domain(self) -> DomainResult<(i32, NewWearReading)> {
        Ok((
            self.vehicle_id,
            NewWearReading {
                component: self.component,
                axle: self.axle,
                reading: self.reading.into(),
                limit: self.limit.map(Into::into),
                measured_on: dates::parse_optional_date("measured_on", self.measured_on)?,
                odometer: self.odometer,
                service_id: self.service_id,
                notes: self.notes,
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListWearReadingsInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Only readings of this component.
    pub component: Option<WearComponent>,
    /// Only readings taken on this axle.
    pub axle: Option<Axle>,
    /// Only the readings this service took.
    pub service_id: Option<i32>,
    /// The most readings to return, counted from the newest after the other filters. At least 1; 0 is an error. Omit for all.
    pub limit: Option<u32>,
}

impl ListWearReadingsInput {
    pub fn into_domain(self) -> (i32, WearFilter) {
        (
            self.vehicle_id,
            WearFilter {
                component: self.component,
                axle: self.axle,
                service_id: self.service_id,
                limit: self.limit,
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CorrectWearReadingInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Wear reading id, from `list_wear_readings`.
    pub wear_reading_id: i32,
    pub component: Option<WearComponent>,
    pub axle: Option<Axle>,
    /// The measurement as written.
    pub reading: Option<MeasureInput>,
    /// The minimum the shop stated. Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<MeasureInput>")]
    pub limit: Option<Option<MeasureInput>>,
    /// The day it was measured, `YYYY-MM-DD`. It cannot be cleared.
    pub measured_on: Option<String>,
    /// The odometer reading. Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub odometer: Option<Option<i32>>,
    /// The service that took it. Send `null` to unlink it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub service_id: Option<Option<i32>>,
    /// Notes. This REPLACES the stored notes: to add to them, send the stored text plus the new. Send `null` to clear them; omit the key to leave them unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub notes: Option<Option<String>>,
}

impl CorrectWearReadingInput {
    pub fn into_domain(self) -> DomainResult<(i32, i32, UpdateWearReading)> {
        Ok((
            self.vehicle_id,
            self.wear_reading_id,
            UpdateWearReading {
                component: self.component,
                axle: self.axle,
                reading: self.reading.map(Into::into),
                limit: self.limit.map(|l| l.map(Into::into)),
                measured_on: dates::parse_optional_date("measured_on", self.measured_on)?,
                odometer: self.odometer,
                service_id: self.service_id,
                notes: self.notes,
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteWearReadingInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Wear reading id, from `list_wear_readings`.
    pub wear_reading_id: i32,
}
