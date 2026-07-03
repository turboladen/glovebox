pub struct NewBuild {
    pub name: String,
    pub description: Option<String>,
    pub target_date: Option<String>,
}

#[derive(Default)]
pub struct UpdateBuild {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub target_date: Option<Option<String>>,
    pub status: Option<String>,
}
