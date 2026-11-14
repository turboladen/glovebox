//! The pages the server renders.

use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use glovebox_shared::{
    entities::{job_need, todo},
    error::{DomainError, DomainResult},
    services::{
        appointment,
        checklist::{self, NeedView, VehicleHome},
        need::{self, Stage},
        part, todo as todo_svc, vehicle,
    },
};
use serde::Deserialize;

use crate::{
    AppState,
    view::{
        Banner, CardContext, Count, Failure, JobCard, Steps, Target, TodoCard, TodoForm, job_card,
        sentence_case, steps,
    },
};

/// What `base.html` shows on every page.
struct Chrome {
    /// The environment's name, followed by the database file's name when
    /// `GLOVEBOX_DB_PATH` chose the file.
    environment: String,
    /// The environment tag's tooltip.
    environment_title: String,
    /// Today's date as the shared layer writes it.
    today: String,
}

impl Chrome {
    fn of(state: &AppState) -> Self {
        let name = &state.env_name;
        let (environment, environment_title) = match &state.db_file {
            Some(file) => (
                format!("{name} \u{b7} {file}"),
                format!("This is the {name} environment, served from {file}"),
            ),
            None => (name.clone(), format!("This is the {name} database")),
        };
        Self {
            environment,
            environment_title,
            today: today_label(state),
        }
    }
}

fn today_label(state: &AppState) -> String {
    glovebox_shared::services::date_label((state.today)())
}

/// One vehicle on the home page, with its to-dos as rows the owner can tick.
struct HomeVehicle {
    home: VehicleHome,
    todos: Vec<TodoCard>,
    todo_form: TodoForm,
}

#[derive(Template)]
#[template(path = "home.html")]
struct HomePage {
    chrome: Chrome,
    banner: Option<Banner>,
    top_error: Option<String>,
    vehicles: Vec<HomeVehicle>,
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

pub async fn get_home(State(state): State<AppState>, Query(query): Query<PageQuery>) -> Response {
    home_response(
        &state,
        Page {
            done: query.done.as_deref(),
            open: None,
            failure: None,
        },
        StatusCode::OK,
    )
    .await
}

/// Renders the home page with `status`. A failed to-do add is shown in its
/// vehicle's form, and any other failure at the top of the page.
pub async fn home_response(state: &AppState, view: Page<'_>, status: StatusCode) -> Response {
    let homes = match checklist::home(&state.db).await {
        Ok(homes) => homes,
        Err(e) => return internal(&e),
    };
    let failure = view.failure;
    let mut banner_found = None;
    let mut vehicles = Vec::with_capacity(homes.len());
    for home in homes {
        let vid = home.vehicle.id;
        if banner_found.is_none() {
            banner_found = home_banner(state, vid, view.done).await;
        }
        let todos = home
            .todos
            .iter()
            .map(|t| TodoCard {
                vid,
                id: t.id,
                title: t.title.clone(),
                back: "/".to_owned(),
                done: false,
            })
            .collect();
        let own = failure.filter(|f| f.target == Target::TodoAdd && f.vid == vid);
        vehicles.push(HomeVehicle {
            todo_form: TodoForm {
                error: own.map(|f| f.message.clone()),
                title: own
                    .and_then(|f| f.values.get("title").cloned())
                    .unwrap_or_default(),
            },
            todos,
            home,
        });
    }
    let placed = failure.is_none_or(|f| f.target == Target::TodoAdd);
    let page = HomePage {
        chrome: Chrome::of(state),
        banner: banner_found,
        top_error: failure.filter(|_| !placed).map(|f| f.message.clone()),
        vehicles,
    };
    html(status, page.render())
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
    /// `need-<id>`: the need whose details start open.
    open: Option<String>,
}

fn open_need(query: &PageQuery) -> Option<i32> {
    query.open.as_deref()?.strip_prefix("need-")?.parse().ok()
}

/// The record a `?done=` value names: `need-<id>` or `todo-<id>`.
enum Done {
    Need(i32),
    Todo(i32),
}

impl Done {
    fn parse(done: Option<&str>) -> Option<Self> {
        let done = done?;
        if let Some(id) = done.strip_prefix("need-") {
            return id.parse().ok().map(Self::Need);
        }
        done.strip_prefix("todo-")?.parse().ok().map(Self::Todo)
    }
}

// The undo strip for a need marked received, when it still reads received.
fn need_banner(vid: i32, back: &str, need: &job_need::Model, stage: Stage) -> Option<Banner> {
    (stage == Stage::Received).then(|| Banner {
        message: format!("Marked \u{201c}{}\u{201d} received.", need.name),
        action: format!("/vehicles/{vid}/needs/{}/received/undo", need.id),
        back: back.to_owned(),
    })
}

// The undo strip for a ticked to-do, when it is still done.
fn todo_banner(vid: i32, back: &str, todo: &todo::Model) -> Option<Banner> {
    todo.done_on.as_ref()?;
    Some(Banner {
        message: format!("Ticked \u{201c}{}\u{201d}.", todo.title),
        action: format!("/vehicles/{vid}/todos/{}/done/undo", todo.id),
        back: back.to_owned(),
    })
}

// The undo strip for a page whose needs and to-dos are in hand. A value that
// names nothing on the page shows no strip.
fn page_banner<'a>(
    vid: i32,
    back: &str,
    done: Option<&str>,
    needs: impl IntoIterator<Item = &'a NeedView>,
    todos: &[todo::Model],
) -> Option<Banner> {
    match Done::parse(done)? {
        Done::Need(id) => needs
            .into_iter()
            .find(|n| n.need.id == id)
            .and_then(|n| need_banner(vid, back, &n.need, n.stage)),
        Done::Todo(id) => todos
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| todo_banner(vid, back, t)),
    }
}

