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
const REGULAR_TAG: &str = as_text(&tag(REGULAR));
const SEMIBOLD_TAG: &str = as_text(&tag(SEMIBOLD));

// A quoted FNV-1a hash of `bytes` as sixteen hex digits, for an `ETag`.
const fn tag(bytes: &[u8]) -> [u8; 18] {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
        i += 1;
    }
    let mut text = [b'"'; 18];
    let mut digit = 0;
    while digit < 16 {
        let nibble = ((hash >> (60 - 4 * digit)) & 0xf) as u8;
        text[digit + 1] = if nibble < 10 {
            b'0' + nibble
        } else {
            b'a' + nibble - 10
        };
        digit += 1;
    }
    text
}

const fn as_text(bytes: &'static [u8]) -> &'static str {
    match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => panic!("a tag is ASCII"),
    }
}

fn font(bytes: &'static [u8], etag: &'static str, request: &HeaderMap) -> Response {
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
    font(REGULAR, REGULAR_TAG, &request)
}

/// Serves IBM Plex Sans in its semibold weight, or `304 Not Modified` when
/// the request names its current `ETag`.
pub async fn semibold(request: HeaderMap) -> Response {
    font(SEMIBOLD, SEMIBOLD_TAG, &request)
}
