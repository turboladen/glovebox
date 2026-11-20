use super::shop::ShopChoice;

/// An appointment to create.
#[derive(Default)]
pub struct NewAppointment {
    /// What the visit is for; must not be blank.
    pub purpose: String,
    /// The shop from the shop directory; a blank name names none.
    pub shop: Option<ShopChoice>,
    /// The planned date; an appointment without one is not booked.
    pub planned_date: Option<chrono::NaiveDate>,
    pub notes: Option<String>,
}

/// Changes to an appointment. An absent field is left alone; an explicit
/// `None` in a double option clears the column.
#[derive(Default)]
pub struct UpdateAppointment {
    pub purpose: Option<String>,
    pub shop: Option<Option<ShopChoice>>,
    pub planned_date: Option<Option<chrono::NaiveDate>>,
    pub notes: Option<Option<String>>,
}

/// A change to an appointment's status, made together with its other edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusChange {
    /// Close it by hand.
    Close,
    /// Cancel it.
    Cancel,
    /// Clear both the closed and the cancelled mark.
    Reopen,
}

impl From<crate::services::appointment::AppointmentStatus> for StatusChange {
    /// The change that leaves an appointment in `status`.
    fn from(status: crate::services::appointment::AppointmentStatus) -> Self {
        use crate::services::appointment::AppointmentStatus;
        match status {
            AppointmentStatus::Planning => Self::Reopen,
            AppointmentStatus::Done => Self::Close,
            AppointmentStatus::Cancelled => Self::Cancel,
        }
    }
}
