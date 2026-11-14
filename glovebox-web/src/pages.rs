//! The pages the server renders.

use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use glovebox_shared::{
    error::{DomainError, DomainResult},
    services::{
        appointment,
        checklist::{self, VehicleHome},
        need, todo, vehicle,
    },
};
use serde::Deserialize;

use crate::{
    AppState,
    view::{
        Banner, CardContext, Count, Failure, JobCard, Steps, Target, TodoForm, job_card,
        sentence_case, steps,
    },
};

/// What `base.html` shows on every page.
struct Chrome {
    /// The environment's name, or `None` for the live one.
    environment: Option<String>,
    /// Today's date as the shared layer writes it.
    today: String,
}

impl Chrome {
    fn of(state: &AppState) -> Self {
        Self {
            environment: (state.env_name != glovebox_shared::env::LIVE)
                .then(|| state.env_name.clone()),
            today: today_label(state),
        }
    }
}

fn today_label(state: &AppState) -> String {
    glovebox_shared::services::date_label((state.today)())
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

#[derive(Template)]
#[template(path = "failure.html")]
struct FailurePage {
    chrome: Chrome,
    message: String,
}

#[derive(Template)]
#[template(path = "appointment.html")]
struct AppointmentPage {
    chrome: Chrome,
    banner: Option<Banner>,
    top_error: Option<String>,
    vid: i32,
    appointment_id: i32,
    /// The path the forms on this page return to.
    back: String,
    vehicle: String,
    purpose: String,
    shop: Option<String>,
    date: Option<String>,
    status_class: &'static str,
    status_text: String,
    needs_open: Count,
    todos_open: Count,
    job_count: Count,
    steps: Steps,
    todo_form: TodoForm,
    jobs: Vec<JobCard>,
}

/// The appointment a job page links back to.
struct LinkedAppointment {
    id: i32,
    purpose: String,
}

#[derive(Template)]
#[template(path = "job.html")]
struct JobPage {
    chrome: Chrome,
    banner: Option<Banner>,
    top_error: Option<String>,
    vid: i32,
    vehicle: String,
    appointment: Option<LinkedAppointment>,
    j: JobCard,
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

// Answers a failed page load: a missing record is the 404 page and anything
// else is a bare 500.
fn load_failed(state: &AppState, error: &DomainError) -> Response {
    match error {
        DomainError::NotFound(_) => not_found_page(state),
        other => internal(other),
    }
}

fn not_found_page(state: &AppState) -> Response {
    let page = NotFoundPage {
        chrome: Chrome::of(state),
    };
    html(StatusCode::NOT_FOUND, page.render())
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
    not_found_page(&state)
}

/// Renders a page saying `message` with `status`, for a failed write whose
/// own page cannot be shown.
pub fn failure_page(state: &AppState, status: StatusCode, message: String) -> Response {
    let page = FailurePage {
        chrome: Chrome::of(state),
        message,
    };
    html(status, page.render())
}

/// The query string of a page that follows an action with an undo.
#[derive(Deserialize)]
pub struct PageQuery {
    done: Option<String>,
}

// Builds the undo strip for `?done=need-<id>` or `?done=todo-<id>`. A value
// that names nothing the vehicle has shows no strip.
async fn banner(state: &AppState, vid: i32, back: &str, done: Option<&str>) -> Option<Banner> {
    let done = done?;
    if let Some(id) = done.strip_prefix("need-").and_then(|n| n.parse().ok()) {
        let found = need::require(&state.db, vid, id).await.ok()?;
        return Some(Banner {
            message: format!("Marked \u{201c}{}\u{201d} received.", found.name),
            action: format!("/vehicles/{vid}/needs/{id}/received/undo"),
            back: back.to_owned(),
        });
    }
    let id = done.strip_prefix("todo-")?.parse().ok()?;
    let found = todo::require(&state.db, vid, id).await.ok()?;
    Some(Banner {
        message: format!("Ticked \u{201c}{}\u{201d}.", found.title),
        action: format!("/vehicles/{vid}/todos/{id}/done/undo"),
        back: back.to_owned(),
    })
}

/// The path of an appointment's page.
pub fn appointment_path(vid: i32, id: i32) -> String {
    format!("/vehicles/{vid}/appointments/{id}")
}

/// The path of a job's page.
pub fn job_path(vid: i32, id: i32) -> String {
    format!("/vehicles/{vid}/jobs/{id}")
}

pub async fn get_appointment(
    State(state): State<AppState>,
    Path((vid, id)): Path<(i32, i32)>,
    Query(query): Query<PageQuery>,
) -> Response {
    appointment_response(&state, vid, id, query.done.as_deref(), None, StatusCode::OK).await
}

/// Renders an appointment's page with `status`. `failure` is shown in the row
/// it names, or at the top of the page when that row is not on it.
pub async fn appointment_response(
    state: &AppState,
    vid: i32,
    id: i32,
    done: Option<&str>,
    failure: Option<&Failure>,
    status: StatusCode,
) -> Response {
    match appointment_page(state, vid, id, done, failure).await {
        Ok(page) => html(status, page.render()),
        Err(e) => load_failed(state, &e),
    }
}

async fn appointment_page(
    state: &AppState,
    vid: i32,
    id: i32,
    done: Option<&str>,
    failure: Option<&Failure>,
) -> DomainResult<AppointmentPage> {
    let list = checklist::for_appointment(&state.db, vid, id).await?;
    let car = vehicle::require(&state.db, vid).await?;
    let summary = list
        .appointment
        .as_ref()
        .ok_or_else(|| DomainError::NotFound(format!("Appointment {id} not found")))?;
    let back = appointment_path(vid, id);
    let today = today_label(state);
    let ctx = CardContext {
        vid,
        back: &back,
        today: &today,
        failure,
    };
    let jobs: Vec<JobCard> = list.jobs.iter().map(|j| job_card(j, &ctx)).collect();
    let grouped = steps(&list, &jobs, &ctx);

    let typed_title = failure
        .filter(|f| f.target == Target::TodoAdd)
        .and_then(|f| f.values.get("title").cloned())
        .unwrap_or_default();
    let todo_error = failure
        .filter(|f| f.target == Target::TodoAdd)
        .map(|f| f.message.clone());
    let placed = match failure.map(|f| f.target) {
        Some(Target::Need(need_id)) => jobs.iter().any(|j| j.needs.iter().any(|n| n.id == need_id)),
        Some(Target::TodoAdd) | None => true,
        Some(Target::Todo) => false,
    };

    Ok(AppointmentPage {
        chrome: Chrome::of(state),
        banner: banner(state, vid, &back, done).await,
        top_error: failure.filter(|_| !placed).map(|f| f.message.clone()),
        vid,
        appointment_id: id,
        vehicle: car.name,
        purpose: summary.appointment.purpose.clone(),
        shop: summary.appointment.shop_name.clone(),
        date: summary.appointment.planned_date.clone(),
        status_class: summary.status,
        status_text: sentence_case(summary.status),
        needs_open: Count::new(summary.needs_open, "need open", "needs open"),
        todos_open: Count::new(summary.todos_open, "to-do open", "to-dos open"),
        job_count: Count::new(summary.jobs, "job", "jobs"),
        steps: grouped,
        todo_form: TodoForm {
            error: todo_error,
            title: typed_title,
        },
        jobs,
        back,
    })
}

pub async fn get_job(
    State(state): State<AppState>,
    Path((vid, id)): Path<(i32, i32)>,
    Query(query): Query<PageQuery>,
) -> Response {
    job_response(&state, vid, id, query.done.as_deref(), None, StatusCode::OK).await
}

/// Renders a job's page with `status`. `failure` is shown in the row it
/// names, or at the top of the page when that row is not on it.
pub async fn job_response(
    state: &AppState,
    vid: i32,
    id: i32,
    done: Option<&str>,
    failure: Option<&Failure>,
    status: StatusCode,
) -> Response {
    match job_page(state, vid, id, done, failure).await {
        Ok(page) => html(status, page.render()),
        Err(e) => load_failed(state, &e),
    }
}

async fn job_page(
    state: &AppState,
    vid: i32,
    id: i32,
    done: Option<&str>,
    failure: Option<&Failure>,
) -> DomainResult<JobPage> {
    let view = checklist::for_job(&state.db, vid, id).await?;
    let car = vehicle::require(&state.db, vid).await?;
    let linked = match view.job.appointment_id {
        Some(appointment_id) => {
            let found = appointment::require(&state.db, vid, appointment_id).await?;
            Some(LinkedAppointment {
                id: found.id,
                purpose: found.purpose,
            })
        }
        None => None,
    };
    let back = job_path(vid, id);
    let today = today_label(state);
    let ctx = CardContext {
        vid,
        back: &back,
        today: &today,
        failure,
    };
    let card = job_card(&view, &ctx);
    let placed = match failure.map(|f| f.target) {
        Some(Target::Need(need_id)) => card.needs.iter().any(|n| n.id == need_id),
        Some(Target::TodoAdd | Target::Todo) => false,
        None => true,
    };
    Ok(JobPage {
        chrome: Chrome::of(state),
        banner: banner(state, vid, &back, done).await,
        top_error: failure.filter(|_| !placed).map(|f| f.message.clone()),
        vid,
        vehicle: car.name,
        appointment: linked,
        j: card,
    })
}
