//! The form posts. Each handler calls one shared service function and then
//! redirects back to the page the form was on; a failed write shows that page
//! again with the error in the row it concerns.

use std::collections::HashMap;

use axum::{
    Form,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use glovebox_shared::{
    entities::{self, job_need},
    error::{DomainError, DomainResult},
    inputs::{
        need::{OrderDetails, ReturnInput, UpdateNeed},
        todo::NewTodo,
    },
    money,
    services::{need, todo, trim_to_none},
};
use serde::Deserialize;

use crate::{
    AppState, pages,
    view::{Failure, Target},
};

/// The page a form returns to: a path on this site, or the home page.
fn safe_back(back: Option<&str>) -> String {
    match back {
        Some(path)
            if path.starts_with('/')
                && !path.starts_with("//")
                && !path.starts_with("/\\")
                && path.bytes().all(|b| (0x21..=0x7e).contains(&b)) =>
        {
            path.to_owned()
        }
        _ => "/".to_owned(),
    }
}

/// The page a path names, when it is one the server renders.
enum Page {
    Home,
    Appointment(i32, i32),
    Job(i32, i32),
}

fn page_of(back: &str, vehicle: i32) -> Option<Page> {
    let path = back.split(['?', '#']).next()?;
    if path == "/" {
        return Some(Page::Home);
    }
    let parts: Vec<&str> = path.split('/').collect();
    let [_, "vehicles", vid, kind, id] = parts.as_slice() else {
        return None;
    };
    let (vid, id): (i32, i32) = (vid.parse().ok()?, id.parse().ok()?);
    // A page of another vehicle is never shown for this vehicle's write.
    if vid != vehicle {
        return None;
    }
    match *kind {
        "appointments" => Some(Page::Appointment(vid, id)),
        "jobs" => Some(Page::Job(vid, id)),
        _ => None,
    }
}

fn field_label(field: &str) -> &str {
    match field {
        "amount" | "cost_cents" => "Price",
        "purchase_date" => "Order date",
        "reason" => "Reason",
        "title" => "To-do",
        "supplied_by" => "Supplier",
        "approach" => "Approach",
        "option" => "Option",
        other => other,
    }
}

// The form input an `Invalid` error on `field` belongs to.
fn input_for(form: &str, field: &str) -> Option<&'static str> {
    match (form, field) {
        ("order", "amount" | "cost_cents") => Some("cost"),
        ("order", "purchase_date") => Some("purchase_date"),
        ("return" | "decide", "reason") => Some("reason"),
        _ => None,
    }
}

/// What a failed write is answered with.
struct Refusal {
    status: StatusCode,
    message: String,
    field: Option<&'static str>,
}

fn refusal(error: &DomainError, form: &str) -> Refusal {
    match error {
        DomainError::NotFound(message) => Refusal {
            status: StatusCode::NOT_FOUND,
            message: message.clone(),
            field: None,
        },
        DomainError::Invalid { field, message } => Refusal {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: format!("{}: {message}", field_label(field)),
            field: input_for(form, field),
        },
        DomainError::BadRequest(message) => Refusal {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: message.clone(),
            field: None,
        },
        DomainError::Db(_) | DomainError::Internal(_) => {
            tracing::error!(%error, "a form post failed");
            Refusal {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "Something went wrong, and nothing was saved.".to_owned(),
                field: None,
            }
        }
    }
}

/// What one post is about, for answering it.
struct Post<'a> {
    state: &'a AppState,
    vid: i32,
    back: String,
    target: Target,
    form: &'static str,
    values: HashMap<String, String>,
    /// The `done` query value the redirect carries, such as `need-3`.
    undo: Option<String>,
}

impl<'a> Post<'a> {
    // A post of `form` about `target` that returns to `back`, with no typed
    // values and no undo.
    fn new(
        state: &'a AppState,
        vid: i32,
        back: Option<&str>,
        target: Target,
        form: &'static str,
    ) -> Self {
        Self {
            state,
            vid,
            back: safe_back(back),
            target,
            form,
            values: HashMap::new(),
            undo: None,
        }
    }

    // Keeps what the owner typed, to show again if the write fails.
    fn typed(mut self, pairs: &[(&str, &str)]) -> Self {
        self.values.extend(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        );
        self
    }

    // Offers an undo for `done` on the page the redirect lands on.
    fn undo(mut self, done: String) -> Self {
        self.undo = Some(done);
        self
    }

