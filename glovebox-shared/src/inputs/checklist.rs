//! Input for reading a job checklist.

/// Which checklist to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChecklistScope {
    /// One appointment's checklist.
    Appointment(i32),
    /// One job and its needs.
    Job(i32),
    /// The vehicle's planned jobs that have needs and are on no appointment.
    Unscheduled,
}