// The undo strip for the home page, which shows no needs and no ticked
// to-dos, so the record is looked up on each vehicle in turn. A value that
// names nothing the vehicle has shows no strip.
async fn home_banner(state: &AppState, vid: i32, done: Option<&str>) -> Option<Banner> {
    match Done::parse(done)? {
        Done::Need(id) => {
            let found = need::require(&state.db, vid, id).await.ok()?;
            let part = part::get(&state.db, vid, found.part_id?).await.ok()?;
            need_banner(vid, "/", &found, need::stage(&found, Some(&part)))
        }
        Done::Todo(id) => {
            let found = todo_svc::require(&state.db, vid, id).await.ok()?;
            todo_banner(vid, "/", &found)
        }
    }
}

/// What a page shows besides its record: an undo strip, the need to open and
/// the failure of the write that led here.
#[derive(Clone, Copy, Default)]
pub struct Page<'a> {
    pub done: Option<&'a str>,
    pub open: Option<i32>,
    pub failure: Option<&'a Failure>,
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
    appointment_response(
        &state,
        vid,
        id,
        Page {
            done: query.done.as_deref(),
            open: open_need(&query),
            failure: None,
        },
        StatusCode::OK,
    )
    .await
}

/// Renders an appointment's page with `status`. `failure` is shown in the row
/// it names, or at the top of the page when that row is not on it.
pub async fn appointment_response(
    state: &AppState,
    vid: i32,
    id: i32,
    view: Page<'_>,
    status: StatusCode,
) -> Response {
    match appointment_page(state, vid, id, view).await {
        Ok(page) => html(status, page.render()),
        Err(e) => load_failed(state, &e),
    }
}

async fn appointment_page(
    state: &AppState,
    vid: i32,
    id: i32,
    view: Page<'_>,
) -> DomainResult<AppointmentPage> {
    let Page {
        done,
        open,
        failure,
    } = view;
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
        open,
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
        banner: page_banner(
            vid,
            &back,
            done,
            list.jobs.iter().flat_map(|j| &j.needs),
            &list.todos,
        ),
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
    job_response(
        &state,
        vid,
        id,
        Page {
            done: query.done.as_deref(),
            open: open_need(&query),
            failure: None,
        },
        StatusCode::OK,
    )
    .await
}

/// Renders a job's page with `status`. `failure` is shown in the row it
/// names, or at the top of the page when that row is not on it.
pub async fn job_response(
    state: &AppState,
    vid: i32,
    id: i32,
    view: Page<'_>,
    status: StatusCode,
) -> Response {
    match job_page(state, vid, id, view).await {
        Ok(page) => html(status, page.render()),
        Err(e) => load_failed(state, &e),
    }
}

async fn job_page(state: &AppState, vid: i32, id: i32, view: Page<'_>) -> DomainResult<JobPage> {
    let Page {
        done,
        open,
        failure,
    } = view;
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
        open,
    };
    let card = job_card(&view, &ctx);
    let placed = match failure.map(|f| f.target) {
        Some(Target::Need(need_id)) => card.needs.iter().any(|n| n.id == need_id),
        Some(Target::TodoAdd | Target::Todo) => false,
        None => true,
    };
    Ok(JobPage {
        chrome: Chrome::of(state),
        banner: page_banner(vid, &back, done, &view.needs, &[]),
        top_error: failure.filter(|_| !placed).map(|f| f.message.clone()),
        vid,
        vehicle: car.name,
        appointment: linked,
        j: card,
    })
}
