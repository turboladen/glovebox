//! A same-origin check that keeps other web pages from reading or posting to
//! this server, and the content security policy every response carries.

use axum::{
    extract::Request,
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{CONTENT_SECURITY_POLICY, HOST, ORIGIN},
    },
    middleware::Next,
    response::{IntoResponse, Response},
};

// `style-src` allows inline styles because the one stylesheet lives in
// `base.html`. The page has no script, and nothing from another origin loads.
const POLICY: &str = "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; base-uri \
                      'none'; frame-ancestors 'none'";

/// Sets the `Content-Security-Policy` header on a response, replacing any
/// header of that name, so a browser runs no script and loads nothing the
/// server does not serve itself.
pub async fn content_security_policy(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static(POLICY));
    response
}

/// Answers 403 to a request whose `Host` is not `127.0.0.1` or `localhost`
/// with a port, and to a request that is not a GET or HEAD unless its
/// `Origin` header is absent or names the same authority as its `Host`.
pub async fn same_origin_only(req: Request, next: Next) -> Response {
    if !is_allowed(&req) {
        return (StatusCode::FORBIDDEN, "cross-site request refused\n").into_response();
    }
    next.run(req).await
}

fn is_allowed(req: &Request) -> bool {
    let headers = req.headers();
    let Some(host) = header_text(headers, &HOST) else {
        return false;
    };
    if !is_local_authority(host) {
        return false;
    }
    if req.method().is_safe() {
        return true;
    }
    match headers.get(ORIGIN) {
        None => true,
        Some(origin) => origin
            .to_str()
            .ok()
            .and_then(|o| o.strip_prefix("http://"))
            .is_some_and(|authority| authority == host),
    }
}

fn header_text<'a>(headers: &'a HeaderMap, name: &axum::http::HeaderName) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

// The `Host` check covers DNS rebinding, where a hostile name resolves to
// 127.0.0.1 and the browser then treats the page as same-origin.
fn is_local_authority(authority: &str) -> bool {
    authority.rsplit_once(':').is_some_and(|(name, port)| {
        matches!(name, "127.0.0.1" | "localhost") && port.parse::<u16>().is_ok()
    })
}
