#[derive(Default)]
pub struct UpdateFinding {
    pub status: Option<String>,
    pub linked_entity_type: Option<Option<String>>,
    pub linked_entity_id: Option<Option<i32>>,
}
