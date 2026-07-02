pub struct NewObservation {
    pub category: String,
    pub title: String,
    pub description: Option<String>,
    pub odometer: Option<i32>,
    pub observed_at: Option<String>,
    pub obd_codes: Option<String>,
    pub notes: Option<String>,
}

#[derive(Default)]
pub struct UpdateObservation {
    pub category: Option<String>,
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub odometer: Option<Option<i32>>,
    pub observed_at: Option<String>,
    pub obd_codes: Option<Option<String>>,
    pub resolved: Option<bool>,
    pub resolved_service_id: Option<Option<i32>>,
    pub notes: Option<Option<String>>,
}
