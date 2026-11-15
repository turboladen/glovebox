//! The typeface the pages are set in, compiled into the binary so the server
//! needs no files beside it.

use axum::{
    http::header::{CACHE_CONTROL, CONTENT_TYPE},
    response::IntoResponse,
};

const REGULAR: &[u8] = include_bytes!("../assets/IBMPlexSans-Regular.woff2");
const SEMIBOLD: &[u8] = include_bytes!("../assets/IBMPlexSans-SemiBold.woff2");

fn font(bytes: &'static [u8]) -> impl IntoResponse {
    (
        [
            (CONTENT_TYPE, "font/woff2"),
            (CACHE_CONTROL, "public, max-age=604800"),
        ],
        bytes,
    )
}

/// Serves IBM Plex Sans in its regular weight.
pub async fn regular() -> impl IntoResponse {
    font(REGULAR)
}

/// Serves IBM Plex Sans in its semibold weight.
pub async fn semibold() -> impl IntoResponse {
    font(SEMIBOLD)
}
