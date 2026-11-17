//! The typeface the pages are set in, compiled into the binary so the server
//! needs no files beside it.

use axum::{
    http::{
        HeaderMap, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH},
    },
    response::{IntoResponse, Response},
};

const REGULAR: &[u8] = include_bytes!("../assets/IBMPlexSans-Regular.woff2");
const SEMIBOLD: &[u8] = include_bytes!("../assets/IBMPlexSans-SemiBold.woff2");

// Each font's validator is a hash of its bytes, worked out when the binary is
// built, so it changes exactly when the file does.
const REGULAR_HASH: u64 = fnv(REGULAR);
const SEMIBOLD_HASH: u64 = fnv(SEMIBOLD);

// The FNV-1a hash of `bytes`.
const fn fnv(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
        i += 1;
    }
    hash
}

fn font(bytes: &'static [u8], hash: u64, request: &HeaderMap) -> Response {
    let etag = format!("\"{hash:016x}\"");
    // `If-None-Match` compares weakly, so a tag a cache marked `W/` matches.
    let unchanged = request
        .get(IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',')
                .map(str::trim)
                .any(|t| t == "*" || t.strip_prefix("W/").unwrap_or(t) == etag)
        });
    let headers = [
        (CONTENT_TYPE, "font/woff2"),
        (CACHE_CONTROL, "public, max-age=604800"),
        (ETAG, etag.as_str()),
    ];
    if unchanged {
        (StatusCode::NOT_MODIFIED, headers).into_response()
    } else {
        (headers, bytes).into_response()
    }
}

/// Serves IBM Plex Sans in its regular weight, or `304 Not Modified` when the
/// request names its current `ETag`.
pub async fn regular(request: HeaderMap) -> Response {
    font(REGULAR, REGULAR_HASH, &request)
}

/// Serves IBM Plex Sans in its semibold weight, or `304 Not Modified` when
/// the request names its current `ETag`.
pub async fn semibold(request: HeaderMap) -> Response {
    font(SEMIBOLD, SEMIBOLD_HASH, &request)
}
