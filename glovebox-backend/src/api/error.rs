use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Debug)]
pub enum ApiError {
    NotFound(String),
    BadRequest(String),
    Internal(String),
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::Internal(msg) => {
                tracing::error!("Internal error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
        };
        (status, Json(ErrorBody { error: message })).into_response()
    }
}

impl From<sea_orm::DbErr> for ApiError {
    fn from(err: sea_orm::DbErr) -> Self {
        ApiError::Internal(err.to_string())
    }
}

impl From<glovebox_shared::error::DomainError> for ApiError {
    fn from(err: glovebox_shared::error::DomainError) -> Self {
        use glovebox_shared::error::DomainError;
        match err {
            DomainError::NotFound(m) => ApiError::NotFound(m),
            DomainError::Invalid { field, message } => {
                ApiError::BadRequest(format!("{field}: {message}"))
            }
            DomainError::Db(e) => ApiError::Internal(e.to_string()),
            DomainError::Internal(m) => ApiError::Internal(m),
        }
    }
}
