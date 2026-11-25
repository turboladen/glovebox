use crate::{
    entities::research_finding::{FindingCategory, FindingSeverity},
    services::finding::{FiledFindingStatus, ShownFindingStatus},
};

/// A finding to file. The category must not be `recall`.
#[derive(Debug)]
pub struct NewFinding {
    pub category: FindingCategory,
    /// What was found, in a line; must not be blank.
    pub title: String,
    pub description: Option<String>,
    pub source_url: Option<String>,
    pub severity: Option<FindingSeverity>,
}

/// Changes to a finding. An absent field is left alone; an explicit `None` in
/// a double option clears the column.
#[derive(Debug, Default)]
pub struct UpdateFinding {
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    /// The new kind, which must not be `recall`.
    pub category: Option<FindingCategory>,
    pub severity: Option<Option<FindingSeverity>>,
    pub source_url: Option<Option<String>>,
    pub status: Option<FiledFindingStatus>,
}

/// Which of a vehicle's findings a list holds. An absent field filters
/// nothing.
#[derive(Debug, Default)]
pub struct FindingFilter {
    /// The kind to list, which must not be `recall`.
    pub category: Option<FindingCategory>,
    /// The status a finding reads, not the one it stores.
    pub status: Option<ShownFindingStatus>,
    /// `true` keeps the findings a planned job addresses, `false` the rest.
    pub has_planned_work: Option<bool>,
    /// The most findings to return; at least 1.
    pub limit: Option<u32>,
}
