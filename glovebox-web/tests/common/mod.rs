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
