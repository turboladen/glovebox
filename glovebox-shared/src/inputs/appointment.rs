/// An appointment to create.
#[derive(Default)]
pub struct NewAppointment {
    /// What the visit is for; must not be blank.
    pub purpose: String,
    pub shop_name: Option<String>,
    /// A shop from the shop directory.
    pub shop_id: Option<i32>,
    /// The planned date (`YYYY-MM-DD`); an appointment without one is not booked.
    pub planned_date: Option<String>,
    pub notes: Option<String>,
}

/// Changes to an appointment. An absent field is left alone; an explicit
/// `None` in a double option clears the column.
#[derive(Default)]
pub struct UpdateAppointment {
    pub purpose: Option<String>,
    pub shop_name: Option<Option<String>>,
    pub shop_id: Option<Option<i32>>,
    pub planned_date: Option<Option<String>>,
    pub notes: Option<Option<String>>,
}
