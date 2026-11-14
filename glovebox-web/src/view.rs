//! The values the checklist templates read, built from what the shared
//! services return. A template only prints these; every decision about what
//! to show is made here.

use std::collections::HashMap;

use glovebox_shared::{
    entities::{need_option, todo},
    money::format_cents,
    services::{
        checklist::{Checklist, JobView, NeedView, StepRow},
        need::{self, Stage},
    },
};

/// Why a write failed, and what the owner had typed, so a page can show the
/// message in the row it concerns and keep the form as it was.
#[derive(Clone, Debug)]
pub struct Failure {
    /// The vehicle the write was for.
    pub vid: i32,
    pub target: Target,
    /// The form that was submitted, such as `order` or `return`.
    pub form: &'static str,
    pub message: String,
    /// The form input to mark invalid, when the error names one.
    pub field: Option<&'static str>,
    /// What each form input held when it was submitted.
    pub values: HashMap<String, String>,
}

/// The row a failure belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Need(i32),
    TodoAdd,
    /// A to-do that already exists; its error has no row of its own.
    Todo,
}

impl Failure {
    fn value(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
}

/// The undo strip shown after a tick or a "received".
pub struct Banner {
    pub message: String,
    /// Where the undo form posts.
    pub action: String,
    pub back: String,
}

/// A number with its noun, such as `2 needs open`.
pub struct Count {
    pub n: usize,
    pub text: String,
}

impl Count {
    pub fn new(n: usize, singular: &str, plural: &str) -> Self {
        Self {
            n,
            text: (if n == 1 { singular } else { plural }).to_owned(),
        }
    }
}

/// An error shown inside the form it came from.
#[derive(Clone)]
struct FormError {
    form: &'static str,
    message: String,
    field: Option<&'static str>,
}

/// One researched option as the decision form lists it.
#[derive(Clone)]
pub struct OptionCard {
    pub id: i32,
    pub manufacturer: Option<String>,
    /// The part number, or the description when the option has no number.
    pub title: String,
    /// The description, when `title` is the part number.
    pub description: Option<String>,
    pub seller: Option<String>,
    pub price: Option<String>,
    /// The option's listing, when it is an `http` or `https` address.
    pub link: Option<String>,
    pub fit_class: String,
    pub fit_text: &'static str,
    pub fit_note: Option<String>,
    pub rejected_reason: Option<String>,
    pub checked: bool,
    /// True for an option that does not fit or was rejected and is not the
    /// current choice; its radio button is disabled.
    pub blocked: bool,
}

impl OptionCard {
    fn new(option: &need_option::Model, checked: bool) -> Self {
        let rejected = option.rejected_reason.is_some() || option.rejected_on.is_some();
        let has_number = option.part_number.is_some();
        let fit_text = match option.fitment.as_str() {
            "confirmed" => "Fit confirmed",
            "does_not_fit" => "Does not fit",
            _ => "Fit unconfirmed",
        };
        Self {
            id: option.id,
            manufacturer: option.manufacturer.clone(),
            title: option
                .part_number
                .clone()
                .unwrap_or_else(|| option.description.clone()),
            description: has_number.then(|| option.description.clone()),
            seller: option.seller.clone(),
            price: option.price_cents.map(format_cents),
            link: option
                .url
                .as_deref()
                .filter(|u| is_web_address(u))
                .map(str::to_owned),
            fit_class: option.fitment.clone(),
            fit_text,
            fit_note: option.fitment_note.clone(),
            rejected_reason: rejected.then(|| option.rejected_reason.clone().unwrap_or_default()),
            checked,
            blocked: !checked && (rejected || option.fitment == "does_not_fit"),
        }
    }

