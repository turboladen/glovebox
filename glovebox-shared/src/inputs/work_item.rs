pub struct NewWorkItem {
    pub title: String,
    pub notes: Option<String>,
    /// Source links (all optional; each must belong to the same vehicle):
    /// a due/overdue schedule item, a research finding (e.g. a recall), or an
    /// incident.
    pub schedule_item_id: Option<i32>,
    pub research_finding_id: Option<i32>,
    pub incident_id: Option<i32>,
    pub est_cost_cents: Option<i32>,
}

#[derive(Default)]
pub struct UpdateWorkItem {
    pub title: Option<String>,
    pub notes: Option<Option<String>>,
    pub schedule_item_id: Option<Option<i32>>,
    pub research_finding_id: Option<Option<i32>>,
    pub incident_id: Option<Option<i32>>,
    pub est_cost_cents: Option<Option<i32>>,
    /// One of `planned | done | dropped`.
    pub status: Option<String>,
}
