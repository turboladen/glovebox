pub struct NewModelTemplate {
    pub platform_id: Option<i32>,
    pub platform_ref: Option<String>,
    pub year: Option<i32>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub trim_level: Option<String>,
    pub body_style: Option<String>,
    pub engine: Option<String>,
    pub transmission: Option<String>,
    pub drivetrain: Option<String>,
}

#[derive(Default)]
pub struct UpdateModelTemplate {
    pub platform_id: Option<Option<i32>>,
    pub platform_ref: Option<Option<String>>,
    pub year: Option<Option<i32>>,
    pub make: Option<Option<String>>,
    pub model: Option<Option<String>>,
    pub trim_level: Option<Option<String>>,
    pub body_style: Option<Option<String>>,
    pub engine: Option<Option<String>>,
    pub transmission: Option<Option<String>>,
    pub drivetrain: Option<Option<String>>,
}
