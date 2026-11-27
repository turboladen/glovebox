//! Inputs for the part tools.

use glovebox_shared::{
    dates,
    entities::part::{PartStatus, Supplier},
    error::DomainResult,
    inputs::part::{Fitting, Install, InstallPatch, NewFitting, NewPart, ReplacePatch, UpdatePart},
};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{PartContentInput, WearTagInput, kit_contents, nullable};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordPartInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Part name, e.g. "Sachs SRE clutch kit". Leave the count out of it,
    /// such as "x4": the work sheet prints the need's quantity beside the name.
    pub name: String,
    /// Manufacturer/brand, e.g. "Sachs".
    pub manufacturer: Option<String>,
    /// Manufacturer part number.
    pub part_number: Option<String>,
    /// Where it was bought, e.g. "FCP Euro".
    pub seller: Option<String>,
    /// Purchase date, `YYYY-MM-DD`.
    pub purchase_date: Option<String>,
    /// What it cost, integer cents (e.g. $123.45 -> 12345).
    pub cost_cents: Option<i32>,
    /// Who supplied the part: `self` (the owner bought it, the default) or
    /// `shop` (the shop sourced it and billed for it).
    pub supplied_by: Option<Supplier>,
    /// Lifecycle status: `purchased`, `installed` or `replaced`. Defaults to
    /// `purchased`. A return is made on the need, not here. Install details
    /// need `installed` or `replaced`: a purchased part takes none. Put
    /// instructions for a later install, such as a torque value, in `notes`.
    pub status: Option<PartStatus>,
    /// Where it goes on the car (free text), e.g. "Front brakes".
    pub location: Option<String>,
    /// Date it was installed, `YYYY-MM-DD`.
    pub installed_date: Option<String>,
    /// Odometer reading at install time.
    pub installed_odometer: Option<i32>,
    /// Link to the service record that installed it.
    pub installed_service_id: Option<i32>,
    /// Product/listing URL for the part.
    pub retailer_url: Option<String>,
    /// Free-form notes. Printed on the appointment's work sheet for the shop, so keep it to what the shop may read.
    pub notes: Option<String>,
    /// Part warranty expiry date, `YYYY-MM-DD` (if the part carries one).
    pub warranty_expires_on: Option<String>,
    /// Part warranty expiry odometer reading (whole miles).
    pub warranty_expires_miles: Option<i32>,
    /// The seller's order or confirmation number.
    pub order_ref: Option<String>,
    /// For a kit: each item inside it, with the number printed on that item's
    /// own packaging, in the order the work sheet should list them. Omit for a
    /// part that is not a kit.
    pub contents: Option<Vec<PartContentInput>>,
    /// For new brake pads, brake rotors or tires: what the part replaces, so the wear readings start a new series from its install. Omit for any other part.
    pub wears_as: Option<WearTagInput>,
}

impl RecordPartInput {
    pub fn into_domain(self) -> DomainResult<(i32, NewPart)> {
        Ok((
            self.vehicle_id,
            NewPart {
                name: self.name,
                manufacturer: self.manufacturer,
                part_number: self.part_number,
                oe_part_number_replaced: None,
                seller: self.seller,
                purchase_date: dates::parse_optional_date("purchase_date", self.purchase_date)?,
                cost_cents: self.cost_cents,
                manufacturer_url: None,
                retailer_url: self.retailer_url,
                fitting: NewFitting::from_fields(
                    self.status,
                    Install {
                        service_id: self.installed_service_id,
                        date: dates::parse_optional_date("installed_date", self.installed_date)?,
                        odometer: self.installed_odometer,
                    },
                )?,
                notes: self.notes,
                location: self.location,
                warranty_expires_on: dates::parse_optional_date(
                    "warranty_expires_on",
                    self.warranty_expires_on,
                )?,
                warranty_expires_miles: self.warranty_expires_miles,
                supplied_by: self.supplied_by,
                order_ref: self.order_ref,
                need_id: None,
                contents: kit_contents(self.contents),
                wears_as: self.wears_as.map(WearTagInput::into_domain).transpose()?,
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListPartsInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Only parts with this status.
    pub status: Option<PartStatus>,
    /// `true` for the parts still under warranty: purchased or installed, with a warranty expiry date or expiry odometer reading, and no bound passed (a part with both is under warranty only while neither has passed). `false` for every other part. Omit for all.
    pub under_warranty: Option<bool>,
    /// The most parts to return, counted from the most recently recorded after the other filters. At least 1; 0 is an error. Omit for all.
    pub limit: Option<u32>,
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
    /// For brake pads, brake rotors or tires: what the part replaces, so the wear readings start a new series from its install. Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<WearTagInput>")]
    pub wears_as: Option<Option<WearTagInput>>,
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
                wears_as: self
                    .wears_as
                    .map(|t| t.map(WearTagInput::into_domain).transpose())
                    .transpose()?,
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
