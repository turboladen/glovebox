pub struct NewProvider {
    pub name: String,
    pub provider_type: String,
    pub api_key: Option<String>,
    pub api_base: Option<String>,
    pub model: Option<String>,
    pub is_default: Option<bool>,
    pub enabled: Option<bool>,
}

#[derive(Default)]
pub struct UpdateProvider {
    pub name: Option<String>,
    pub provider_type: Option<String>,
    pub api_key: Option<Option<String>>,
    pub api_base: Option<Option<String>>,
    pub model: Option<Option<String>>,
    pub is_default: Option<bool>,
    pub enabled: Option<bool>,
}

/// Input for an AI chat turn. `attachment` bytes are read by the caller (or the
/// service) from a linked document; the domain layer stays HTTP-agnostic.
pub struct ChatInput {
    pub vehicle_id: Option<i32>,
    pub conversation_id: i32,
    pub message: String,
    pub provider_id: Option<i32>,
    pub document_id: Option<i32>,
}