    // Redirects after a write, or shows the page again with the error.
    async fn answer<T>(self, result: DomainResult<T>) -> Response {
        let Err(error) = result else {
            return self.redirect();
        };
        let refused = refusal(&error, self.form);
        let failure = Failure {
            vid: self.vid,
            target: self.target,
            form: self.form,
            message: refused.message.clone(),
            field: refused.field,
            values: self.values,
        };
        let view = pages::Page {
            failure: Some(&failure),
            ..Default::default()
        };
        match page_of(&self.back, self.vid) {
            Some(Page::Home) => pages::home_response(self.state, view, refused.status).await,
            Some(Page::Appointment(vid, id)) => {
                pages::appointment_response(self.state, vid, id, view, refused.status).await
            }
            Some(Page::Job(vid, id)) => {
                pages::job_response(self.state, vid, id, view, refused.status).await
            }
            None => pages::failure_page(self.state, refused.status, refused.message),
        }
    }

    fn redirect(&self) -> Response {
        let location = match &self.undo {
            Some(done) => {
                let base = self.back.split(['?', '#']).next().unwrap_or("/");
                format!("{base}?done={done}")
            }
            None => self.back.clone(),
        };
        Redirect::to(&location).into_response()
    }
}

// Reads an optional id typed into a form: blank is absent, and anything else
// must be a number or the write is refused as `Invalid` on `field`.
fn optional_id(text: &str, field: &str, message: &str) -> DomainResult<Option<i32>> {
    trim_to_none(text)
        .map(|text| {
            text.parse::<i32>()
                .map_err(|_| DomainError::invalid(field, message))
        })
        .transpose()
}

#[derive(Deserialize)]
pub struct SupplierForm {
    #[serde(default)]
    supplied_by: String,
    #[serde(default)]
    approach: String,
    back: Option<String>,
}

pub async fn post_supplier(
    State(state): State<AppState>,
    Path((vid, id)): Path<(i32, i32)>,
    Form(form): Form<SupplierForm>,
) -> Response {
    let post = Post::new(
        &state,
        vid,
        form.back.as_deref(),
        Target::Need(id),
        "supplier",
    )
    .typed(&[
        ("supplied_by", &form.supplied_by),
        ("approach", &form.approach),
    ]);
    let result = need::update(
        &state.db,
        vid,
        id,
        UpdateNeed {
            supplied_by: Some(trim_to_none(&form.supplied_by)),
            approach: Some(trim_to_none(&form.approach)),
            ..Default::default()
        },
    )
    .await;
    post.answer(result).await
}

#[derive(Deserialize)]
pub struct DecisionForm {
    #[serde(default)]
    option_id: String,
    #[serde(default)]
    reason: String,
    back: Option<String>,
}

pub async fn post_decision(
    State(state): State<AppState>,
    Path((vid, id)): Path<(i32, i32)>,
    Form(form): Form<DecisionForm>,
) -> Response {
    let post = Post::new(
        &state,
        vid,
        form.back.as_deref(),
        Target::Need(id),
        "decide",
    )
    .typed(&[("option_id", &form.option_id), ("reason", &form.reason)]);
    let result = decide(&state, vid, id, &form).await;
    post.answer(result).await
}

async fn decide(
    state: &AppState,
    vid: i32,
    id: i32,
    form: &DecisionForm,
) -> DomainResult<job_need::Model> {
    let option_id = optional_id(
        &form.option_id,
        "option",
        "choose one of the listed options",
    )?;
    // The form sends the stored reason along with an empty choice; clearing
    // the choice drops the reason.
    let reason = option_id.and(trim_to_none(&form.reason));
    need::decide(&state.db, vid, id, option_id, reason).await
}

#[derive(Deserialize)]
pub struct OrderedForm {
    #[serde(default)]
    seller: String,
    #[serde(default)]
    purchase_date: String,
    #[serde(default)]
    cost: String,
    #[serde(default)]
    order_ref: String,
    back: Option<String>,
}

pub async fn post_ordered(
    State(state): State<AppState>,
    Path((vid, id)): Path<(i32, i32)>,
    Form(form): Form<OrderedForm>,
) -> Response {
    let post = Post::new(&state, vid, form.back.as_deref(), Target::Need(id), "order").typed(&[
        ("seller", &form.seller),
        ("purchase_date", &form.purchase_date),
        ("cost", &form.cost),
        ("order_ref", &form.order_ref),
    ]);
    let result = order(&state, vid, id, &form).await;
    post.answer(result).await
}

