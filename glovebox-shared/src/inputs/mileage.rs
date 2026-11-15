pub struct NewMileageEntry {
    pub mileage: i32,
    pub recorded_at: Option<crate::dates::DayOrTime>,
    pub source: Option<String>,
    pub notes: Option<String>,
}