    pub fn is_rejected(&self) -> bool {
        self.rejected_reason.is_some()
    }
}

// Only a web address becomes a link, so a stored `javascript:` or `data:`
// value is never something the owner can click.
fn is_web_address(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

/// What the "mark ordered" form holds.
#[derive(Clone)]
pub struct OrderValues {
    pub seller: String,
    pub date: String,
    /// The price of the whole quantity: the option's price times the need's
    /// quantity, unless the owner typed one.
    pub cost: String,
    /// The price field's label, which names the quantity when it is above 1.
    pub cost_label: String,
    /// The count and the price of one, such as `2 × $74.99`, when the need
    /// is for more than one and the option has a price.
    pub unit_note: Option<String>,
    pub order_ref: String,
}

/// One need: its stage, its part, its options and the values of its forms.
// The flags are independent facts a template tests one by one, so grouping
// them into enums would only add indirection.
#[derive(Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct NeedCard {
    pub vid: i32,
    pub id: i32,
    pub name: String,
    /// How many are needed, such as `2 needed`, when that is more than one.
    pub quantity: Option<String>,
    /// The page the forms return to.
    pub back: String,
    pub stage_class: &'static str,
    pub stage_text: &'static str,
    /// Whether the need's details start open.
    pub open: bool,
    pub chosen: Option<OptionCard>,
    /// Every option, the chosen one included, for the decision form.
    pub options: Vec<OptionCard>,
    pub none_checked: bool,
    pub options_note: String,
    supplier: String,
    approach: String,
    pub supplier_text: String,
    pub approach_text: String,
    pub supplier_set: bool,
    pub approach_set: bool,
    pub decision_reason: Option<String>,
    pub reason_value: String,
    pub purchase: Option<String>,
    pub notes: Vec<String>,
    pub order: OrderValues,
    pub return_reason: String,
    pub option_was_wrong: bool,
    pub can_decide: bool,
    pub can_order: bool,
    pub can_receive: bool,
    pub can_unreceive: bool,
    pub can_return: bool,
    error: Option<FormError>,
}

impl NeedCard {
    pub fn supplier_is(&self, value: &str) -> bool {
        self.supplier == value
    }

    pub fn approach_is(&self, value: &str) -> bool {
        self.approach == value
    }

    /// The message of a failure of `form`, when that is the form that failed.
    pub fn error_in(&self, form: &str) -> Option<&str> {
        self.error
            .as_ref()
            .filter(|e| e.form == form)
            .map(|e| e.message.as_str())
    }

    /// The message of a failure of a form that has no place for one.
    pub fn error_elsewhere(&self) -> Option<&str> {
        self.error
            .as_ref()
            .filter(|e| !["order", "return", "decide", "supplier"].contains(&e.form))
            .map(|e| e.message.as_str())
    }

    /// Whether `input` is the field the failure names.
    pub fn invalid(&self, input: &str) -> bool {
        self.error.as_ref().and_then(|e| e.field) == Some(input)
    }
}

/// A to-do row.
pub struct TodoCard {
    pub vid: i32,
    pub id: i32,
    pub title: String,
    pub back: String,
    pub done: bool,
}

/// The add-a-to-do form, with its error and what was typed.
pub struct TodoForm {
    pub error: Option<String>,
    pub title: String,
}

/// One need in a next-steps group.
pub struct StepCard {
    pub need: NeedCard,
    /// The group the row sits in: `decide`, `order`, `arriving` or `done`.
    group: &'static str,
    /// The summary without the need's name.
    pub detail: String,
    pub job_title: String,
    pub stage_class: &'static str,
    pub stage_text: &'static str,
    /// The link target of a "decide" row's button, and its label.
    pub decide_href: Option<String>,
    pub decide_label: &'static str,
}

impl StepCard {
    pub fn is(&self, group: &str) -> bool {
        self.group == group
    }
}

/// One checklist warning with the need and job it belongs to.
pub struct CheckRow {
    pub need_id: i32,
    pub name: String,
    pub note: String,
    pub job_title: String,
}

/// A job with its needs.
pub struct JobCard {
    pub vid: i32,
    pub id: i32,
    pub title: String,
    pub status_class: String,
    pub status_text: String,
    pub open: Count,
    pub needs: Vec<NeedCard>,
}

/// The grouped rows of an appointment's next steps.
pub struct Steps {
    pub check: Vec<CheckRow>,
    pub decide: Vec<StepCard>,
    pub order: Vec<StepCard>,
    pub arriving: Vec<StepCard>,
    pub done: Vec<StepCard>,
    pub todos: Vec<TodoCard>,
    pub done_todos: Vec<TodoCard>,
}

impl Steps {
    pub fn done_count(&self) -> usize {
        self.done.len() + self.done_todos.len()
    }
}

/// Capitalizes the first letter, as the stage and status tags read.
pub fn sentence_case(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map_or_default(|c| c.to_uppercase().chain(chars).collect())
}

fn stage_text(stage: Stage) -> &'static str {
    match stage {
        Stage::Undecided => "Undecided",
        Stage::ToOrder => "To order",
        Stage::ShopSupplies => "Shop supplies",
        Stage::Ordered => "Ordered",
        Stage::Received => "Received",
        Stage::Installed => "Installed",
    }
}

