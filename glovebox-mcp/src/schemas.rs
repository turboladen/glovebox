//! Tool input/output shapes for the MCP surface.
//!
//! Inputs derive `Deserialize` + `JsonSchema`; doc comments become field
//! descriptions in the advertised schema, so write them for an LLM. Each
//! input maps 1:1 onto a `glovebox_shared::inputs` struct (fields the MCP
//! surface doesn't expose are filled with `None`) — no business logic here.

use base64::Engine as _;
use glovebox_shared::{
    entities::vehicle,
    error::{DomainError, DomainResult},
    inputs::{
        document::StoreDocument,
        incident::NewIncident,
        mileage::NewMileageEntry,
        part::NewPart,
        service_record::{NewLineItem, NewServiceRecord},
        visit::{CompleteVisit, NewVisit},
        work_item::NewWorkItem,
    },
    services::research::NewFiledFinding,
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
    /// Who paid: `self` (default), `insurance`, or `third_party` (e.g. the
    /// other driver). Anything not paid by the owner counts as covered, not
    /// out-of-pocket, in cost summaries.
    pub paid_by: Option<String>,
    /// Who exactly paid / claim number, e.g. "Progressive claim #12345".
    pub payer_note: Option<String>,
    /// Itemized invoice lines.
    pub line_items: Option<Vec<LineItemInput>>,
    /// Link this service to a build (from `list_builds`).
    pub build_id: Option<i32>,
    /// Ids of schedule items (from `check_due_maintenance`) this work
    /// satisfies — linking clears the reminder and restarts its interval.
    pub schedule_item_ids: Option<Vec<i32>>,
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
                paid_by: self.paid_by,
                payer_note: self.payer_note,
                schedule_item_ids: self.schedule_item_ids,
                part_ids: None,
                line_items,
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct LogIncidentInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Short title, e.g. "Squeak from front left on braking".
    pub title: String,
    /// One of: `general`, `noise`, `leak`, `warning_light`, `cosmetic`,
    /// `performance`, `obd_code`, `damage`, `accident`, `note`. Defaults to
    /// `general`. Collisions/crashes with another party belong under
    /// category `accident`.
    pub category: Option<String>,
    /// Longer description of what happened or was noticed.
    pub description: Option<String>,
    /// Odometer reading when it happened.
    pub odometer: Option<i32>,
    /// When it happened, `YYYY-MM-DD HH:MM:SS`. Defaults to now.
    pub occurred_at: Option<String>,
    /// OBD-II codes if a scanner was involved, e.g. "P0301,P0420".
    pub obd_codes: Option<String>,
    /// Free-form notes.
    pub notes: Option<String>,
    /// Link this incident to a build (from `list_builds`).
    pub build_id: Option<i32>,
    /// If this is the same problem coming back, the id of the earlier
    /// incident it recurs from (same vehicle).
    pub recurrence_of_id: Option<i32>,
}

