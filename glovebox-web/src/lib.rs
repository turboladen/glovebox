//! The glovebox web page: a local, server-rendered view of the same database
//! the MCP server uses.

mod actions;
mod guard;
mod pages;
mod view;

use axum::{
    Router, middleware,
    routing::{get, post},
};
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
        .route(
            "/vehicles/{vid}/appointments/{id}",
            get(pages::get_appointment),
        )
        .route("/vehicles/{vid}/jobs/{id}", get(pages::get_job))
        .route(
            "/vehicles/{vid}/needs/{id}/supplier",
            post(actions::post_supplier),
        )
        .route(
            "/vehicles/{vid}/needs/{id}/decision",
            post(actions::post_decision),
        )
        .route(
            "/vehicles/{vid}/needs/{id}/ordered",
            post(actions::post_ordered),
        )
        .route(
            "/vehicles/{vid}/needs/{id}/received",
            post(actions::post_received),
        )
        .route(
            "/vehicles/{vid}/needs/{id}/received/undo",
            post(actions::post_received_undo),
        )
        .route(
            "/vehicles/{vid}/needs/{id}/return",
            post(actions::post_return),
        )
        .route("/vehicles/{vid}/todos", post(actions::post_todo))
        .route(
            "/vehicles/{vid}/todos/{id}/done",
            post(actions::post_todo_done),
        )
        .route(
            "/vehicles/{vid}/todos/{id}/done/undo",
            post(actions::post_todo_undo),
        )
        .route(
            "/vehicles/{vid}/todos/{id}/delete",
            post(actions::post_todo_delete),
        )
        .fallback(pages::not_found)
        .layer(middleware::from_fn(guard::same_origin_only))
        .layer(middleware::map_response(guard::content_security_policy))
        .with_state(state)
}
