use crate::entities::work_item::JobStatus;

#[derive(Default)]
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
    /// Date by which to look at this item again.
    pub revisit_by_date: Option<chrono::NaiveDate>,
    /// Odometer reading by which to look at this item again.
    pub revisit_by_miles: Option<i32>,
    /// The appointment the job goes on; it must be the vehicle's and still open.
    pub appointment_id: Option<i32>,
    /// Who gave the quote the job came from. A blank value reads as none.
    pub quoted_by: Option<String>,
    /// The day the quote was given.
    pub quoted_on: Option<chrono::NaiveDate>,
    /// The service whose visit produced the quote; it must be the vehicle's.
    pub quote_service_id: Option<i32>,
}

#[derive(Default)]
pub struct UpdateWorkItem {
    pub title: Option<String>,
    pub notes: Option<Option<String>>,
    pub schedule_item_id: Option<Option<i32>>,
    pub research_finding_id: Option<Option<i32>>,
    pub incident_id: Option<Option<i32>>,
    pub est_cost_cents: Option<Option<i32>>,
    pub revisit_by_date: Option<Option<chrono::NaiveDate>>,
    pub revisit_by_miles: Option<Option<i32>>,
    pub status: Option<JobStatus>,
    /// Moves the job to an appointment (which must be open) or, with `None`, off it.
    pub appointment_id: Option<Option<i32>>,
    /// Sets or, with `None`, clears who gave the quote. A blank value clears it.
    pub quoted_by: Option<Option<String>>,
    /// Sets or, with `None`, clears the day the quote was given.
    pub quoted_on: Option<Option<chrono::NaiveDate>>,
    /// Sets or, with `None`, clears the quoting service, which must be the vehicle's.
    pub quote_service_id: Option<Option<i32>>,
}