impl LogIncidentInput {
    pub fn into_domain(self) -> (i32, NewIncident) {
        (
            self.vehicle_id,
            NewIncident {
                category: self.category.unwrap_or_else(|| "general".to_string()),
                title: self.title,
                description: self.description,
                odometer: self.odometer,
                occurred_at: self.occurred_at,
                obd_codes: self.obd_codes,
                notes: self.notes,
                build_id: self.build_id,
                recurrence_of_id: self.recurrence_of_id,
                ..Default::default()
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
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
                category: "note".to_string(),
                title,
                description: Some(self.note),
                ..Default::default()
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct RecordPartInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Part name, e.g. "Sachs SRE clutch kit".
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
    /// Lifecycle status: `purchased`, `installed`, or `replaced`.
    /// Defaults to `purchased`.
    pub status: Option<String>,
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
    /// Free-form notes.
    pub notes: Option<String>,
    /// Link this part to a build (from `list_builds`).
    pub build_id: Option<i32>,
    /// Part warranty expiry date, `YYYY-MM-DD` (if the part carries one).
    pub warranty_expires_on: Option<String>,
    /// Part warranty expiry odometer reading (whole miles).
    pub warranty_expires_miles: Option<i32>,
}

impl RecordPartInput {
    pub fn into_domain(self) -> (i32, NewPart) {
        (
            self.vehicle_id,
            NewPart {
                name: self.name,
                manufacturer: self.manufacturer,
                part_number: self.part_number,
                oe_part_number_replaced: None,
                seller: self.seller,
                purchase_date: self.purchase_date,
                cost_cents: self.cost_cents,
                cost_currency: None,
                invoice_url: None,
                manufacturer_url: None,
                retailer_url: self.retailer_url,
                status: self.status,
                installed_date: self.installed_date,
                installed_odometer: self.installed_odometer,
                installed_service_id: self.installed_service_id,
                notes: self.notes,
                build_id: self.build_id,
                location: self.location,
                warranty_expires_on: self.warranty_expires_on,
                warranty_expires_miles: self.warranty_expires_miles,
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct PlanWorkInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// What the work is, e.g. "Replace fuel pump (recall)".
    pub title: String,
    /// Estimated cost, integer cents (feeds the budget forecast).
    pub est_cost_cents: Option<i32>,
    /// Free-form notes.
    pub notes: Option<String>,
    /// The overdue/due schedule item this satisfies (from
    /// `check_due_maintenance`) — completing the work then clears the
    /// reminder.
    pub schedule_item_id: Option<i32>,
    /// The research finding (e.g. a recall from `check_recalls`) this
    /// addresses — completing the work then closes the finding.
    pub research_finding_id: Option<i32>,
    /// The incident this fixes (from `log_incident`) — completing the work
    /// links the incident to the service record.
    pub incident_id: Option<i32>,
    /// Link this work to a build (from `list_builds`).
    pub build_id: Option<i32>,
}

impl PlanWorkInput {
    pub fn into_domain(self) -> (i32, NewWorkItem) {
        (
            self.vehicle_id,
            NewWorkItem {
                title: self.title,
                notes: self.notes,
                schedule_item_id: self.schedule_item_id,
                research_finding_id: self.research_finding_id,
                incident_id: self.incident_id,
                build_id: self.build_id,
                est_cost_cents: self.est_cost_cents,
                visit_id: None,
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct ListPlannedWorkInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Also include finished work: done/dropped items and
    /// completed/canceled visits. Default false (open work only).
    pub include_done: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ScheduleVisitInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// When the visit is planned for, `YYYY-MM-DD`.
    pub planned_date: Option<String>,
    /// Shop doing the work (free text) — the primary way to name the shop.
    /// Omit for DIY.
    pub shop_name: Option<String>,
    /// Id of a saved shop from the shops list, when the user refers to one
    /// they already track. `shop_name` free text works fine on its own.
    pub shop_id: Option<i32>,
    /// Work item ids (from `plan_work` / `list_planned_work`) to group into
    /// this visit — they flip to `scheduled`.
    pub work_item_ids: Option<Vec<i32>>,
    /// Free-form notes.
    pub notes: Option<String>,
}

impl ScheduleVisitInput {
    pub fn into_domain(self) -> (i32, NewVisit) {
        (
            self.vehicle_id,
            NewVisit {
                planned_date: self.planned_date,
                shop_name: self.shop_name,
                shop_id: self.shop_id,
                notes: self.notes,
                work_item_ids: self.work_item_ids,
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct CancelVisitInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Visit id, from `schedule_visit` / `list_planned_work`.
    pub visit_id: i32,
}

#[derive(Deserialize, JsonSchema)]
pub struct CompleteVisitInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Visit id, from `schedule_visit` / `list_planned_work`.
    pub visit_id: i32,
    /// Date the work was done, `YYYY-MM-DD`.
    pub service_date: String,
    /// Odometer reading at service time. Also logs a mileage entry.
    pub mileage: Option<i32>,
    /// What was done. Defaults to the attached work items' titles.
    pub description: Option<String>,
    /// Total invoice amount in integer cents (e.g. $123.45 -> 12345).
    pub total_cost_cents: Option<i32>,
    /// Parts portion of the invoice, integer cents.
    pub parts_cost_cents: Option<i32>,
    /// Labor portion of the invoice, integer cents.
    pub labor_cost_cents: Option<i32>,
    /// Who paid: `self` (default), `insurance`, or `third_party`.
    pub paid_by: Option<String>,
    /// Who exactly paid / claim number, e.g. "Progressive claim #12345".
    pub payer_note: Option<String>,
    /// Free-form notes.
    pub notes: Option<String>,
}

impl CompleteVisitInput {
    pub fn into_domain(self) -> (i32, i32, CompleteVisit) {
        (
            self.vehicle_id,
            self.visit_id,
            CompleteVisit {
                service_date: self.service_date,
                mileage: self.mileage,
                description: self.description,
                total_cost_cents: self.total_cost_cents,
                parts_cost_cents: self.parts_cost_cents,
                labor_cost_cents: self.labor_cost_cents,
                paid_by: self.paid_by,
                payer_note: self.payer_note,
                notes: self.notes,
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
pub struct DismissScheduleItemInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Schedule item id, from `check_due_maintenance`.
    pub schedule_item_id: i32,
    /// Why it's being waived, e.g. "independent shop handles this".
    pub reason: Option<String>,
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
    /// `incidents`, `builds`, `documents`, or `research`. Default `all`.
    pub scope: Option<String>,
    /// Restrict to one vehicle (from `list_vehicles`). Omit to search the garage.
    pub vehicle_id: Option<i32>,
}

#[derive(Deserialize, JsonSchema)]
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
pub struct AttachDocumentInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Original file name with extension, e.g. "fcp-invoice-2026-06.pdf".
    /// Used for the stored name and MIME detection; unsafe characters are
    /// normalized.
    pub file_name: String,
    /// The file's bytes, base64-encoded (standard alphabet, with padding).
    /// Decoded size cap: 10 MiB.
    pub content_base64: String,
    /// Display title, e.g. "FCP Euro invoice — clutch kit". Defaults to
    /// `file_name`.
    pub title: Option<String>,
    /// The text YOU extracted from the document (read/OCR it yourself and
    /// pass it here). Full-text indexed — without it, `find_documents` can
    /// only match the title and file name.
    pub extracted_text: Option<String>,
    /// Link target kind: `service` (a service record — the usual case for
    /// invoices), `part`, or `incident`. Pair with `linked_entity_id`.
    pub linked_entity_type: Option<String>,
    /// Id of the linked record (must belong to the same vehicle), e.g. the
    /// id returned by `record_service`. Pair with `linked_entity_type`.
    pub linked_entity_id: Option<i32>,
}

impl AttachDocumentInput {
    /// Decode the base64 payload into the shared store input. A malformed
    /// payload is an LLM-recoverable `BadRequest`, not a protocol failure.
    pub fn into_domain(self) -> DomainResult<StoreDocument> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(self.content_base64.trim())
            .map_err(|e| {
                DomainError::BadRequest(format!(
                    "content_base64 is not valid base64 ({e}). Send the file bytes encoded with \
                     the standard base64 alphabet."
                ))
            })?;
        Ok(StoreDocument {
            vehicle_id: Some(self.vehicle_id),
            title: self.title,
            file_name: self.file_name,
            bytes,
            mime_type: None,
            doc_type: None,
            linked_entity_type: self.linked_entity_type,
            linked_entity_id: self.linked_entity_id,
            notes: None,
            extracted_text: self.extracted_text,
        })
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct LinkServiceToMaintenanceInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// The existing service record to link, e.g. an id returned by
    /// `record_service` or found via `summarize_recent_activity`.
    pub service_record_id: i32,
    /// Schedule item ids (from `check_due_maintenance`) this service
    /// satisfies — linking clears their reminders.
    pub schedule_item_ids: Vec<i32>,
    /// `add` (default) keeps the record's existing links and adds these;
    /// `replace` overwrites them with exactly this list.
    pub mode: Option<String>,
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
