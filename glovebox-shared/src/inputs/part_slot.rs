pub struct NewPartSlot {
    pub name: String,
    pub category: Option<String>,
    pub oe_spec: Option<String>,
    pub oe_part_number: Option<String>,
    pub notes: Option<String>,
}

#[derive(Default)]
pub struct UpdatePartSlot {
    pub name: Option<String>,
    pub category: Option<Option<String>>,
    pub oe_spec: Option<Option<String>>,
    pub oe_part_number: Option<Option<String>>,
    pub notes: Option<Option<String>>,
}
