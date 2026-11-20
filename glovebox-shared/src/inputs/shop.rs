/// A shop to add to the directory.
#[derive(Debug, Clone, Default)]
pub struct NewShop {
    pub name: String,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub website: Option<String>,
    pub specialty: Option<String>,
    pub notes: Option<String>,
}

/// Changes to a shop. An absent field is left alone and `Some(None)` clears
/// it; the name cannot be cleared.
#[derive(Debug, Clone, Default)]
pub struct UpdateShop {
    pub name: Option<String>,
    pub address: Option<Option<String>>,
    pub phone: Option<Option<String>>,
    pub website: Option<Option<String>>,
    pub specialty: Option<Option<String>>,
    pub notes: Option<Option<String>>,
}
