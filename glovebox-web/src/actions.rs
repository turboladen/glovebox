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
    error::{DomainError, DomainResult},
    inputs::{
        need::{OrderDetails, ReturnInput, UpdateNeed},
        todo::NewTodo,
    },
    money,
    services::{need, todo},
};
use serde::Deserialize;

use crate::{
    AppState, pages,
    view::{Failure, Target},
};

/// A form field that was left empty reads as absent.
fn present(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

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
    Appointment(i32, i32),
    Job(i32, i32),
}

fn page_of(back: &str) -> Option<Page> {
    let path = back.split(['?', '#']).next()?;
    let parts: Vec<&str> = path.split('/').collect();
    let [_, "vehicles", vid, kind, id] = parts.as_slice() else {
        return None;
    };
    let (vid, id) = (vid.parse().ok()?, id.parse().ok()?);
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

impl Post<'_> {
    // Redirects after a write, or shows the page again with the error.
    async fn answer(self, result: DomainResult<()>) -> Response {
        let error = match result {
            Ok(()) => return self.redirect(),
            Err(error) => error,
        };
        let refused = refusal(&error, self.form);
        let failure = Failure {
            target: self.target,
            form: self.form,
            message: refused.message.clone(),
            field: refused.field,
            values: self.values,
        };
        match page_of(&self.back) {
            Some(Page::Appointment(vid, id)) => {
                pages::appointment_response(
                    self.state,
                    vid,
                    id,
                    None,
                    Some(&failure),
                    refused.status,
                )
                .await
            }
            Some(Page::Job(vid, id)) => {
                pages::job_response(self.state, vid, id, None, Some(&failure), refused.status).await
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

fn values(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
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
    let post = Post {
        state: &state,
        vid,
        back: safe_back(form.back.as_deref()),
        target: Target::Need(id),
        form: "supplier",
        values: values(&[
            ("supplied_by", &form.supplied_by),
            ("approach", &form.approach),
        ]),
        undo: None,
    };
    let result = need::update(
        &state.db,
        post.vid,
        id,
        UpdateNeed {
            supplied_by: Some(present(&form.supplied_by)),
            approach: Some(present(&form.approach)),
            ..Default::default()
        },
    )
    .await
    .map(|_| ());
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
    let post = Post {
        state: &state,
        vid,
        back: safe_back(form.back.as_deref()),
        target: Target::Need(id),
        form: "decide",
        values: values(&[("option_id", &form.option_id), ("reason", &form.reason)]),
        undo: None,
    };
    let result = decide(&state, vid, id, &form).await;
    post.answer(result).await
}

async fn decide(state: &AppState, vid: i32, id: i32, form: &DecisionForm) -> DomainResult<()> {
    let option_id = match present(&form.option_id) {
        None => None,
        Some(text) => Some(
            text.parse::<i32>()
                .map_err(|_| DomainError::invalid("option", "choose one of the listed options"))?,
        ),
    };
    need::decide(&state.db, vid, id, option_id, present(&form.reason))
        .await
        .map(|_| ())
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
    let post = Post {
        state: &state,
        vid,
        back: safe_back(form.back.as_deref()),
        target: Target::Need(id),
        form: "order",
        values: values(&[
            ("seller", &form.seller),
            ("purchase_date", &form.purchase_date),
            ("cost", &form.cost),
            ("order_ref", &form.order_ref),
        ]),
        undo: None,
    };
    let result = order(&state, vid, id, &form).await;
    post.answer(result).await
}

async fn order(state: &AppState, vid: i32, id: i32, form: &OrderedForm) -> DomainResult<()> {
    let cost_cents = present(&form.cost)
        .map(|text| money::parse_dollars(&text))
        .transpose()?;
    need::mark_ordered(
        &state.db,
        vid,
        id,
        OrderDetails {
            seller: present(&form.seller),
            purchase_date: present(&form.purchase_date),
            cost_cents,
            order_ref: present(&form.order_ref),
        },
    )
    .await
    .map(|_| ())
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
    let post = Post {
        state: &state,
        vid,
        back: safe_back(form.back.as_deref()),
        target: Target::Need(id),
        form: "received",
        values: HashMap::new(),
        undo: Some(format!("need-{id}")),
    };
    let result = need::mark_received(&state.db, vid, id, None)
        .await
        .map(|_| ());
    post.answer(result).await
}

pub async fn post_received_undo(
    State(state): State<AppState>,
    Path((vid, id)): Path<(i32, i32)>,
    Form(form): Form<BackOnly>,
) -> Response {
    let post = Post {
        state: &state,
        vid,
        back: safe_back(form.back.as_deref()),
        target: Target::Need(id),
        form: "received_undo",
        values: HashMap::new(),
        undo: None,
    };
    let result = need::clear_received(&state.db, vid, id).await.map(|_| ());
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
    let mut typed = values(&[("reason", &form.reason)]);
    if form.option_was_wrong.is_some() {
        typed.insert("option_was_wrong".to_owned(), "true".to_owned());
    }
    let post = Post {
        state: &state,
        vid,
        back: safe_back(form.back.as_deref()),
        target: Target::Need(id),
        form: "return",
        values: typed,
        undo: None,
    };
    let result = need::return_part(
        &state.db,
        vid,
        id,
        ReturnInput {
            reason: form.reason.clone(),
            option_was_wrong: form.option_was_wrong.is_some(),
        },
    )
    .await
    .map(|_| ());
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
    let post = Post {
        state: &state,
        vid,
        back: safe_back(form.back.as_deref()),
        target: Target::TodoAdd,
        form: "todo",
        values: values(&[("title", &form.title)]),
        undo: None,
    };
    let result = add_todo(&state, vid, &form).await;
    post.answer(result).await
}

async fn add_todo(state: &AppState, vid: i32, form: &TodoForm) -> DomainResult<()> {
    let appointment_id = match present(&form.appointment_id) {
        None => None,
        Some(text) => Some(
            text.parse::<i32>()
                .map_err(|_| DomainError::invalid("appointment", "is not an appointment number"))?,
        ),
    };
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
    .map(|_| ())
}

pub async fn post_todo_done(
    State(state): State<AppState>,
    Path((vid, id)): Path<(i32, i32)>,
    Form(form): Form<BackOnly>,
) -> Response {
    let post = Post {
        state: &state,
        vid,
        back: safe_back(form.back.as_deref()),
        target: Target::Todo,
        form: "todo_done",
        values: HashMap::new(),
        undo: Some(format!("todo-{id}")),
    };
    let result = todo::complete(&state.db, vid, id, None).await.map(|_| ());
    post.answer(result).await
}

pub async fn post_todo_undo(
    State(state): State<AppState>,
    Path((vid, id)): Path<(i32, i32)>,
    Form(form): Form<BackOnly>,
) -> Response {
    let post = Post {
        state: &state,
        vid,
        back: safe_back(form.back.as_deref()),
        target: Target::Todo,
        form: "todo_undo",
        values: HashMap::new(),
        undo: None,
    };
    let result = todo::reopen(&state.db, vid, id).await.map(|_| ());
    post.answer(result).await
}

pub async fn post_todo_delete(
    State(state): State<AppState>,
    Path((vid, id)): Path<(i32, i32)>,
    Form(form): Form<BackOnly>,
) -> Response {
    let post = Post {
        state: &state,
        vid,
        back: safe_back(form.back.as_deref()),
        target: Target::Todo,
        form: "todo_delete",
        values: HashMap::new(),
        undo: None,
    };
    let result = todo::delete(&state.db, vid, id).await;
    post.answer(result).await
}
