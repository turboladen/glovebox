//! Tool input/output shapes for the MCP surface.
//!
//! Inputs derive `Deserialize` + `JsonSchema`; doc comments become field
//! descriptions in the advertised schema, so write them for an LLM. Each
//! input maps 1:1 onto a `glovebox_shared::inputs` struct (fields the MCP
//! surface doesn't expose are filled with `None`) — no business logic here.

use glovebox_shared::{
    entities::vehicle,
    inputs::{
        mileage::NewMileageEntry,
        observation::NewObservation,
        service_record::{NewLineItem, NewServiceRecord},
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, JsonSchema)]
pub struct EmptyParams {}

#[derive(Deserialize, JsonSchema)]
pub struct VehicleParams {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
}

#[derive(Deserialize, JsonSchema)]
pub struct BuildParams {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Build id, from `list_builds`.
    pub build_id: i32,
}

#[derive(Deserialize, JsonSchema)]
pub struct RecordServiceInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Date the work was done, `YYYY-MM-DD`.
    pub service_date: String,
    /// What was done, e.g. "Oil change + tire rotation".
    pub description: Option<String>,
    /// Odometer reading at service time. Also logs a mileage entry.
    pub mileage: Option<i32>,
    /// Total invoice amount in integer cents (e.g. $123.45 -> 12345).
    pub total_cost_cents: Option<i32>,
    /// Parts portion of the invoice, integer cents.
    pub parts_cost_cents: Option<i32>,
    /// Labor portion of the invoice, integer cents.
    pub labor_cost_cents: Option<i32>,
    /// Shop that did the work (free text). Omit for DIY.
    pub shop_name: Option<String>,
    /// Free-form notes.
    pub notes: Option<String>,
    /// Itemized invoice lines.
    pub line_items: Option<Vec<LineItemInput>>,
    /// Link this service to a build (from `list_builds`).
    pub build_id: Option<i32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct LineItemInput {
    /// Line description, e.g. "5W-30 synthetic oil, 5qt".
    pub description: String,
    /// Category, e.g. "parts", "labor", "fluids", "fees".
    pub category: Option<String>,
    pub quantity: Option<f64>,
    /// Per-unit cost, integer cents.
    pub unit_cost_cents: Option<i32>,
    /// Line total, integer cents.
    pub cost_cents: Option<i32>,
}

impl RecordServiceInput {
    pub fn into_domain(self) -> (i32, NewServiceRecord) {
        let line_items = self.line_items.map(|items| {
            items
                .into_iter()
                .map(|li| NewLineItem {
                    description: li.description,
                    category: li.category,
                    quantity: li.quantity,
                    unit_cost_cents: li.unit_cost_cents,
                    cost_cents: li.cost_cents,
                })
                .collect()
        });
        (
            self.vehicle_id,
            NewServiceRecord {
                service_date: self.service_date,
                mileage: self.mileage,
                description: self.description,
                parts_cost_cents: self.parts_cost_cents,
                parts_cost_currency: None,
                labor_cost_cents: self.labor_cost_cents,
                labor_cost_currency: None,
                total_cost_cents: self.total_cost_cents,
                total_cost_currency: None,
                shop_name: self.shop_name,
                shop_id: None,
                notes: self.notes,
                build_id: self.build_id,
                schedule_item_ids: None,
                part_ids: None,
                line_items,
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct LogObservationInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Short title, e.g. "Squeak from front left on braking".
    pub title: String,
    /// Longer description of what was noticed.
    pub description: Option<String>,
    /// Category, e.g. "noise", "leak", "`warning_light`", "note". Defaults to "note".
    pub category: Option<String>,
    /// Odometer reading when observed.
    pub odometer: Option<i32>,
    /// When it was observed, `YYYY-MM-DD HH:MM:SS`. Defaults to now.
    pub observed_at: Option<String>,
    /// OBD-II codes if a scanner was involved, e.g. "P0301,P0420".
    pub obd_codes: Option<String>,
    /// Link this observation to a build (from `list_builds`).
    pub build_id: Option<i32>,
}

impl LogObservationInput {
    pub fn into_domain(self) -> (i32, NewObservation) {
        (
            self.vehicle_id,
            NewObservation {
                category: self.category.unwrap_or_else(|| "note".to_string()),
                title: self.title,
                description: self.description,
                odometer: self.odometer,
                observed_at: self.observed_at,
                obd_codes: self.obd_codes,
                notes: None,
                build_id: self.build_id,
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct LogMileageInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Odometer reading (whole miles).
    pub mileage: i32,
    /// When the reading was taken, `YYYY-MM-DD HH:MM:SS`. Defaults to now.
    pub recorded_at: Option<String>,
    /// Free-form notes.
    pub notes: Option<String>,
}

impl LogMileageInput {
    pub fn into_domain(self) -> (i32, NewMileageEntry) {
        (
            self.vehicle_id,
            NewMileageEntry {
                mileage: self.mileage,
                recorded_at: self.recorded_at,
                source: None,
                notes: self.notes,
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct SummarizeActivityInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Max items to return (default 20).
    pub limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct FindDocumentsInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Full-text query over document titles and extracted text (receipts,
    /// manuals, photos with OCR). Plain words; no special syntax needed.
    pub query: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct SearchRecordsInput {
    /// Full-text query. Plain words; no special syntax needed.
    pub query: String,
    /// Restrict to one record kind: `all`, `vehicles`, `services`,
    /// `observations`, `accidents`, `documents`, or `research`. Default `all`.
    pub scope: Option<String>,
    /// Restrict to one vehicle (from `list_vehicles`). Omit to search the garage.
    pub vehicle_id: Option<i32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct UpdateBuildStatusInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Build id, from `list_builds`.
    pub build_id: i32,
    /// New lifecycle status: `planned`, `active`, `on_hold`, `completed`,
    /// or `abandoned`. Entering `completed` stamps the completion date.
    pub status: String,
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
