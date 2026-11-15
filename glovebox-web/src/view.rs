//! The values the checklist templates read, built from what the shared
//! services return. A template only prints these; every decision about what
//! to show is made here.

use std::collections::HashMap;

use glovebox_shared::{
    entities::{need_option, need_option::Fitment, part, todo},
    money::format_cents,
    services::{
        checklist::{self, Checklist, HomeAppointment, JobView, NeedView, StepRow},
        date_label,
        need::{self, Stage},
        reminders,
        waiting::{Waiting, booked_note},
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
    pub fit_class: &'static str,
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
        let rejected = need::is_rejected(option);
        let has_number = option.part_number.is_some();
        let fit_text = match option.fitment {
            Fitment::Confirmed => "Confirmed",
            Fitment::DoesNotFit => "Does not fit",
            Fitment::Unconfirmed => "Unconfirmed",
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
            fit_class: option.fitment.as_str(),
            fit_text,
            fit_note: option.fitment_note.clone(),
            rejected_reason: rejected.then(|| option.rejected_reason.clone().unwrap_or_default()),
            checked,
            blocked: !checked && (rejected || option.fitment == Fitment::DoesNotFit),
        }
    }
}

/// The part linked to a need, as the row's part columns show it.
#[derive(Clone)]
pub struct LinkedPart {
    pub manufacturer: Option<String>,
    /// The part number, or the part's name when it has no number.
    pub title: String,
    /// The part's name, when `title` is the part number.
    pub description: Option<String>,
    pub seller: Option<String>,
    /// What the whole purchase cost.
    pub price: Option<String>,
}

impl LinkedPart {
    // Returns the need's part when no option is chosen, because a chosen
    // option fills the row's part columns itself.
    fn of(view: &NeedView) -> Option<Self> {
        view.part
            .as_ref()
            .filter(|_| view.chosen.is_none())
            .map(Self::new)
    }

