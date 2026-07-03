pub struct NewVisit {
    pub planned_date: Option<String>,
    pub shop_name: Option<String>,
    pub shop_id: Option<i32>,
    pub notes: Option<String>,
    /// Work items to attach (must belong to the same vehicle); attaching
    /// sets each item's `visit_id` and flips its status to `scheduled`.
    pub work_item_ids: Option<Vec<i32>>,
}

/// Updates apply only while the visit is open (`planned`/`scheduled`);
/// completed and canceled visits are immutable history.
#[derive(Default)]
pub struct UpdateVisit {
    pub planned_date: Option<Option<String>>,
    pub shop_name: Option<Option<String>>,
    pub shop_id: Option<Option<i32>>,
    pub notes: Option<Option<String>>,
    /// One of `planned | scheduled | canceled` — `completed` is only
    /// reachable through `services::visit::complete`. Canceling detaches
    /// the attached items back to the backlog (like delete does).
    pub status: Option<String>,
    /// Replace-all attach semantics: items newly listed are attached
    /// (`visit_id` set, status `scheduled`), items no longer listed are
    /// detached (`visit_id` NULL, status back to `planned` when it was
    /// `scheduled`).
    pub work_item_ids: Option<Vec<i32>>,
}

/// Actuals for closing out a visit — one transaction creates the service
/// record, clears satisfied reminders (via the items' schedule links),
/// resolves linked recalls/incidents, and marks everything done.
pub struct CompleteVisit {
    pub service_date: String,
    pub mileage: Option<i32>,
    /// Defaults to the attached items' titles joined with ", ".
    pub description: Option<String>,
    pub total_cost_cents: Option<i32>,
    pub parts_cost_cents: Option<i32>,
    pub labor_cost_cents: Option<i32>,
    /// Who paid: `self` (default), `insurance`, or `third_party`.
    pub paid_by: Option<String>,
    pub payer_note: Option<String>,
    pub notes: Option<String>,
}