fn kind_text(value: Option<&str>, unset: &str) -> (String, bool) {
    match value {
        Some(v) => (sentence_case(v), true),
        None => (unset.to_owned(), false),
    }
}

/// What a need card is built from besides the need itself.
#[derive(Clone, Copy)]
pub struct CardContext<'a> {
    pub vid: i32,
    pub back: &'a str,
    pub today: &'a str,
    pub failure: Option<&'a Failure>,
    /// The need whose details start open, from the `open` query value.
    pub open: Option<i32>,
}

/// Builds the card of one need. `failure` applies when it names this need.
pub fn need_card(view: &NeedView, ctx: &CardContext<'_>) -> NeedCard {
    let need = &view.need;
    let failure = ctx.failure.filter(|f| f.target == Target::Need(need.id));
    let typed = |key: &str| failure.and_then(|f| f.value(key));

    let chosen_id = need.chosen_option_id;
    let selected: Option<i32> = match typed("option_id") {
        Some(text) => text.parse().ok(),
        None => chosen_id,
    };
    let mut all: Vec<&need_option::Model> = view.chosen.iter().chain(&view.other_options).collect();
    all.sort_by_key(|o| o.id);
    let options: Vec<OptionCard> = all
        .into_iter()
        .map(|o| OptionCard::new(o, Some(o.id) == selected))
        .collect();
    let chosen = view.chosen.as_ref().map(|o| OptionCard::new(o, true));

    let (supplier_text, supplier_set) =
        kind_text(need.supplied_by.as_deref(), "Supplier not decided");
    let (approach_text, approach_set) = kind_text(need.approach.as_deref(), "Approach not decided");

    let stage = view.stage;
    let several = need.quantity > 1;
    let order = OrderValues {
        seller: typed("seller")
            .map(str::to_owned)
            .or_else(|| chosen.as_ref().and_then(|c| c.seller.clone()))
            .unwrap_or_default(),
        date: typed("purchase_date").map_or_else(|| ctx.today.to_owned(), str::to_owned),
        cost: typed("cost")
            .map(str::to_owned)
            .or_else(|| {
                view.chosen
                    .as_ref()
                    .and_then(|c| need::order_cost_cents(need, c))
                    .map(format_cents)
            })
            .unwrap_or_default(),
        cost_label: if several {
            format!("Price for {}", need.quantity)
        } else {
            "Price".to_owned()
        },
        unit_note: chosen
            .as_ref()
            .and_then(|c| c.price.as_ref())
            .filter(|_| several)
            .map(|unit| format!("{} \u{d7} {unit}", need.quantity)),
        order_ref: typed("order_ref").unwrap_or_default().to_owned(),
    };

    NeedCard {
        vid: ctx.vid,
        id: need.id,
        name: need.name.clone(),
        quantity: several.then(|| format!("{} needed", need.quantity)),
        back: ctx.back.to_owned(),
        stage_class: stage_class(stage),
        stage_text: stage_text(stage),
        open: failure.is_some() || ctx.open == Some(need.id),
        none_checked: !options.iter().any(|o| o.checked),
        options_note: view.options_note(),
        can_decide: view.part.is_none() && !options.is_empty(),
        chosen,
        options,
        supplier: typed("supplied_by").map_or_else(
            || need.supplied_by.clone().unwrap_or_default(),
            str::to_owned,
        ),
        approach: typed("approach")
            .map_or_else(|| need.approach.clone().unwrap_or_default(), str::to_owned),
        supplier_text,
        approach_text,
        supplier_set,
        approach_set,
        decision_reason: need.decision_reason.clone(),
        reason_value: typed("reason")
            .map(str::to_owned)
            .or_else(|| need.decision_reason.clone())
            .unwrap_or_default(),
        purchase: view.purchase_summary.clone(),
        notes: view.notes.clone(),
        order,
        return_reason: if failure.is_some_and(|f| f.form == "return") {
            typed("reason").unwrap_or_default().to_owned()
        } else {
            String::new()
        },
        option_was_wrong: typed("option_was_wrong").is_some(),
        can_order: stage == Stage::ToOrder,
        can_receive: stage == Stage::Ordered,
        can_unreceive: stage == Stage::Received,
        can_return: matches!(stage, Stage::Ordered | Stage::Received),
        error: failure.map(|f| FormError {
            form: f.form,
            message: f.message.clone(),
            field: f.field,
        }),
    }
}

