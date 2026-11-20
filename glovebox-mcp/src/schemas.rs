//! Tool input/output shapes for the MCP surface.
//!
//! Inputs derive `Deserialize` + `JsonSchema`; doc comments become field
//! descriptions in the advertised schema, so write them for an LLM. Each
//! input maps 1:1 onto a `glovebox_shared::inputs` struct (fields the MCP
//! surface doesn't expose are filled with `None`) — no business logic here.

use glovebox_shared::{
    dates,
    entities::{
        incident::IncidentCategory,
        part::{PartStatus, Supplier},
        service_record::{Payer, Performer},
        service_record_line_item::WorkKind,
        vehicle,
    },
    error::{DomainError, DomainResult},
    inputs::{
        incident::NewIncident,
        mileage::NewMileageEntry,
        part::{Install, NewFitting, NewPart, NewPartContent},
        service_record::{NewLineItem, NewServiceRecord},
        shop::ShopChoice,
        vehicle::VehicleSale,
        work_item::NewWorkItem,
    },
    services::{recall::RecallStatus, research::NewFiledFinding},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// The correct and update inputs use `Option<Option<T>>` to tell an absent key
// from an explicit `null`.
#[allow(clippy::option_option)]
mod corrections;

pub use corrections::*;

#[allow(clippy::option_option)]
mod checklist;

pub use checklist::*;

#[allow(clippy::option_option)]
mod shops;

pub use shops::*;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyParams {}

const BOTH_SHOP_FORMS: &str = "Name the shop by shop_id or by shop_name, not both";

/// The shop a new record names, from its two arguments.
pub(crate) fn shop_choice(
    shop_id: Option<i32>,
    shop_name: Option<String>,
) -> DomainResult<Option<ShopChoice>> {
    match (shop_id, shop_name) {
        (Some(_), Some(_)) => Err(DomainError::BadRequest(BOTH_SHOP_FORMS.into())),
        (Some(id), None) => Ok(Some(ShopChoice::Id(id))),
        (None, name) => Ok(name.map(ShopChoice::Name)),
    }
}

/// The change to a record's shop, from its two arguments: `null` clears it,
/// and a value in one beside anything in the other is refused, unless both
/// are `null`.
#[expect(
    clippy::option_option,
    reason = "an absent change differs from clearing"
)]
pub(crate) fn shop_change(
    shop_id: Option<Option<i32>>,
    shop_name: Option<Option<String>>,
) -> DomainResult<Option<Option<ShopChoice>>> {
    match (shop_id, shop_name) {
        (None, None) => Ok(None),
        (Some(None), None | Some(None)) | (None, Some(None)) => Ok(Some(None)),
        (Some(Some(id)), None) => Ok(Some(Some(ShopChoice::Id(id)))),
        (None, Some(Some(name))) => Ok(Some(Some(ShopChoice::Name(name)))),
        (Some(_), Some(_)) => Err(DomainError::BadRequest(BOTH_SHOP_FORMS.into())),
    }
}

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
pub struct VehicleParams {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordServiceInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Date the work was done (the first day, for multi-day jobs), `YYYY-MM-DD`.
    pub service_date: String,
    /// Last day of the work for a multi-day job, `YYYY-MM-DD`. Not before
    /// `service_date`. Omit for same-day work.
    pub end_date: Option<String>,
    /// Who did the work: `shop` (default) or `self` (the owner did it, e.g. a
    /// DIY job or an ECU flash). A `self` job takes no `shop_id` or `shop_name`.
    pub performed_by: Option<Performer>,
    /// What was done, e.g. "Oil change + tire rotation".
    pub description: Option<String>,
    /// Odometer reading at service time. Also logs a mileage entry.
    pub mileage: Option<i32>,
    /// The invoice's grand total across all lines, in integer cents (e.g.
    /// $123.45 -> 12345), including lines someone else paid for. What the
    /// owner paid is derived as this total minus the covered lines. It
    /// excludes the cost of any part with `supplied_by: "self"`, which was
    /// paid for elsewhere.
    pub total_cost_cents: Option<i32>,
    /// Parts subtotal, integer cents. Fill it only when the invoice prints one.
    pub parts_cost_cents: Option<i32>,
    /// Labor subtotal, integer cents. Fill it only when the invoice prints one.
    pub labor_cost_cents: Option<i32>,
    /// The shop that did the work, by its id from `list_shops`. Omit for DIY.
    pub shop_id: Option<i32>,
    /// The shop by name instead of `shop_id`. It must match one shop from
    /// `list_shops` (case and surrounding spaces aside); a name that matches
    /// none is refused, so add a new shop with `add_shop` first.
    pub shop_name: Option<String>,
    /// Free-form notes.
    pub notes: Option<String>,
    /// Who paid: `self` (default), `insurance`, `third_party` (e.g. the
    /// other driver), or `manufacturer` (the manufacturer or dealer covered
    /// the work, as with a recall or warranty repair). Anything not paid by
    /// the owner counts as covered, not out-of-pocket, in cost summaries.
    pub paid_by: Option<Payer>,
    /// Who exactly paid / claim number, e.g. "Progressive claim #12345".
    pub payer_note: Option<String>,
    /// Itemized invoice lines, one per job on the paperwork.
    pub line_items: Option<Vec<LineItemInput>>,
    /// Parts this invoice lists that are not recorded yet. Each is created
    /// already installed by this service, at its mileage and date, so no
    /// separate `record_part` call is needed.
    pub parts: Option<Vec<InlinePartInput>>,
    /// Ids of parts already recorded for this vehicle (from `record_part` or
    /// `list_parts`) that this service installed. Each is marked installed
    /// at this service's date and mileage. Do not list such a part again
    /// under `parts`.
    pub part_ids: Option<Vec<i32>>,
    /// Ids of schedule items (from `whats_coming`) this work satisfies, every
    /// one of them and not only the one the owner mentioned. Linking clears
    /// the reminder and restarts its interval. A job in `work_item_ids`
    /// brings its own schedule item, so that item need not be named here.
    pub schedule_item_ids: Option<Vec<i32>>,
    /// The number printed on the invoice or repair order (e.g. "RO-1182").
    /// Re-recording the SAME ref for this vehicle returns the existing record
    /// (with `already_recorded: true` and a `message` saying nothing was
    /// applied) and ignores the lines and parts sent this time; they are not
    /// merged. Omit for work with no paperwork number.
    pub invoice_ref: Option<String>,
    /// Ids of open to-do items (from `list_planned_work`) this service
    /// completes. Each is marked done, and its schedule item, if any, is
    /// linked so the reminder clears.
    pub work_item_ids: Option<Vec<i32>>,
    /// Ids of incidents (from `log_incident`) the invoice says this service
    /// fixed. Each is marked resolved and linked to this service, and stays
    /// so if a job filed from it is later taken off the service. A job in
    /// `work_item_ids` resolves its incident without being named here.
    pub resolved_incident_ids: Option<Vec<i32>>,
    /// Ids of incidents first noticed during this service, e.g. a shop
    /// observation logged beforehand. An incident is noticed during one
    /// service only.
    pub noticed_incident_ids: Option<Vec<i32>>,
    /// Ids of recalls (`finding_id`s from `whats_coming` or `check_recalls`)
    /// this service performed. Each
    /// recall then reads as done, with this service as the one that did it.
    /// Name a recall here when the invoice cites its campaign number.
    pub recall_finding_ids: Option<Vec<i32>>,
    /// The appointment this invoice is for (from `list_appointments`). Pass it
    /// when the invoice settles a planned appointment. Naming it installs and
    /// completes nothing by itself: name the finished jobs in `work_item_ids`
    /// and the parts the shop fitted in `part_ids` or `parts`. The result's
    /// `unfitted_needs` lists every need on a completed job whose part is
    /// linked but neither installed nor replaced; ask the owner about each
    /// one.
    pub appointment_id: Option<i32>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineItemInput {
    /// Line description, e.g. "5W-30 synthetic oil, 5qt".
    pub description: String,
    /// What kind of work this line is: `maintenance`, `mod`, `repair`,
    /// `inspection`, `recall` (work done under a recall campaign), or `fee`
    /// (tax, shop supplies, disposal and similar charges on the invoice that
    /// are not work).
    pub work_kind: WorkKind,
    /// Who paid this line. Leave it out unless this line was paid by someone
    /// other than whoever paid the service. Mark a warranty or recall line on
    /// an owner-paid invoice `manufacturer`; the line's cost still counts
    /// inside the service's `total_cost_cents`, and the owner's share is the
    /// total minus the covered lines.
    pub paid_by: Option<Payer>,
    /// How many units, e.g. 5 for five quarts of oil. May be fractional.
    pub quantity: Option<f64>,
    /// Per-unit cost, integer cents.
    pub unit_cost_cents: Option<i32>,
    /// Line total, integer cents. For a line someone else paid, the value the
    /// invoice shows for that work; leave it out or send 0 when the invoice
    /// shows none.
    pub cost_cents: Option<i32>,
}

impl LineItemInput {
    pub fn into_domain(self) -> NewLineItem {
        NewLineItem {
            description: self.description,
            work_kind: self.work_kind,
            paid_by: self.paid_by,
            quantity: self.quantity,
            unit_cost_cents: self.unit_cost_cents,
            cost_cents: self.cost_cents,
        }
    }
}

impl RecordServiceInput {
    pub fn into_domain(self) -> DomainResult<(i32, NewServiceRecord)> {
        let line_items = self
            .line_items
            .map(|items| items.into_iter().map(LineItemInput::into_domain).collect());
        let parts = self
            .parts
            .map(|ps| ps.into_iter().map(InlinePartInput::into_domain).collect())
            .transpose()?;
        Ok((
            self.vehicle_id,
            NewServiceRecord {
                service_date: dates::parse_date("service_date", &self.service_date)?,
                mileage: self.mileage,
                description: self.description,
                parts_cost_cents: self.parts_cost_cents,
                parts_cost_currency: None,
                labor_cost_cents: self.labor_cost_cents,
                labor_cost_currency: None,
                total_cost_cents: self.total_cost_cents,
                total_cost_currency: None,
                shop: shop_choice(self.shop_id, self.shop_name)?,
                notes: self.notes,
                paid_by: self.paid_by,
                payer_note: self.payer_note,
                schedule_item_ids: self.schedule_item_ids,
                part_ids: self.part_ids,
                work_item_ids: self.work_item_ids,
                resolved_incident_ids: self.resolved_incident_ids,
                noticed_incident_ids: self.noticed_incident_ids,
                recall_finding_ids: self.recall_finding_ids,
                line_items,
                invoice_ref: self.invoice_ref,
                end_date: dates::parse_optional_date("end_date", self.end_date)?,
                performed_by: self.performed_by,
                parts,
                appointment_id: self.appointment_id,
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LogIncidentInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Short title, e.g. "Squeak from front left on braking".
    pub title: String,
    /// Defaults to `general`. Collisions/crashes with another party belong
    /// under category `accident`; anything a shop flagged but did not fix
    /// belongs under `shop_observation`.
    pub category: Option<IncidentCategory>,
    /// Longer description of what happened or was noticed.
    pub description: Option<String>,
    /// Odometer reading when it happened.
    pub odometer: Option<i32>,
    /// When it happened, `YYYY-MM-DD HH:MM:SS` or just `YYYY-MM-DD`. Defaults to the current local date and time.
    pub occurred_at: Option<String>,
    /// OBD-II codes if a scanner was involved, e.g. "P0301,P0420".
    pub obd_codes: Option<String>,
    /// Free-form notes.
    pub notes: Option<String>,
    /// If this is the same problem coming back, the id of the earlier
    /// incident it recurs from (same vehicle).
    pub recurrence_of_id: Option<i32>,
    /// The service record (same vehicle) during which this was noticed, e.g.
    /// the visit where the shop flagged it.
    pub noticed_during_service_id: Option<i32>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetRecallStatusInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Recall id: the `finding_id` from `whats_coming` or `check_recalls`.
    pub finding_id: i32,
    /// `done` (the work was done and left no paperwork), `not_applicable`
    /// (the recall does not apply to this car), or `open` (undo either).
    pub status: RecallStatus,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetServiceInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Service record id.
    pub service_id: i32,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetIncidentInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Incident id, from `log_incident` or `summarize_recent_activity`.
    pub incident_id: i32,
}

impl LogIncidentInput {
    pub fn into_domain(self) -> DomainResult<(i32, NewIncident)> {
        Ok((
            self.vehicle_id,
            NewIncident {
                category: self.category.unwrap_or(IncidentCategory::General),
                title: self.title,
                description: self.description,
                odometer: self.odometer,
                occurred_at: dates::parse_optional_day_or_time("occurred_at", self.occurred_at)?,
                obd_codes: self.obd_codes,
                notes: self.notes,
                recurrence_of_id: self.recurrence_of_id,
                noticed_during_service_id: self.noticed_during_service_id,
                ..Default::default()
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveNoteInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// The thing to remember, in plain words.
    pub note: String,
}

impl SaveNoteInput {
    /// Thin alias over `incident::create`: category `note`, title = first 80
    /// chars of the note, description = the full note.
    pub fn into_domain(self) -> (i32, NewIncident) {
        let title: String = self.note.chars().take(80).collect();
        (
            self.vehicle_id,
            NewIncident {
                category: IncidentCategory::Note,
                title,
                description: Some(self.note),
                ..Default::default()
            },
        )
    }
}

/// A part listed on a service invoice. The service supplies the install
/// date, mileage and link.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InlinePartInput {
    /// Part name, e.g. "Front brake pads".
    pub name: String,
    /// Manufacturer/brand, e.g. "Sachs".
    pub manufacturer: Option<String>,
    /// Manufacturer part number.
    pub part_number: Option<String>,
    /// Where it was bought, e.g. "FCP Euro".
    pub seller: Option<String>,
    /// Purchase date, `YYYY-MM-DD`.
    pub purchase_date: Option<String>,
    /// What the part cost, integer cents. With `supplied_by: "shop"` the cost
    /// is already inside the invoice's `total_cost_cents` and is not added
    /// again; with `supplied_by: "self"` it was paid for elsewhere and is
    /// added on top of the invoice total.
    pub cost_cents: Option<i32>,
    /// Who supplied the part: `self` (the owner bought it, the default; its
    /// cost is added to the total) or `shop` (the shop sourced and billed it;
    /// its cost is already inside the invoice total).
    pub supplied_by: Option<Supplier>,
    /// Where it goes on the car (free text), e.g. "Front brakes".
    pub location: Option<String>,
    /// Free-form notes. Printed on the appointment's work sheet for the shop, so keep it to what the shop may read.
    pub notes: Option<String>,
    /// Part warranty expiry date, `YYYY-MM-DD`.
    pub warranty_expires_on: Option<String>,
    /// Part warranty expiry odometer reading (whole miles).
    pub warranty_expires_miles: Option<i32>,
    /// The seller's order or confirmation number.
    pub order_ref: Option<String>,
    /// The need this part fills (from `get_checklist`), for a part the shop
    /// supplied that was not recorded beforehand. The part is linked to the
    /// need in the same write. The need must have no part yet.
    pub need_id: Option<i32>,
    /// For a kit: each item inside it, with the number printed on that item's
    /// own packaging, in the order the work sheet should list them. Omit for a
    /// part that is not a kit.
    pub contents: Option<Vec<PartContentInput>>,
}

impl InlinePartInput {
    fn into_domain(self) -> DomainResult<NewPart> {
        Ok(NewPart {
            name: self.name,
            manufacturer: self.manufacturer,
            part_number: self.part_number,
            seller: self.seller,
            purchase_date: dates::parse_optional_date("purchase_date", self.purchase_date)?,
            cost_cents: self.cost_cents,
            supplied_by: self.supplied_by,
            location: self.location,
            notes: self.notes,
            warranty_expires_on: dates::parse_optional_date(
                "warranty_expires_on",
                self.warranty_expires_on,
            )?,
            warranty_expires_miles: self.warranty_expires_miles,
            order_ref: self.order_ref,
            need_id: self.need_id,
            contents: kit_contents(self.contents),
            ..Default::default()
        })
    }
}

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
}

/// One item inside a kit, such as the filter in an oil change kit.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PartContentInput {
    /// What the item is, e.g. "Oil filter".
    pub name: String,
    /// Manufacturer/brand, e.g. "Mann".
    pub manufacturer: Option<String>,
    /// The number printed on the item's own box or label, e.g. "HU 7020 z".
    pub part_number: Option<String>,
    /// How many of it one kit holds, whatever the need's quantity. Defaults
    /// to 1.
    pub quantity: Option<i32>,
    /// The `id` an item carries in a reply. It is ignored, so an item copied
    /// from a reply can be sent as it is; a new item gets a new id.
    pub id: Option<i32>,
}

/// A kit's items as the shared service takes them; none when the argument
/// is left out.
fn kit_contents(items: Option<Vec<PartContentInput>>) -> Vec<NewPartContent> {
    items.into_iter().flatten().map(Into::into).collect()
}

impl From<PartContentInput> for NewPartContent {
    fn from(item: PartContentInput) -> Self {
        NewPartContent {
            name: item.name,
            manufacturer: item.manufacturer,
            part_number: item.part_number,
            quantity: item.quantity,
        }
    }
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
                cost_currency: None,
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
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanWorkInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// What the work is, e.g. "Replace fuel pump (recall)".
    pub title: String,
    /// Estimated cost, integer cents (feeds the budget forecast).
    pub est_cost_cents: Option<i32>,
    /// Free-form notes. Printed on the appointment's work sheet for the shop, so keep it to what the shop may read.
    pub notes: Option<String>,
    /// The overdue/due schedule item this satisfies (from
    /// `whats_coming`) — completing the work then clears the
    /// reminder.
    pub schedule_item_id: Option<i32>,
    /// The research finding (e.g. a recall from `whats_coming` or `check_recalls`) this
    /// addresses. The recall reads as planned while the work is open and as done once the
    /// work is done. Completing the work through `record_service` also links the service to it.
    pub research_finding_id: Option<i32>,
    /// The incident this fixes (from `log_incident`) — completing the work
    /// links the incident to the service record.
    pub incident_id: Option<i32>,
    /// Date to look at this item again, `YYYY-MM-DD`, for work to watch
    /// rather than do now.
    pub revisit_by_date: Option<String>,
    /// Odometer reading at which to look at this item again. It is judged
    /// against the estimated mileage, which needs an odometer reading or a
    /// purchase mileage; until the car has one, an item with only this field
    /// reads `revisit_status: "ok"`.
    pub revisit_by_miles: Option<i32>,
    /// The appointment this job goes on (from `list_appointments`). It must be
    /// open. Leave it out for a job with no shop visit yet; it can be moved
    /// later with `update_planned_work`.
    pub appointment_id: Option<i32>,
    /// For a quote with no invoice: who gave it, such as the shop's name. A
    /// quote linked by `quote_service_id` takes its shop from that service.
    pub quoted_by: Option<String>,
    /// The day of the quote, `YYYY-MM-DD`.
    pub quoted_on: Option<String>,
    /// The service whose visit produced the quote (from `list_services`), when
    /// the quote came with an invoice for other work. The service then names
    /// the job in its `quoted_work_items`.
    pub quote_service_id: Option<i32>,
}

impl PlanWorkInput {
    pub fn into_domain(self) -> DomainResult<(i32, NewWorkItem)> {
        Ok((
            self.vehicle_id,
            NewWorkItem {
                title: self.title,
                notes: self.notes,
                schedule_item_id: self.schedule_item_id,
                research_finding_id: self.research_finding_id,
                incident_id: self.incident_id,
                est_cost_cents: self.est_cost_cents,
                revisit_by_date: dates::parse_optional_date(
                    "revisit_by_date",
                    self.revisit_by_date,
                )?,
                revisit_by_miles: self.revisit_by_miles,
                appointment_id: self.appointment_id,
                quoted_by: self.quoted_by,
                quoted_on: dates::parse_optional_date("quoted_on", self.quoted_on)?,
                quote_service_id: self.quote_service_id,
            },
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListPlannedWorkInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Also include finished work: done/dropped items. Default false (open
    /// work only).
    pub include_done: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LogMileageInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Odometer reading (whole miles).
    pub mileage: i32,
    /// When the reading was taken, `YYYY-MM-DD HH:MM:SS` or just `YYYY-MM-DD`. Defaults to the current local date and time.
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

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DismissScheduleItemInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Schedule item id, from `whats_coming`.
    pub schedule_item_id: i32,
    /// Why it's being waived, e.g. "independent shop handles this".
    pub reason: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreScheduleItemInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// The dismissed item's id: the `id` `dismiss_schedule_item` returned, or
    /// an item with `enabled: false` in `list_schedule` with
    /// `include_dismissed`.
    pub schedule_item_id: i32,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListScheduleInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Also list dismissed items, shown with `enabled: false`. Default false.
    pub include_dismissed: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SummarizeActivityInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Max items to return (default 20).
    pub limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchRecordsInput {
    /// Full-text query. Plain words; no special syntax needed.
    pub query: String,
    /// Restrict to one record kind. Default `all`.
    pub scope: Option<glovebox_shared::services::search::SearchScope>,
    /// Restrict to one vehicle (from `list_vehicles`). Omit to search every vehicle, archived (sold) ones included.
    pub vehicle_id: Option<i32>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileResearchFindingInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Finding kind: `forum_report`, `suggested_maintenance`, `upgrade_idea`,
    /// or `recall` (other values render under their own heading).
    pub category: String,
    /// Short title, e.g. "DSG service interval is 40k, not 60k".
    pub title: String,
    /// Longer description of what was found.
    pub description: Option<String>,
    /// Where this was found (URL).
    pub source_url: Option<String>,
    /// "critical", "recommended", "optional", or "informational".
    pub severity: Option<String>,
}

impl FileResearchFindingInput {
    pub fn into_domain(self) -> (i32, NewFiledFinding) {
        (
            self.vehicle_id,
            NewFiledFinding {
                category: self.category,
                title: self.title,
                description: self.description,
                source_url: self.source_url,
                severity: self.severity,
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LinkServiceToMaintenanceInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// The existing service record to link, e.g. an id returned by
    /// `record_service` or found via `summarize_recent_activity`.
    pub service_record_id: i32,
    /// Schedule item ids (from `whats_coming`) this service
    /// satisfies — linking clears their reminders.
    pub schedule_item_ids: Vec<i32>,
    /// `add` (default) keeps the record's existing links and adds these;
    /// `replace` overwrites them with exactly this list. Either way the
    /// schedule item of a job the service completed stays, and the reply
    /// lists it.
    pub mode: Option<glovebox_shared::services::service_record::LinkMode>,
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
