//! The pages the server renders.

use askama::Template;
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use glovebox_shared::services::checklist::{self, VehicleHome};

use crate::AppState;

/// What `base.html` shows on every page.
struct Chrome {
    /// The environment's name, or `None` for the live one.
    environment: Option<String>,
    /// Today's date as `YYYY-MM-DD`.
    today: String,
}

impl Chrome {
    fn of(state: &AppState) -> Self {
        Self {
            environment: (state.env_name != glovebox_shared::env::LIVE)
                .then(|| state.env_name.clone()),
            today: (state.today)().format("%Y-%m-%d").to_string(),
        }
    }
}

#[derive(Template)]
#[template(path = "home.html")]
struct HomePage {
    chrome: Chrome,
    vehicles: Vec<VehicleHome>,
}

#[derive(Template)]
#[template(path = "not_found.html")]
struct NotFoundPage {
    chrome: Chrome,
}

// Answers `status` with a rendered page, or a 500 when the template fails.
fn html(status: StatusCode, rendered: askama::Result<String>) -> Response {
    match rendered {
        Ok(body) => (status, Html(body)).into_response(),
        Err(e) => internal(&e),
    }
}

// Logs the detail and answers a 500 that does not repeat it.
fn internal(error: &dyn std::fmt::Display) -> Response {
    tracing::error!(%error, "could not render a page");
    (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong.\n").into_response()
}

pub async fn get_home(State(state): State<AppState>) -> Response {
    match checklist::home(&state.db).await {
        Ok(vehicles) => {
            let page = HomePage {
                chrome: Chrome::of(&state),
                vehicles,
            };
            html(StatusCode::OK, page.render())
        }
        Err(e) => internal(&e),
    }
}

pub async fn not_found(State(state): State<AppState>) -> Response {
    let page = NotFoundPage {
        chrome: Chrome::of(&state),
    };
    html(StatusCode::NOT_FOUND, page.render())
}
