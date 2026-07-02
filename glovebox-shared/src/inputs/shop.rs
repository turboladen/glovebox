pub struct NewShop {
    pub name: String,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub website: Option<String>,
    pub specialty: Option<String>,
    pub notes: Option<String>,
}

#[derive(Default)]
pub struct UpdateShop {
    pub name: Option<String>,
    pub address: Option<Option<String>>,
    pub phone: Option<Option<String>>,
    pub website: Option<Option<String>>,
    pub specialty: Option<Option<String>>,
    pub notes: Option<Option<String>>,
}
