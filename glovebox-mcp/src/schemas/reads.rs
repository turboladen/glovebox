//! Inputs for the tools that read across the records.

use glovebox_shared::{dates, error::DomainResult, services::costs::CostRange};
use schemars::JsonSchema;
use serde::Deserialize;

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

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SummarizeActivityInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Max items to return (default 20, at most 200).
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
pub struct CostSummaryInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// First date to count, `YYYY-MM-DD`, inclusive. Omit to count from the first record.
    pub from: Option<String>,
    /// Last date to count, `YYYY-MM-DD`, inclusive. Omit to count through the latest record. A `from` after `to` is an error.
    pub to: Option<String>,
}

impl CostSummaryInput {
    pub fn into_domain(self) -> DomainResult<(i32, CostRange)> {
        Ok((
            self.vehicle_id,
            CostRange {
                from: dates::parse_optional_date("from", self.from)?,
                to: dates::parse_optional_date("to", self.to)?,
            },
        ))
    }
}