async fn order(
    state: &AppState,
    vid: i32,
    id: i32,
    form: &OrderedForm,
) -> DomainResult<need::NeedPurchase> {
    let cost_cents = trim_to_none(&form.cost)
        .map(|text| money::parse_dollars(&text))
        .transpose()?;
    need::mark_ordered(
        &state.db,
        vid,
        id,
        OrderDetails {
            seller: trim_to_none(&form.seller),
            purchase_date: trim_to_none(&form.purchase_date),
            cost_cents,
            order_ref: trim_to_none(&form.order_ref),
        },
    )
    .await
}

#[derive(Deserialize)]
pub struct BackOnly {
    back: Option<String>,
}

pub async fn post_received(
    State(state): State<AppState>,
    Path((vid, id)): Path<(i32, i32)>,
    Form(form): Form<BackOnly>,
) -> Response {
    let post = Post::new(
        &state,
        vid,
        form.back.as_deref(),
        Target::Need(id),
        "received",
    )
    .undo(format!("need-{id}"));
    let result = need::mark_received(&state.db, vid, id, None).await;
    post.answer(result).await
}

pub async fn post_received_undo(
    State(state): State<AppState>,
    Path((vid, id)): Path<(i32, i32)>,
    Form(form): Form<BackOnly>,
) -> Response {
    let post = Post::new(
        &state,
        vid,
        form.back.as_deref(),
        Target::Need(id),
        "received_undo",
    );
    let result = need::clear_received(&state.db, vid, id).await;
    post.answer(result).await
}

#[derive(Deserialize)]
pub struct ReturnForm {
    #[serde(default)]
    reason: String,
    option_was_wrong: Option<String>,
    back: Option<String>,
}

pub async fn post_return(
    State(state): State<AppState>,
    Path((vid, id)): Path<(i32, i32)>,
    Form(form): Form<ReturnForm>,
) -> Response {
    let mut post = Post::new(
        &state,
        vid,
        form.back.as_deref(),
        Target::Need(id),
        "return",
    )
    .typed(&[("reason", &form.reason)]);
    if form.option_was_wrong.is_some() {
        post = post.typed(&[("option_was_wrong", "true")]);
    }
    let result = need::return_part(
        &state.db,
        vid,
        id,
        ReturnInput {
            reason: form.reason.clone(),
            option_was_wrong: form.option_was_wrong.is_some(),
        },
    )
    .await;
    post.answer(result).await
}

#[derive(Deserialize)]
pub struct TodoForm {
    #[serde(default)]
    title: String,
    #[serde(default)]
    appointment_id: String,
    back: Option<String>,
}

pub async fn post_todo(
    State(state): State<AppState>,
    Path(vid): Path<i32>,
    Form(form): Form<TodoForm>,
) -> Response {
    let post = Post::new(&state, vid, form.back.as_deref(), Target::TodoAdd, "todo")
        .typed(&[("title", &form.title)]);
    let result = add_todo(&state, vid, &form).await;
    post.answer(result).await
}

async fn add_todo(
    state: &AppState,
    vid: i32,
    form: &TodoForm,
) -> DomainResult<entities::todo::Model> {
    let appointment_id = optional_id(
        &form.appointment_id,
        "appointment",
        "is not an appointment number",
    )?;
    todo::add(
        &state.db,
        vid,
        NewTodo {
            title: form.title.clone(),
            appointment_id,
            ..Default::default()
        },
    )
    .await
}

pub async fn post_todo_done(
    State(state): State<AppState>,
    Path((vid, id)): Path<(i32, i32)>,
    Form(form): Form<BackOnly>,
) -> Response {
    let post = Post::new(&state, vid, form.back.as_deref(), Target::Todo, "todo_done")
        .undo(format!("todo-{id}"));
    let result = todo::complete(&state.db, vid, id, None).await;
    post.answer(result).await
}

pub async fn post_todo_undo(
    State(state): State<AppState>,
    Path((vid, id)): Path<(i32, i32)>,
    Form(form): Form<BackOnly>,
) -> Response {
    let post = Post::new(&state, vid, form.back.as_deref(), Target::Todo, "todo_undo");
    let result = todo::reopen(&state.db, vid, id).await;
    post.answer(result).await
}

pub async fn post_todo_delete(
    State(state): State<AppState>,
    Path((vid, id)): Path<(i32, i32)>,
    Form(form): Form<BackOnly>,
) -> Response {
    let post = Post::new(
        &state,
        vid,
        form.back.as_deref(),
        Target::Todo,
        "todo_delete",
    );
    let result = todo::delete(&state.db, vid, id).await;
    post.answer(result).await
}
