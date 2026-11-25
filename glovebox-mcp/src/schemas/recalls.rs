//! Inputs for the recall and research tools.

use glovebox_shared::services::{recall::RecallStatus, research::NewFiledFinding};
use schemars::JsonSchema;
use serde::Deserialize;

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