    fn new(part: &part::Model) -> Self {
        Self {
            manufacturer: part.manufacturer.clone(),
            title: part
                .part_number
                .clone()
                .unwrap_or_else(|| part.name.clone()),
            description: part.part_number.is_some().then(|| part.name.clone()),
            seller: part.seller.clone(),
            price: part.cost_cents.map(format_cents),
        }
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
    /// How many are needed, such as `× 2`, when that is more than one.
    pub quantity: Option<String>,
    /// The page the forms return to.
    pub back: String,
    pub stage_class: &'static str,
    pub stage_text: &'static str,
    /// What a row with no chosen option and no linked part shows in place of
    /// the part: the sentence the need's row under Next steps shows, then the
    /// options note when the need has options.
    pub status_line: String,
    /// Whether the need's details start open.
    pub open: bool,
    pub chosen: Option<OptionCard>,
    /// The linked part, when the need has one and no chosen option. The row
    /// then shows the part where it would show the option.
    pub linked: Option<LinkedPart>,
    /// Every option, the chosen one included, for the decision form.
    pub options: Vec<OptionCard>,
    pub none_checked: bool,
    supplier: String,
    approach: String,
    /// The stored supplier and approach as their tags read, when decided.
    pub supplier_kind: Option<String>,
    pub approach_kind: Option<String>,
    pub decision_reason: Option<String>,
    pub reason_value: String,
    pub purchase: Option<String>,
    /// The need's notes, without the one `no_decision` stands for.
    pub notes: Vec<String>,
    /// True when a part is linked and no decision is recorded. The row shows
    /// it as a pill.
    pub no_decision: bool,
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

    pub fn supplier_text(&self) -> &str {
        self.supplier_kind.as_deref().unwrap_or("Not decided")
    }

    pub fn approach_text(&self) -> &str {
        self.approach_kind.as_deref().unwrap_or("Not decided")
    }

    /// The message of a failure of `form`, when that is the form that failed.
    pub fn error_in(&self, form: &str) -> Option<&str> {
        self.error
            .as_ref()
            .filter(|e| e.form == form)
            .map(|e| e.message.as_str())
    }

    /// The message of a failure that no form on the card shows: the form has
    /// no place for one, or the need has moved on and the card leaves that
    /// form out (an order lost to another order, a return of a part that is
    /// already gone).
    pub fn error_elsewhere(&self) -> Option<&str> {
        self.error
            .as_ref()
            .filter(|e| !self.shows_form(e.form))
            .map(|e| e.message.as_str())
    }

    // Returns true when the card renders `form`, which then shows its own error.
    fn shows_form(&self, form: &str) -> bool {
        match form {
            "order" => self.can_order,
            "return" => self.can_return,
            "decide" => self.can_decide,
            "supplier" => true,
            _ => false,
        }
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

impl TodoForm {
    /// The form as `failure` left it, when the failure is a to-do add, or
    /// empty otherwise.
    pub fn after(failure: Option<&Failure>) -> Self {
        let own = failure.filter(|f| f.target == Target::TodoAdd);
        Self {
            error: own.map(|f| f.message.clone()),
            title: own
                .and_then(|f| f.value("title"))
                .map_or_default(str::to_owned),
        }
    }
}

/// One need in a next-steps group.
pub struct StepCard {
    pub need: NeedCard,
    /// The group the row sits in: `decide`, `order`, `arriving` or `done`.
    group: &'static str,
    /// The summary without the need's name.
    pub detail: String,
    pub job_title: String,
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
    pub status_class: &'static str,
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

/// Describes an open appointment as the home page lists it.
pub struct HomeAppointmentRow {
    pub id: i32,
    pub purpose: String,
    pub shop_name: Option<String>,
    /// The planned date, or the words the shared row gives when there is none.
    pub when: String,
    pub booked: bool,
    pub status_class: &'static str,
    pub status_text: String,
    pub jobs: usize,
    pub needs_open: usize,
    pub todos_open: usize,
}

impl From<&HomeAppointment> for HomeAppointmentRow {
    fn from(a: &HomeAppointment) -> Self {
        Self {
            id: a.id,
            purpose: a.purpose.clone(),
            shop_name: a.shop_name.clone(),
            when: a.when.clone(),
            booked: a.booked,
            status_class: a.status,
            status_text: sentence_case(a.status),
            jobs: a.jobs,
            needs_open: a.needs_open,
            todos_open: a.todos_open,
        }
    }
}

/// A tag for a status: the class that picks its tint and mark, and its words.
pub struct Tag {
    pub class: String,
    pub text: String,
}

impl Tag {
    /// Reads `overdue` as "Overdue" and `due_soon` as "Due soon".
    fn of(status: &str) -> Self {
        Self {
            class: status.to_owned(),
            text: sentence_case(&status.replace('_', " ")),
        }
    }
}

/// A schedule item that is due, as the home page lists it.
pub struct DueRow {
    pub name: String,
    pub status: Tag,
    pub due_at: String,
    pub how_far: String,
    pub last_done: Option<String>,
    pub job_id: Option<i32>,
}

/// A planned job on no appointment, as the home page lists it.
pub struct WaitingJobRow {
    pub id: i32,
    pub title: String,
    /// The revisit tag, for a job whose revisit is overdue or due soon.
    pub revisit: Option<Tag>,
    pub estimate: Option<String>,
    pub needs_open: usize,
}

/// An open recall, as the home page lists it.
pub struct RecallRow {
    pub title: String,
    pub campaign: Option<String>,
    pub job_id: Option<i32>,
}

/// An open incident, as the home page lists it.
pub struct IncidentRow {
    pub title: String,
    pub category: String,
    pub date: String,
    pub job_id: Option<i32>,
}

/// What is waiting on one vehicle, as the home page lists it.
pub struct WaitingRows {
    /// The odometer reading the maintenance was judged against.
    pub judged_at: String,
    pub due: Vec<DueRow>,
    /// Says how many due items are left out, when any are.
    pub due_note: Option<String>,
    pub jobs: Vec<WaitingJobRow>,
    pub recalls: Vec<RecallRow>,
    /// Says how many open recalls are left out, when any are.
    pub recalls_note: Option<String>,
    pub incidents: Vec<IncidentRow>,
    /// Says how many open incidents are left out, when any are.
    pub incidents_note: Option<String>,
}

impl From<&Waiting> for WaitingRows {
    fn from(w: &Waiting) -> Self {
        Self {
            judged_at: reminders::judged_at(w.estimated_mileage, w.mileage_is_estimate),
            due_note: booked_note(w.maintenance_booked),
            recalls_note: booked_note(w.recalls_booked),
            incidents_note: booked_note(w.incidents_booked),
            due: w
                .maintenance
                .iter()
                .map(|m| DueRow {
                    name: m.name.clone(),
                    status: Tag::of(&m.status),
                    due_at: reminders::due_at(m.due_at_miles, m.due_at_date),
                    how_far: m.how_far.clone(),
                    last_done: m.last_done.map(date_label),
                    job_id: m.job_id,
                })
                .collect(),
            jobs: w
                .jobs
                .iter()
                .map(|j| WaitingJobRow {
                    id: j.id,
                    title: j.title.clone(),
                    revisit: j
                        .revisit_status
                        .filter(|status| *status != "ok")
                        .map(Tag::of),
                    estimate: j.est_cost_cents.map(format_cents),
                    needs_open: j.needs_open,
                })
                .collect(),
            recalls: w
                .recalls
                .iter()
                .map(|r| RecallRow {
                    title: r.title.clone(),
                    campaign: r.campaign_number.clone(),
                    job_id: r.job_id,
                })
                .collect(),
            incidents: w
                .incidents
                .iter()
                .map(|i| IncidentRow {
                    title: i.title.clone(),
                    category: sentence_case(&i.category.as_str().replace('_', " ")),
                    date: date_label(i.occurred_at.day()),
                    job_id: i.job_id,
                })
                .collect(),
        }
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

// Splits the need's notes into the missing-decision note, which the row shows
// as a pill, and the notes it prints as sentences.
fn split_notes(view: &NeedView) -> (Vec<String>, Vec<String>) {
    view.notes
        .iter()
        .cloned()
        .partition(|n| n == checklist::NO_DECISION_NOTE)
}

/// Builds the card of one need. `failure` applies when it names this need.
pub fn need_card(view: &NeedView, ctx: &CardContext<'_>) -> NeedCard {
    let need = &view.need;
    let failure = ctx.failure.filter(|f| f.target == Target::Need(need.id));
    let typed = |key: &str| failure.and_then(|f| f.value(key));
    let supplier = need
        .supplied_by
        .map(glovebox_shared::entities::part::Supplier::as_str);
    let approach = need
        .approach
        .map(glovebox_shared::entities::job_need::Approach::as_str);

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

    let (unrecorded, notes) = split_notes(view);
    let mut status_line = checklist::step_detail(view);
    if !options.is_empty() {
        status_line = format!("{status_line}, {}", view.options_note());
    }
    NeedCard {
        vid: ctx.vid,
        id: need.id,
        name: need.name.clone(),
        quantity: several.then(|| format!("\u{d7} {}", need.quantity)),
        status_line,
        back: ctx.back.to_owned(),
        stage_class: stage.label(),
        stage_text: stage_text(stage),
        open: failure.is_some() || ctx.open == Some(need.id),
        none_checked: !options.iter().any(|o| o.checked),
        can_decide: view.part.is_none() && !options.is_empty(),
        linked: LinkedPart::of(view),
        chosen,
        options,
        supplier: form_text(typed("supplied_by"), supplier),
        approach: form_text(typed("approach"), approach),
        supplier_kind: supplier.map(sentence_case),
        approach_kind: approach.map(sentence_case),
        decision_reason: need.decision_reason.clone(),
        // The return form also has a `reason` input, so only a failed
        // decision brings back what was typed here.
        reason_value: typed("reason")
            .filter(|_| failure.is_some_and(|f| f.form == "decide"))
            .map(str::to_owned)
            .or_else(|| need.decision_reason.clone())
            .unwrap_or_default(),
        purchase: view.purchase_summary.clone(),
        notes,
        no_decision: !unrecorded.is_empty(),
        order,
        return_reason: if failure.is_some_and(|f| f.form == "return") {
            typed("reason").unwrap_or_default().to_owned()
        } else {
            String::new()
        },
        option_was_wrong: typed("option_was_wrong").is_some(),
        can_order: stage.can_order(),
        can_receive: stage.can_receive(),
        can_unreceive: stage.can_unreceive(),
        can_return: stage.can_return(),
        error: failure.map(|f| FormError {
            form: f.form,
            message: f.message.clone(),
            field: f.field,
        }),
    }
}

/// What a select shows: the text the owner sent when the form failed, else
/// the stored value, else nothing.
fn form_text(typed: Option<&str>, stored: Option<&str>) -> String {
    typed.or(stored).unwrap_or_default().to_owned()
}

/// Builds the card of a job from its view.
pub fn job_card(job: &JobView, ctx: &CardContext<'_>) -> JobCard {
    let needs: Vec<NeedCard> = job.needs.iter().map(|n| need_card(n, ctx)).collect();
    let open = job.open_needs();
    JobCard {
        vid: ctx.vid,
        id: job.job.id,
        title: job.job.title.clone(),
        status_class: job.job.status.as_str(),
        status_text: sentence_case(job.job.status.as_str()),
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
        // The link names the page's own path, because a failed write shows
        // the page at the form's address, where a bare `?open=` would lead.
        let back = ctx.back;
        let (decide_href, decide_label) = if need.chosen.is_some() {
            (
                Some(format!("{back}?open=need-{id}#need-{id}-supplier")),
                "Set the supplier",
            )
        } else if need.options.is_empty() {
            (None, "")
        } else {
            (
                Some(format!("{back}?open=need-{id}#need-{id}-decide")),
                "Choose a part",
            )
        };
        Some(StepCard {
            group,
            detail: row.detail.clone(),
            job_title: row.job_title.clone(),
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

    // A missing decision is a fact about the record that the need's own row
    // states, so the group keeps to what could go wrong at the shop.
    let check = list
        .checks
        .iter()
        .filter(|c| c.note != checklist::NO_DECISION_NOTE)
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
