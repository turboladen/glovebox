use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("{0}")]
    NotFound(String),
    /// A field-attributed validation failure; renders as "{field}: {message}".
    #[error("{field}: {message}")]
    Invalid { field: String, message: String },
    /// A general 400 with a verbatim message (no field prefix).
    #[error("{0}")]
    BadRequest(String),
    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
    #[error("{0}")]
    Internal(String),
}

pub type DomainResult<T> = Result<T, DomainError>;

impl DomainError {
    pub fn invalid(field: impl Into<String>, message: impl Into<String>) -> Self {
        DomainError::Invalid {
            field: field.into(),
            message: message.into(),
        }
    }
}
