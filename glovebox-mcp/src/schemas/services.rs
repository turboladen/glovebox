//! Inputs for the service tools.

use glovebox_shared::{
    dates,
    entities::{
        part::Supplier,
        service_record::{Payer, Performer},
        service_record_line_item::WorkKind,
    },
    error::DomainResult,
    inputs::{
        part::NewPart,
        service_record::{NewLineItem, NewServiceRecord, ServiceFilter, UpdateServiceRecord},
    },
};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{PartContentInput, kit_contents, nullable, shop_choice};

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
    /// Odometer reading at service time, zero or more. Also logs a mileage entry.
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
                labor_cost_cents: self.labor_cost_cents,
                total_cost_cents: self.total_cost_cents,
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

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListServicesInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Earliest service date to include, `YYYY-MM-DD`.
    pub from: Option<String>,
    /// Latest service date to include, `YYYY-MM-DD`.
    pub to: Option<String>,
    /// Only services at this shop, by id from `list_shops`. An id that names no shop is an error.
    pub shop_id: Option<i32>,
    /// Only services with at least one line of this kind.
    pub work_kind: Option<WorkKind>,
    /// The most services to return, counted from the newest service date after the other filters. At least 1; 0 is an error. Omit for all.
    pub limit: Option<u32>,
}

impl ListServicesInput {
    pub fn into_domain(self) -> DomainResult<(i32, ServiceFilter)> {
        Ok((
            self.vehicle_id,
            ServiceFilter {
                from: dates::parse_optional_date("from", self.from)?,
                to: dates::parse_optional_date("to", self.to)?,
                shop_id: self.shop_id,
                work_kind: self.work_kind,
                limit: self.limit,
            },
        ))
    }
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
    /// Odometer reading at service time, zero or more. Keeps the service's own odometer log in step.
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
    /// The shop that did the work, by its id from `list_shops`.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub shop_id: Option<Option<i32>>,
    /// The shop by name instead of `shop_id`. It must match one shop from `list_shops` (case and surrounding spaces aside); add a new shop with `add_shop` first.
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
    /// What happened to the work this call takes off the service (jobs left out of `work_item_ids`, fitted parts left out of `part_ids`, recalls left out of `recall_finding_ids`, incidents left out of `resolved_incident_ids` that no job of this service still brings and no other service resolves): `did_not_happen` undoes it, so jobs go back to planned, parts read purchased, recalls read open and incidents read unresolved unless the owner resolved them by hand; `done_another_way` keeps it done and only unlinks it from this invoice. Either way a job taken off stops giving its schedule item a baseline from this service, so after `done_another_way` name the item in `schedule_item_ids` on the service that did the work when its reminder should stay cleared. Needed only when the call takes such work off; ask the owner.
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
                shop: super::shop_change(self.shop_id, self.shop_name)?,
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
