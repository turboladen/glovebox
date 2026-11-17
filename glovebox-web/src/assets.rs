//! The typeface the pages are set in, compiled into the binary so the server
//! needs no files beside it.

use std::sync::LazyLock;

use axum::{
    http::{
        HeaderMap, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH},
    },
    response::{IntoResponse, Response},
};

const REGULAR: &[u8] = include_bytes!("../assets/IBMPlexSans-Regular.woff2");
const SEMIBOLD: &[u8] = include_bytes!("../assets/IBMPlexSans-SemiBold.woff2");

// Each font's validator is a hash of its bytes, so it changes exactly when
// the file does.
static REGULAR_TAG: LazyLock<String> = LazyLock::new(|| etag(REGULAR));
static SEMIBOLD_TAG: LazyLock<String> = LazyLock::new(|| etag(SEMIBOLD));

// A quoted FNV-1a hash of `bytes`, for an `ETag`.
fn etag(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("\"{hash:016x}\"")
}

fn font(bytes: &'static [u8], etag: &str, request: &HeaderMap) -> Response {
    let headers = [
        (CONTENT_TYPE, "font/woff2"),
        (CACHE_CONTROL, "public, max-age=604800"),
        (ETAG, etag),
    ];
    let unchanged = request
        .get(IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag || t.trim() == "*"));
    if unchanged {
        (StatusCode::NOT_MODIFIED, headers).into_response()
    } else {
        (headers, bytes).into_response()
    }
}

/// Serves IBM Plex Sans in its regular weight, or `304 Not Modified` when the
/// request names its current `ETag`.
pub async fn regular(request: HeaderMap) -> Response {
    font(REGULAR, &REGULAR_TAG, &request)
}

/// Serves IBM Plex Sans in its semibold weight, or `304 Not Modified` when
/// the request names its current `ETag`.
pub async fn semibold(request: HeaderMap) -> Response {
    font(SEMIBOLD, &SEMIBOLD_TAG, &request)
}
