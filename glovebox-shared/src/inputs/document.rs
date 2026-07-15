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
    /// Hex SHA-256 of the file bytes, computed by the store path from the
    /// bytes it wrote. The content-hash dedup key.
    pub content_sha256: String,
}

/// Where [`crate::services::document::store`] gets the file bytes.
pub enum DocumentSource {
    /// Raw file bytes already in memory — the HTTP multipart upload path.
    /// (The MCP `attach_document` tool has no inline-bytes route; it always
    /// resolves an [`DocumentSource::InboxPath`].) Capped at
    /// [`crate::services::document::MAX_FILE_BYTES`].
    Bytes(Vec<u8>),
    /// Path **relative to the configured inbox dir**
    /// (`AppConfig::inbox_dir`) — the MCP affordance for real files: the
    /// LLM saves the file into the inbox with its own file tools and passes
    /// only the path, so the bytes never travel through model context. The
    /// service COPIES the file into the files dir and leaves the inbox
    /// original in place (the LLM may retry, and the user may still want
    /// the original).
    InboxPath(String),
}

/// Full upload payload for [`crate::services::document::store`]: the file
/// source plus row metadata. The service owns validation (size cap, filename
/// sanitizing, inbox path containment, vehicle + linked-entity guards), disk
/// placement under the configured files dir, and the DB row — both the HTTP
/// upload handler and the MCP `attach_document` tool are thin mappers onto
/// this.
pub struct StoreDocument {
    pub vehicle_id: Option<i32>,
    /// Display title; defaults to `file_name` when `None`.
    pub title: Option<String>,
    /// Original file name (used for the stored name; sanitized). Defaults
    /// to the inbox file's basename for [`DocumentSource::InboxPath`];
    /// required for [`DocumentSource::Bytes`].
    pub file_name: Option<String>,
    /// The file bytes, inline or by inbox reference.
    pub source: DocumentSource,
    /// MIME type; inferred from the file extension when `None`.
    pub mime_type: Option<String>,
    pub doc_type: Option<String>,
    pub linked_entity_type: Option<String>,
    pub linked_entity_id: Option<i32>,
    pub notes: Option<String>,
    pub extracted_text: Option<String>,
}
