//! Inputs for the recall and research tools.

use glovebox_shared::{
    entities::research_finding::{FindingCategory, FindingSeverity},
    inputs::finding::{FindingFilter, NewFinding, UpdateFinding},
    services::{
        finding::{FiledFindingStatus, ShownFindingStatus},
        recall::RecallStatus,
    },
};
use schemars::JsonSchema;
use serde::Deserialize;

use super::nullable;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetRecallStatusInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Recall id: the `finding_id` from `whats_coming` or `check_recalls`.
    pub finding_id: i32,
    /// `done` (the work was done and left no paperwork), `not_applicable`
    /// (the recall does not apply to this car), or `open` (undo either; the
    /// recall then reads what its jobs say).
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

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListResearchFindingsInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Only findings of this kind. `recall` is refused: `list_recalls` lists the recalls.
    pub category: Option<FindingCategory>,
    /// Only findings that read this status: `open` (nothing was done about it), `planned` (a
    /// planned job addresses it), `done` or `dismissed`.
    pub status: Option<ShownFindingStatus>,
    /// `true` for only the findings a planned job addresses, `false` for only the rest.
    pub has_planned_work: Option<bool>,
    /// The most findings to return, counted from the newest after the other filters. At least
    /// 1; 0 is an error. Omit for all.
    pub limit: Option<u32>,
}

impl ListResearchFindingsInput {
    pub fn into_domain(self) -> (i32, FindingFilter) {
        (
            self.vehicle_id,
            FindingFilter {
                category: self.category,
                status: self.status,
                has_planned_work: self.has_planned_work,
                limit: self.limit,
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResearchFindingParams {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Finding id: the `id` from `list_research_findings`, `file_research_finding` or the
    /// `open_findings` in `whats_coming`. A recall's `finding_id` is not found here.
    pub finding_id: i32,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateResearchFindingInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Finding id: the `id` from `list_research_findings`, `file_research_finding` or the
    /// `open_findings` in `whats_coming`. A recall's `finding_id` is not found here.
    pub finding_id: i32,
    /// New title. It must not be blank.
    pub title: Option<String>,
    /// Longer description of what was found.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub description: Option<Option<String>>,
    /// New kind: `forum_report`, `suggested_maintenance`, `upgrade_idea`, `known_issue`,
    /// `service_bulletin` or `other`. `recall` is refused: a finding cannot become a recall.
    pub category: Option<FindingCategory>,
    /// How much it matters: `critical`, `recommended`, `optional` or `informational`.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<FindingSeverity>")]
    pub severity: Option<Option<FindingSeverity>>,
    /// Where this was found (URL).
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub source_url: Option<Option<String>>,
    /// `done` (it was acted on and no job records that), `dismissed` (the owner decided
    /// against it), or `open` (undo either; the finding then reads what its jobs say, which
    /// may be `planned` or `done`). Omit to leave the status alone.
    pub status: Option<FiledFindingStatus>,
}

impl UpdateResearchFindingInput {
    pub fn into_domain(self) -> (i32, i32, UpdateFinding) {
        (
            self.vehicle_id,
            self.finding_id,
            UpdateFinding {
                title: self.title,
                description: self.description,
                category: self.category,
                severity: self.severity,
                source_url: self.source_url,
                status: self.status,
            },
        )
    }
}
