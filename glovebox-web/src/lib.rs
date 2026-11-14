//! The glovebox web page: a local, server-rendered view of the same database
//! the MCP server uses.

mod guard;
mod pages;

use axum::{Router, middleware, routing::get};
use chrono::NaiveDate;
use glovebox_shared::db::DatabaseConnection;

/// What every handler reads: the database, the name of the environment the
/// process serves, and the clock that says what day it is.
#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub env_name: String,
    pub today: fn() -> NaiveDate,
}

/// Builds the application over `state`. Every response, errors included,
/// carries the content security policy, and a request addressed to another
/// host or posted from another origin is refused with 403.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(pages::get_home))
        .fallback(pages::not_found)
        .layer(middleware::from_fn(guard::same_origin_only))
        .layer(middleware::map_response(guard::content_security_policy))
        .with_state(state)
}
