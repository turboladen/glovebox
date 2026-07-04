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
    /// Text content extracted from the file (OCR / LLM reading). Indexed by
    /// the documents FTS so search finds the document by its content.
    pub extracted_text: Option<String>,
}

/// Full upload payload for [`crate::services::document::store`]: the raw file
/// bytes plus row metadata. The service owns validation (size cap, filename
/// sanitizing, vehicle + linked-entity guards), disk placement under the
/// configured files dir, and the DB row — both the HTTP upload handler and
/// the MCP `attach_document` tool are thin mappers onto this.
pub struct StoreDocument {
    pub vehicle_id: Option<i32>,
    /// Display title; defaults to `file_name` when `None`.
    pub title: Option<String>,
    /// Original file name (used for the stored name; sanitized).
    pub file_name: String,
    /// Decoded file bytes. Capped at
    /// [`crate::services::document::MAX_FILE_BYTES`].
    pub bytes: Vec<u8>,
    /// MIME type; inferred from the file extension when `None`.
    pub mime_type: Option<String>,
    pub doc_type: Option<String>,
    pub linked_entity_type: Option<String>,
    pub linked_entity_id: Option<i32>,
    pub notes: Option<String>,
    pub extracted_text: Option<String>,
}
