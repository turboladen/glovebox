//! Helpers shared by the route tests.
#![allow(dead_code)]

use axum::{
    Router,
    body::Body,
    http::{Request, Response, StatusCode},
};
use chrono::NaiveDate;
use glovebox_shared::{db::DatabaseConnection, test_support::test_db};
use glovebox_web::{AppState, router};
use http_body_util::BodyExt;
use tower::ServiceExt;

/// The origin the tests pretend to be served from.
pub const HOST: &str = "127.0.0.1:4828";

pub fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 13).unwrap()
}

/// An in-memory database with migrations applied.
pub async fn db() -> DatabaseConnection {
    test_db().await
}

/// The application over `db`, serving the environment called `env_name`.
pub fn app(db: DatabaseConnection, env_name: &str) -> Router {
    router(AppState {
        db,
        env_name: env_name.to_owned(),
        db_file: None,
        today,
    })
}

/// The application over `db`, serving `env_name` from a database file that
/// `GLOVEBOX_DB_PATH` named.
pub fn app_with_file(db: DatabaseConnection, env_name: &str, file: &str) -> Router {
    router(AppState {
        db,
        env_name: env_name.to_owned(),
        db_file: Some(file.to_owned()),
        today,
    })
}

/// A request to `path` with the headers a browser sends from the page itself.
pub fn request(method: &str, path: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(path)
        .header("host", HOST)
}

pub fn get(path: &str) -> Request<Body> {
    request("GET", path).body(Body::empty()).unwrap()
}

/// Sends `req` through `app` and returns the whole response with its body as text.
pub async fn send(app: Router, req: Request<Body>) -> (Response<Body>, String) {
    let res = app.oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    (Response::from_parts(parts, Body::empty()), text)
}

/// Sends `req` through `app` and returns the status and body.
pub async fn page(app: Router, req: Request<Body>) -> (StatusCode, String) {
    let (res, body) = send(app, req).await;
    (res.status(), body)
}

/// Percent-encodes `text` for a form body.
pub fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A form post of `fields` to `path`, sent the way a browser sends it from the
/// page itself.
pub fn form_post(path: &str, fields: &[(&str, &str)]) -> Request<Body> {
    let body = fields
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    request("POST", path)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .unwrap()
}

/// Loads `path` from the application over `db` and returns the status and
/// body.
pub async fn get_page(db: &DatabaseConnection, path: &str) -> (StatusCode, String) {
    page(app(db.clone(), "test"), get(path)).await
}

/// Posts `fields` to `path` on the application over `db` and returns the
/// status, the redirect location if there is one, and the body.
pub async fn post_form(
    db: &DatabaseConnection,
    path: &str,
    fields: &[(&str, &str)],
) -> (StatusCode, Option<String>, String) {
    let (res, body) = send(app(db.clone(), "test"), form_post(path, fields)).await;
    let location = res
        .headers()
        .get("location")
        .map(|v| v.to_str().unwrap().to_owned());
    (res.status(), location, body)
}