fn stage_class(stage: Stage) -> &'static str {
    stage.label()
}

/// Builds the card of a job from its view.
pub fn job_card(job: &JobView, ctx: &CardContext<'_>) -> JobCard {
    let needs: Vec<NeedCard> = job.needs.iter().map(|n| need_card(n, ctx)).collect();
    let open = job.open_needs();
    JobCard {
        vid: ctx.vid,
        id: job.job.id,
        title: job.job.title.clone(),
        status_class: job.job.status.clone(),
        status_text: sentence_case(&job.job.status),
        open: Count::new(open, "need open", "needs open"),
        needs,
    }
}

/// Splits a checklist into the groups the appointment page prints.
pub fn steps(list: &Checklist, jobs: &[JobCard], ctx: &CardContext<'_>) -> Steps {
    let cards: HashMap<i32, &NeedCard> = jobs
        .iter()
        .flat_map(|j| &j.needs)
        .map(|n| (n.id, n))
        .collect();
    let step = |group: &'static str, row: &StepRow| -> Option<StepCard> {
        let need = (*cards.get(&row.need_id)?).clone();
        let id = row.need_id;
        let (decide_href, decide_label) = if need.chosen.is_some() {
            (
                Some(format!("?open=need-{id}#need-{id}-supplier")),
                "Set the supplier",
            )
        } else if need.options.is_empty() {
            (None, "")
        } else {
            (
                Some(format!("?open=need-{id}#need-{id}-decide")),
                "Choose a part",
            )
        };
        Some(StepCard {
            group,
            detail: row.detail().to_owned(),
            job_title: row.job_title.clone(),
            stage_class: need.stage_class,
            stage_text: need.stage_text,
            decide_href,
            decide_label,
            need,
        })
    };
    let group = |name: &'static str, rows: &[StepRow]| {
        rows.iter()
            .filter_map(|r| step(name, r))
            .collect::<Vec<_>>()
    };

    let check = list
        .checks
        .iter()
        .map(|c| CheckRow {
            need_id: c.need_id,
            name: c.need_name.clone(),
            note: c.note.clone(),
            job_title: c.job_title.clone(),
        })
        .collect();

    let todo_card = |t: &todo::Model| TodoCard {
        vid: ctx.vid,
        id: t.id,
        title: t.title.clone(),
        back: ctx.back.to_owned(),
        done: t.done_on.is_some(),
    };
    Steps {
        check,
        decide: group("decide", &list.next_steps.decide),
        order: group("order", &list.next_steps.order),
        arriving: group("arriving", &list.next_steps.arriving),
        done: group("done", &list.next_steps.done),
        todos: list.next_steps.todos.iter().map(todo_card).collect(),
        done_todos: list
            .todos
            .iter()
            .filter(|t| t.done_on.is_some())
            .map(todo_card)
            .collect(),
    }
}
