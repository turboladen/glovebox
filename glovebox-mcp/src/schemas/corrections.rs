//! Inputs for the tools that add, correct, list and delete existing records.
//!
//! A field typed `Option<Option<T>>` distinguishes three cases: an absent key
//! leaves the stored value alone, an explicit JSON `null` clears it, and a
//! value sets it. Doc comments become the field descriptions the model sees.

use glovebox_shared::{
    dates,
    entities::{
        incident::IncidentCategory,
        part::{PartStatus, Supplier},
        service_record::{Payer, Performer},
        work_item::JobStatus,
    },
    error::DomainResult,
    inputs::{
        incident::{NewFollowup, UpdateIncident},
        part::{Fitting, InstallPatch, ReplacePatch, UpdatePart},
        schedule::{NewScheduleItem, UpdateScheduleItem},
        service_record::UpdateServiceRecord,
        vehicle::{NewVehicle, UpdateVehicle},
        work_item::UpdateWorkItem,
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

use super::{LineItemInput, PartContentInput};

/// Deserializes a present key into `Some(value)`, so that `null` becomes
/// `Some(None)` and only an absent key stays `None` (via `serde(default)`).
pub(super) fn nullable<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddVehicleInput {
    /// Display name, e.g. 'Daily Golf R'.
    pub name: String,
    /// Model template id from `list_model_templates`. Gives the car its inherited maintenance schedule and fills year, make, model, trim, body style, engine, transmission and drivetrain left empty (a VIN decode fills first, then the template).
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
    /// Odometer reading at purchase.
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
            purchase_price_currency: None,
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
    /// VIN. Updating it does not re-run the NHTSA lookup.
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
    /// Odometer reading at purchase.
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
                ..Default::default()
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListServicesInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Earliest service date to include, `YYYY-MM-DD`.
    pub from: Option<String>,
    /// Latest service date to include, `YYYY-MM-DD`.
    pub to: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListPartsInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Only parts with this status.
    pub status: Option<PartStatus>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListIncidentsInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// `true` for resolved incidents only, `false` for unresolved only. Omit for all.
    pub resolved: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CorrectServiceInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Service record id, from `list_services` or `summarize_recent_activity`.
    pub service_id: i32,
    /// New service date, `YYYY-MM-DD`.
    pub service_date: Option<String>,
    /// Who did the work. A `self` job takes no shop name.
    pub performed_by: Option<Performer>,
    /// Who paid the invoice overall.
    pub paid_by: Option<Payer>,
    /// REPLACES the schedule items this invoice names with exactly this list (ids from `list_schedule`). The schedule item of a job the service completes counts as well, is listed in a service read, and is never stored from this list; take the job off through `work_item_ids` to remove it, or change which item the job satisfies with `update_planned_work`. A call that takes a job off ignores that job's item in this list; when the invoice still covered the item, name it in a later call. Omit to leave the list alone.
    pub schedule_item_ids: Option<Vec<i32>>,
    /// REPLACES the existing parts installed by this service with exactly this list (ids from `list_parts`). A fitted part taken off the list needs `completed_work`. Omit to leave them alone.
    pub part_ids: Option<Vec<i32>>,
    /// REPLACES every line item with exactly this list. Omit to leave the lines alone.
    pub line_items: Option<Vec<LineItemInput>>,
    /// Last day of a multi-day job, `YYYY-MM-DD`.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub end_date: Option<Option<String>>,
    /// Odometer reading at service time. Keeps the service's own odometer log in step.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub mileage: Option<Option<i32>>,
    /// What was done.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub description: Option<Option<String>>,
    /// Parts portion, integer cents.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub parts_cost_cents: Option<Option<i32>>,
    /// Labor portion, integer cents.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub labor_cost_cents: Option<Option<i32>>,
    /// The invoice's grand total across all lines, integer cents, including lines someone else paid for.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub total_cost_cents: Option<Option<i32>>,
    /// Shop that did the work.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub shop_name: Option<Option<String>>,
    /// Free-form notes.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub notes: Option<Option<String>>,
    /// Who exactly paid / claim number.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub payer_note: Option<Option<String>>,
    /// The number printed on the invoice or repair order. Another service on
    /// this vehicle with the same number is an error that names it.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub invoice_ref: Option<Option<String>>,
    /// REPLACES the recalls this service performed with exactly this list (`finding_id`s from `whats_coming` or `check_recalls`). A recall taken off the list needs `completed_work`. A recall that a job this service completes came from stays performed whether or not this list names it, so take the job off to take that recall off. Send `[]` to remove every recall no completed job brings; omit to leave them alone.
    pub recall_finding_ids: Option<Vec<i32>>,
    /// REPLACES the planned jobs this service completed with exactly this list (ids from `list_planned_work`). A job added must be planned and becomes done, resolving its incident and performing its recall; a job taken off needs `completed_work` and takes its incident (unless the invoice named that incident), recall, the parts fitted to its needs and the reminder baseline of its schedule item with it. Send `[]` to remove them all; omit to leave them alone.
    pub work_item_ids: Option<Vec<i32>>,
    /// REPLACES the incidents this invoice names as fixed with exactly this list (ids from `list_incidents`). Name only what the invoice itself says it fixed: in a service read these are the `resolved_incidents` with `named_by_invoice: true`, and one with `false` is there only because a job filed from it was completed, so do not send it back unless the invoice names it. Each incident the list adds is marked resolved (one already named keeps its flag), and stays resolved by this service when its job is later taken off. To name the incident of a job you are taking off, name it in an earlier call: one sent with the call that takes the job off is ignored and listed in the reply's `incident_names_ignored`. An incident taken off the list that no job of this service still brings and no other service resolves needs `completed_work`. Send `[]` to remove them all; omit to leave them alone.
    pub resolved_incident_ids: Option<Vec<i32>>,
    /// What happened to the work this call takes off the service (jobs left out of `work_item_ids`, fitted parts left out of `part_ids`, recalls left out of `recall_finding_ids`, incidents left out of `resolved_incident_ids` that no job of this service still brings): `did_not_happen` undoes it, so jobs go back to planned, parts read purchased, recalls read open and incidents read unresolved unless the owner resolved them by hand; `done_another_way` keeps it done and only unlinks it from this invoice. Either way a job taken off stops giving its schedule item a baseline from this service, so after `done_another_way` name the item in `schedule_item_ids` on the service that did the work when its reminder should stay cleared. Needed only when the call takes such work off; ask the owner.
    #[serde(default)]
    pub completed_work: Option<glovebox_shared::services::service_record::CompletedWork>,
    /// The appointment this invoice settles (from `list_appointments`). It must be open.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub appointment_id: Option<Option<i32>>,
}

impl CorrectServiceInput {
    pub fn into_domain(self) -> DomainResult<(i32, i32, UpdateServiceRecord)> {
        Ok((
            self.vehicle_id,
            self.service_id,
            UpdateServiceRecord {
                service_date: dates::parse_optional_date("service_date", self.service_date)?,
                performed_by: self.performed_by,
                paid_by: self.paid_by,
                schedule_item_ids: self.schedule_item_ids,
                part_ids: self.part_ids,
                end_date: dates::parse_date_change("end_date", self.end_date)?,
                mileage: self.mileage,
                description: self.description,
                parts_cost_cents: self.parts_cost_cents,
                labor_cost_cents: self.labor_cost_cents,
                total_cost_cents: self.total_cost_cents,
                shop_name: self.shop_name,
                notes: self.notes,
                payer_note: self.payer_note,
                invoice_ref: self.invoice_ref,
                recall_finding_ids: self.recall_finding_ids,
                work_item_ids: self.work_item_ids,
                resolved_incident_ids: self.resolved_incident_ids,
                completed_work: self.completed_work,
                appointment_id: self.appointment_id,
                line_items: self
                    .line_items
                    .map(|items| items.into_iter().map(LineItemInput::into_domain).collect()),
                ..Default::default()
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteServiceInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Service record id, from `list_services` or `summarize_recent_activity`.
    pub service_id: i32,
    /// What happened to the work this service recorded as done: `did_not_happen` or
    /// `done_another_way`. Needed only when the service recorded work as done; ask the owner.
    #[serde(default)]
    pub completed_work: Option<glovebox_shared::services::service_record::CompletedWork>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CorrectPartInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Part id, from `list_parts`.
    pub part_id: i32,
    /// New part name, without a count such as "x4": the work sheet prints
    /// the need's quantity beside it.
    pub name: Option<String>,
    /// `purchased`, `installed` or `replaced`. A return is made on the need,
    /// not here; a returned part can only be set back to `purchased`. With
    /// no status, install details are refused for a purchased part, and
    /// replacement details for any part but a replaced one. `installed` takes
    /// no replacement details, and setting a replaced part to `installed`
    /// needs `replaced_date: null` and `replaced_odometer: null` for the ones
    /// it holds.
    /// Setting an installed or replaced part to `purchased` clears its
    /// install and replacement details. A part with no received date gets its
    /// install date, or today when it has none; a `received_on` date sent in
    /// the same call is used instead. It is refused for a part the shop
    /// supplied (correct the service's `part_ids` or delete the part
    /// instead), when the same call sends install or replacement details, and
    /// when it sends `received_on: null`.
    pub status: Option<PartStatus>,
    /// Who supplied the part.
    pub supplied_by: Option<Supplier>,
    /// Manufacturer/brand.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub manufacturer: Option<Option<String>>,
    /// Manufacturer part number.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub part_number: Option<Option<String>>,
    /// Where it was bought.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub seller: Option<Option<String>>,
    /// Purchase date, `YYYY-MM-DD`.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub purchase_date: Option<Option<String>>,
    /// Cost, integer cents.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub cost_cents: Option<Option<i32>>,
    /// Product/listing URL.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub retailer_url: Option<Option<String>>,
    /// Install date, `YYYY-MM-DD`.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub installed_date: Option<Option<String>>,
    /// Odometer reading at install.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub installed_odometer: Option<Option<i32>>,
    /// Service record (same vehicle) that installed it.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub installed_service_id: Option<Option<i32>>,
    /// Date it was replaced, `YYYY-MM-DD`.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub replaced_date: Option<Option<String>>,
    /// Odometer reading when it was replaced.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub replaced_odometer: Option<Option<i32>>,
    /// Free-form notes. Printed on the appointment's work sheet for the shop, so keep it to what the shop may read.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub notes: Option<Option<String>>,
    /// Where it goes on the car.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub location: Option<Option<String>>,
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
    /// The seller's order or confirmation number.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub order_ref: Option<Option<String>>,
    /// The date the part arrived, `YYYY-MM-DD`. For a part linked to a need,
    /// prefer `mark_need_received`. This corrects the stored date whatever
    /// the part's status, installed and replaced included.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub received_on: Option<Option<String>>,
    /// For a kit: items to add, each with the number printed on its own
    /// packaging. They go after the items the kit already holds, and the
    /// items already there stay as they are. `null` adds nothing.
    pub add_contents: Option<Vec<PartContentInput>>,
    /// For a kit: the `id`s of items to remove, as `contents` in any reply
    /// that shows the kit lists them. Items not named stay. To fix an
    /// item, remove it and add it again; it then prints last. `null` removes
    /// nothing; to empty a kit, name every item.
    pub remove_contents: Option<Vec<i32>>,
}

impl CorrectPartInput {
    pub fn into_domain(self) -> DomainResult<(i32, i32, UpdatePart)> {
        Ok((
            self.vehicle_id,
            self.part_id,
            UpdatePart {
                name: self.name,
                fitting: Fitting::from_fields(
                    self.status,
                    InstallPatch {
                        service_id: self.installed_service_id,
                        date: dates::parse_date_change("installed_date", self.installed_date)?,
                        odometer: self.installed_odometer,
                    },
                    ReplacePatch {
                        date: dates::parse_date_change("replaced_date", self.replaced_date)?,
                        odometer: self.replaced_odometer,
                    },
                )?,
                supplied_by: self.supplied_by,
                manufacturer: self.manufacturer,
                part_number: self.part_number,
                seller: self.seller,
                purchase_date: dates::parse_date_change("purchase_date", self.purchase_date)?,
                cost_cents: self.cost_cents,
                retailer_url: self.retailer_url,
                notes: self.notes,
                location: self.location,
                warranty_expires_on: dates::parse_date_change(
                    "warranty_expires_on",
                    self.warranty_expires_on,
                )?,
                warranty_expires_miles: self.warranty_expires_miles,
                order_ref: self.order_ref,
                received_on: dates::parse_date_change("received_on", self.received_on)?,
                add_contents: super::kit_contents(self.add_contents),
                remove_contents: self.remove_contents.unwrap_or_default(),
                ..Default::default()
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeletePartInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Part id, from `list_parts`.
    pub part_id: i32,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateIncidentInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Incident id, from `list_incidents` or `log_incident`.
    pub incident_id: i32,
    /// New category.
    pub category: Option<IncidentCategory>,
    /// New title.
    pub title: Option<String>,
    /// When it happened, `YYYY-MM-DD HH:MM:SS` or just `YYYY-MM-DD`.
    pub occurred_at: Option<String>,
    /// Mark resolved or unresolved by hand, e.g. fixed with no invoice. Resolving by hand is remembered (`resolved_by_hand`), and undoing a service's work later leaves such an incident resolved. Omit to leave the flag to the service links.
    pub resolved: Option<bool>,
    /// REPLACES the services that fixed it with exactly this list. A non-empty list marks the incident resolved; an empty list removes every fixing service and marks it unresolved, unless the owner resolved it by hand. A service that completed a job filed from the incident stays whatever the list says, and naming it here changes nothing, so the list alone neither resolves nor reopens the incident while only such services are on it; send `resolved`, or take the job off with `correct_service`. A `resolved` sent in the same call wins. Omit to leave the links alone.
    pub resolved_by_service_ids: Option<Vec<i32>>,
    /// Longer description.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub description: Option<Option<String>>,
    /// Odometer reading when it happened.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub odometer: Option<Option<i32>>,
    /// OBD-II codes.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub obd_codes: Option<Option<String>>,
    /// Free-form notes.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub notes: Option<Option<String>>,
    /// Who was at fault (accidents).
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub fault: Option<Option<String>>,
    /// Other party's name (accidents).
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub other_party_name: Option<Option<String>>,
    /// Other party's phone.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub other_party_phone: Option<Option<String>>,
    /// Other party's email.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub other_party_email: Option<Option<String>>,
    /// Other party's insurer.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub other_party_insurance: Option<Option<String>>,
    /// Other party's policy number.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub other_party_policy_number: Option<Option<String>>,
    /// Your insurance claim number.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub insurance_claim_number: Option<Option<String>>,
    /// Adjuster's name.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub insurance_adjuster: Option<Option<String>>,
    /// Adjuster's phone.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub insurance_adjuster_phone: Option<Option<String>>,
    /// Total repair cost, integer cents.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub total_repair_cost_cents: Option<Option<i32>>,
    /// Deductible, integer cents.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub deductible_cents: Option<Option<i32>>,
    /// Insurance payout, integer cents.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub insurance_payout_cents: Option<Option<i32>>,
    /// Earlier incident (same vehicle) this one is a recurrence of.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub recurrence_of_id: Option<Option<i32>>,
    /// The service record (same vehicle) during which this was noticed.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub noticed_during_service_id: Option<Option<i32>>,
}

impl UpdateIncidentInput {
    pub fn into_domain(self) -> DomainResult<(i32, i32, UpdateIncident)> {
        Ok((
            self.vehicle_id,
            self.incident_id,
            UpdateIncident {
                category: self.category,
                title: self.title,
                occurred_at: dates::parse_optional_day_or_time("occurred_at", self.occurred_at)?,
                resolved: self.resolved,
                resolved_by_service_ids: self.resolved_by_service_ids,
                description: self.description,
                odometer: self.odometer,
                obd_codes: self.obd_codes,
                notes: self.notes,
                fault: self.fault,
                other_party_name: self.other_party_name,
                other_party_phone: self.other_party_phone,
                other_party_email: self.other_party_email,
                other_party_insurance: self.other_party_insurance,
                other_party_policy_number: self.other_party_policy_number,
                insurance_claim_number: self.insurance_claim_number,
                insurance_adjuster: self.insurance_adjuster,
                insurance_adjuster_phone: self.insurance_adjuster_phone,
                total_repair_cost_cents: self.total_repair_cost_cents,
                deductible_cents: self.deductible_cents,
                insurance_payout_cents: self.insurance_payout_cents,
                recurrence_of_id: self.recurrence_of_id,
                noticed_during_service_id: self.noticed_during_service_id,
                ..Default::default()
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddIncidentFollowupInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Incident id, from `list_incidents` or `log_incident`.
    pub incident_id: i32,
    /// When the contact happened, `YYYY-MM-DD HH:MM:SS` or just `YYYY-MM-DD`.
    pub occurred_at: String,
    /// What happened or was said, e.g. 'Adjuster approved the estimate'.
    pub summary: String,
    /// How, e.g. `phone`, `email`.
    pub contact_method: Option<String>,
    /// Who with.
    pub contact_with: Option<String>,
    /// Free-form notes.
    pub notes: Option<String>,
}

impl AddIncidentFollowupInput {
    pub fn into_domain(self) -> DomainResult<(i32, i32, NewFollowup)> {
        Ok((
            self.vehicle_id,
            self.incident_id,
            NewFollowup {
                occurred_at: dates::parse_day_or_time("occurred_at", &self.occurred_at)?,
                contact_method: self.contact_method,
                contact_with: self.contact_with,
                summary: self.summary,
                notes: self.notes,
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteIncidentInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Incident id, from `list_incidents` or `log_incident`.
    pub incident_id: i32,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePlannedWorkInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Planned-work id, from `list_planned_work`.
    pub work_item_id: i32,
    /// New title.
    pub title: Option<String>,
    /// `planned`, `done` or `dropped`. Use `done` only for work that left no paperwork (otherwise `record_service` with `work_item_ids`), `dropped` for work the owner has decided against, and `planned` to reopen an item.
    pub status: Option<JobStatus>,
    /// Free-form notes. Printed on the appointment's work sheet for the shop, so keep it to what the shop may read.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub notes: Option<Option<String>>,
    /// Schedule item this satisfies.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub schedule_item_id: Option<Option<i32>>,
    /// Research finding (e.g. recall) this addresses.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub research_finding_id: Option<Option<i32>>,
    /// Incident this fixes.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub incident_id: Option<Option<i32>>,
    /// Estimated cost, integer cents.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub est_cost_cents: Option<Option<i32>>,
    /// Date to look at it again, `YYYY-MM-DD`.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub revisit_by_date: Option<Option<String>>,
    /// Odometer reading at which to look at it again.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub revisit_by_miles: Option<Option<i32>>,
    /// The appointment this job is on (from `list_appointments`). It must be
    /// open.
    ///
    /// Send `null` to take the job off its appointment; omit the key to leave
    /// it unchanged. Its needs, options and reasons stay with the job.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub appointment_id: Option<Option<i32>>,
    /// Who gave the quote this job came from.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub quoted_by: Option<Option<String>>,
    /// The day of that quote, `YYYY-MM-DD`.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub quoted_on: Option<Option<String>>,
    /// The service whose visit produced the quote (from `list_services`).
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub quote_service_id: Option<Option<i32>>,
}

impl UpdatePlannedWorkInput {
    pub fn into_domain(self) -> DomainResult<(i32, i32, UpdateWorkItem)> {
        Ok((
            self.vehicle_id,
            self.work_item_id,
            UpdateWorkItem {
                title: self.title,
                status: self.status,
                notes: self.notes,
                schedule_item_id: self.schedule_item_id,
                research_finding_id: self.research_finding_id,
                incident_id: self.incident_id,
                est_cost_cents: self.est_cost_cents,
                revisit_by_date: dates::parse_date_change("revisit_by_date", self.revisit_by_date)?,
                revisit_by_miles: self.revisit_by_miles,
                appointment_id: self.appointment_id,
                quoted_by: self.quoted_by,
                quoted_on: dates::parse_date_change("quoted_on", self.quoted_on)?,
                quote_service_id: self.quote_service_id,
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeletePlannedWorkInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Planned-work id, from `list_planned_work`.
    pub work_item_id: i32,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddScheduleItemInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Item name, e.g. 'Smog check'.
    pub name: String,
    /// Whether the manufacturer recommends it.
    pub is_factory_recommended: Option<bool>,
    /// What the item covers.
    pub description: Option<String>,
    /// Repeat every this many miles. Must be greater than zero.
    pub interval_miles: Option<i32>,
    /// Repeat every this many months. Must be greater than zero.
    pub interval_months: Option<i32>,
    /// Start warning this many miles before due. Must be zero or more. Defaults to 1000.
    pub warning_miles: Option<i32>,
    /// Start warning this many days before due. Must be zero or more. Defaults to 30.
    pub warning_days: Option<i32>,
    /// Where the interval comes from, e.g. 'owner manual'.
    pub source: Option<String>,
    /// Free-form notes.
    pub notes: Option<String>,
    /// Estimated cost per occurrence, integer cents.
    pub est_cost_cents: Option<i32>,
}

impl AddScheduleItemInput {
    pub fn into_domain(self) -> (i32, NewScheduleItem) {
        (
            self.vehicle_id,
            NewScheduleItem {
                name: self.name,
                is_factory_recommended: self.is_factory_recommended,
                description: self.description,
                interval_miles: self.interval_miles,
                interval_months: self.interval_months,
                warning_miles: self.warning_miles,
                warning_days: self.warning_days,
                source: self.source,
                notes: self.notes,
                est_cost_cents: self.est_cost_cents,
                ..Default::default()
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateScheduleItemInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Schedule item id, from `list_schedule` or `whats_coming`. An inherited
    /// item's id and the id of this car's copy of it reach the same item.
    pub schedule_item_id: i32,
    /// New name. Only an item added with `add_schedule_item` can be renamed;
    /// an inherited item keeps its name.
    pub name: Option<String>,
    /// `false` stops the item appearing in `whats_coming`.
    pub enabled: Option<bool>,
    /// Whether the manufacturer recommends it.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<bool>")]
    pub is_factory_recommended: Option<Option<bool>>,
    /// What the item covers.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub description: Option<Option<String>>,
    /// Repeat every this many miles. Must be greater than zero.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub interval_miles: Option<Option<i32>>,
    /// Repeat every this many months. Must be greater than zero.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub interval_months: Option<Option<i32>>,
    /// Start warning this many miles before due. Must be zero or more.
    ///
    /// Send `null` to restore the default of 1000; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub warning_miles: Option<Option<i32>>,
    /// Start warning this many days before due. Must be zero or more.
    ///
    /// Send `null` to restore the default of 30; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub warning_days: Option<Option<i32>>,
    /// Where the interval comes from, e.g. 'owner manual'.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub source: Option<Option<String>>,
    /// Free-form notes.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub notes: Option<Option<String>>,
    /// Estimated cost per occurrence, integer cents.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub est_cost_cents: Option<Option<i32>>,
}

impl UpdateScheduleItemInput {
    pub fn into_domain(self) -> (i32, i32, UpdateScheduleItem) {
        (
            self.vehicle_id,
            self.schedule_item_id,
            UpdateScheduleItem {
                name: self.name,
                enabled: self.enabled,
                is_factory_recommended: self.is_factory_recommended,
                description: self.description,
                interval_miles: self.interval_miles,
                interval_months: self.interval_months,
                warning_miles: self.warning_miles,
                warning_days: self.warning_days,
                source: self.source,
                notes: self.notes,
                est_cost_cents: self.est_cost_cents,
                ..Default::default()
            },
        )
    }
}
