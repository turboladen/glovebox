pub struct NewPlatform {
    pub name: String,
    pub website_url: Option<String>,
    pub api_base_url: Option<String>,
    pub notes: Option<String>,
}

#[derive(Default)]
pub struct UpdatePlatform {
    pub name: Option<String>,
    pub website_url: Option<Option<String>>,
    pub api_base_url: Option<Option<String>>,
    pub notes: Option<Option<String>>,
}
