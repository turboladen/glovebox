//! Inputs for the recall and research tools.

use glovebox_shared::{
    entities::research_finding::{FindingCategory, FindingSeverity},
    inputs::finding::NewFinding,
    services::recall::RecallStatus,
};
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
    /// Finding kind: `forum_report` (what owners report), `suggested_maintenance` (work worth
    /// doing that the schedule does not list), `upgrade_idea`, `known_issue` (a failure this
    /// model is known for), `service_bulletin` (a TSB) or `other`. `recall` is refused:
    /// recalls come only from `check_recalls`.
    pub category: FindingCategory,
    /// Short title, e.g. "DSG service interval is 40k, not 60k". It must not be blank.
    pub title: String,
    /// Longer description of what was found.
    pub description: Option<String>,
    /// Where this was found (URL).
    pub source_url: Option<String>,
    /// How much it matters: `critical`, `recommended`, `optional` or `informational`.
    pub severity: Option<FindingSeverity>,
}

impl FileResearchFindingInput {
    pub fn into_domain(self) -> (i32, NewFinding) {
        (
            self.vehicle_id,
            NewFinding {
                category: self.category,
                title: self.title,
                description: self.description,
                source_url: self.source_url,
                severity: self.severity,
            },
        )
    }
}
