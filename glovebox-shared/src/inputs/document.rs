/// Filter for listing documents.
#[derive(Default)]
pub struct DocumentFilter {
    pub vehicle_id: Option<i32>,
    pub linked_entity_type: Option<String>,
    pub linked_entity_id: Option<i32>,
}

/// Persistence payload for a stored document. File I/O (writing bytes to disk,
/// computing `file_path`/`file_size_bytes`) happens in the caller; this struct
/// carries only what is written to the database row.
pub struct NewDocument {
    pub vehicle_id: Option<i32>,
    pub title: String,
    pub file_path: String,
    pub file_name: String,
    pub mime_type: Option<String>,
    pub file_size_bytes: Option<i32>,
    pub doc_type: Option<String>,
    pub linked_entity_type: Option<String>,
    pub linked_entity_id: Option<i32>,
    pub notes: Option<String>,
}
