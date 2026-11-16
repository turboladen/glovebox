use crate::entities::{job_need::Approach, need_option::Fitment, part::Supplier};

/// A need to add to a job.
#[derive(Default)]
pub struct NewNeed {
    pub name: String,
    /// How many are needed; defaults to 1 and must be at least 1.
    pub quantity: Option<i32>,
    /// Who supplies it. `None` means the owner has not decided.
    pub supplied_by: Option<Supplier>,
    /// How it is filled. `None` means the owner has not decided.
    pub approach: Option<Approach>,
}

/// Changes to a need. An absent field is left alone; an explicit `None` in a
/// double option clears the column.
#[derive(Default)]
pub struct UpdateNeed {
    pub name: Option<String>,
    pub quantity: Option<i32>,
    pub supplied_by: Option<Option<Supplier>>,
    pub approach: Option<Option<Approach>>,
    pub decision_reason: Option<Option<String>>,
}

/// A researched candidate for a need.
#[derive(Default)]
pub struct NewOption {
    pub description: String,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub seller: Option<String>,
    pub price_cents: Option<i32>,
    pub url: Option<String>,
    /// Defaults to `unconfirmed`.
    pub fitment: Option<Fitment>,
    pub fitment_note: Option<String>,
}

/// Changes to an option. An absent field is left alone; an explicit `None` in
/// a double option clears the column.
#[derive(Default)]
pub struct UpdateOption {
    pub description: Option<String>,
    pub manufacturer: Option<Option<String>>,
    pub part_number: Option<Option<String>>,
    pub seller: Option<Option<String>>,
    pub price_cents: Option<Option<i32>>,
    pub url: Option<Option<String>>,
    pub fitment: Option<Fitment>,
    pub fitment_note: Option<Option<String>>,
    /// Setting a reason rejects the option and stamps the rejection date;
    /// clearing it un-rejects the option.
    pub rejected_reason: Option<Option<String>>,
}

/// What the owner knows about an order when marking a need ordered. A field
/// left out takes the chosen option's value, or today for the date.
#[derive(Default)]
pub struct OrderDetails {
    pub seller: Option<String>,
    /// The order's date; defaults to today.
    pub purchase_date: Option<chrono::NaiveDate>,
    pub cost_cents: Option<i32>,
    pub order_ref: Option<String>,
}

/// Why a part is going back.
pub struct ReturnInput {
    pub reason: String,
    /// True when the chosen option itself was the mistake, so the option is
    /// rejected for the same reason and the need goes back to undecided.
    pub option_was_wrong: bool,
}

/// A change to the part linked to a need.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartLink {
    /// Link a part that is already recorded, stamping the day it arrived when
    /// `received_on` is given.
    Link {
        part_id: i32,
        received_on: Option<chrono::NaiveDate>,
    },
    /// Detach the need's current part and keep the part.
    Unlink,
}

/// A change to the arrival date of a need's part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrivalChange {
    /// The part arrived on this day, or today when it is `None`.
    Received(Option<chrono::NaiveDate>),
    /// The part has not arrived; its arrival date is cleared.
    NotReceived,
}
