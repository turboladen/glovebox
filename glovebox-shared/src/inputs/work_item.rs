pub struct NewWorkItem {
    pub title: String,
    pub notes: Option<String>,
    /// Source links (all optional; each must belong to the same vehicle):
    /// a due/overdue schedule item, a research finding (e.g. a recall), an
    /// incident, or a build.
    pub schedule_item_id: Option<i32>,
    pub research_finding_id: Option<i32>,
    pub incident_id: Option<i32>,
    pub build_id: Option<i32>,
    pub est_cost_cents: Option<i32>,
    /// Attach to an existing visit at creation (sets status `scheduled`).
    pub visit_id: Option<i32>,
}

#[derive(Default)]
pub struct UpdateWorkItem {
    pub title: Option<String>,
    pub notes: Option<Option<String>>,
    pub schedule_item_id: Option<Option<i32>>,
    pub research_finding_id: Option<Option<i32>>,
    pub incident_id: Option<Option<i32>>,
    pub build_id: Option<Option<i32>>,
    pub est_cost_cents: Option<Option<i32>>,
    /// One of `planned | scheduled | done | dropped`.
    pub status: Option<String>,
    /// Moving drives status: `Some(Some(v))` attaches (target must be an
    /// open visit; the item must be planned/scheduled) and sets
    /// `scheduled`; `Some(None)` detaches and a `scheduled` item goes back
    /// to `planned`. Items on a completed visit never move.
    pub visit_id: Option<Option<i32>>,
}
