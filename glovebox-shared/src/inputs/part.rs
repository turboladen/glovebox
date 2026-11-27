use chrono::NaiveDate;

use crate::{
    entities::part::{PartStatus, Supplier},
    error::{DomainError, DomainResult},
};

/// The service, date and odometer reading a part was installed with. Each
/// may be unknown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Install {
    pub service_id: Option<i32>,
    pub date: Option<NaiveDate>,
    pub odometer: Option<i32>,
}

impl Install {
    pub(crate) fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// These details with each field `patch` sends put in place.
    pub(crate) fn patched(self, patch: InstallPatch) -> Self {
        Self {
            service_id: patch.service_id.unwrap_or(self.service_id),
            date: patch.date.unwrap_or(self.date),
            odometer: patch.odometer.unwrap_or(self.odometer),
        }
    }
}

/// Install details a correction sends: an absent field keeps the stored
/// value, and `Some(None)` clears it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InstallPatch {
    pub service_id: Option<Option<i32>>,
    pub date: Option<Option<NaiveDate>>,
    pub odometer: Option<Option<i32>>,
}

impl InstallPatch {
    pub(crate) fn sets_any(&self) -> bool {
        matches!(self.service_id, Some(Some(_)))
            || matches!(self.date, Some(Some(_)))
            || matches!(self.odometer, Some(Some(_)))
    }
}

/// Replacement details a correction sends, read as [`InstallPatch`] is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReplacePatch {
    pub date: Option<Option<NaiveDate>>,
    pub odometer: Option<Option<i32>>,
}

impl ReplacePatch {
    pub(crate) fn sets_any(&self) -> bool {
        matches!(self.date, Some(Some(_))) || matches!(self.odometer, Some(Some(_)))
    }
}

/// Where a correction puts a part: its status, with the details that status
/// takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fitting {
    /// The status stays, and the details change the ones the part holds.
    Keep(InstallPatch, ReplacePatch),
    /// Off the car, or back from a return.
    Purchased,
    /// On the car. The replacement details hold only the ones the change
    /// clears, which a replaced part needs to become installed again.
    Installed(InstallPatch, ReplacePatch),
    Replaced(InstallPatch, ReplacePatch),
}

impl Default for Fitting {
    fn default() -> Self {
        Self::Keep(InstallPatch::default(), ReplacePatch::default())
    }
}

/// The message for a status of `returned`, which only returning a part from
/// its need sets.
fn returned_refused() -> DomainError {
    DomainError::BadRequest(
        "A part is marked returned by returning it from its need (return_part)".into(),
    )
}

impl Fitting {
    /// The change a correction's status and details describe.
    ///
    /// # Errors
    /// `BadRequest` for the status `returned`, for install or replacement
    /// details sent with `purchased`, and for replacement details sent with
    /// `installed`. Clearing a detail is never refused here.
    pub fn from_fields(
        status: Option<PartStatus>,
        install: InstallPatch,
        replace: ReplacePatch,
    ) -> DomainResult<Self> {
        match status {
            None => Ok(Self::Keep(install, replace)),
            Some(PartStatus::Returned) => Err(returned_refused()),
            Some(PartStatus::Purchased) if install.sets_any() || replace.sets_any() => {
                Err(DomainError::BadRequest(
                    "A part cannot go back to purchased and be given install or replacement \
                     details at once"
                        .into(),
                ))
            }
            Some(PartStatus::Purchased) => Ok(Self::Purchased),
            Some(PartStatus::Installed) if replace.sets_any() => Err(DomainError::BadRequest(
                "An installed part takes no replacement details; set its status to replaced with \
                 them"
                    .into(),
            )),
            Some(PartStatus::Installed) => Ok(Self::Installed(install, replace)),
            Some(PartStatus::Replaced) => Ok(Self::Replaced(install, replace)),
        }
    }

    /// The installing service the change names, if any.
    pub(crate) fn service_id(&self) -> Option<i32> {
        match self {
            Self::Keep(install, _) | Self::Installed(install, _) | Self::Replaced(install, _) => {
                install.service_id.flatten()
            }
            Self::Purchased => None,
        }
    }
}

/// Where a new part starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NewFitting {
    #[default]
    Purchased,
    Installed(Install),
    /// Already off the car, with the details of the install it came off.
    Replaced(Install),
}

impl NewFitting {
    /// The state a new part's status and install details describe, with
    /// `purchased` when no status is given.
    ///
    /// # Errors
    /// `BadRequest` for the status `returned`, and for install details on a
    /// part that is purchased.
    pub fn from_fields(status: Option<PartStatus>, install: Install) -> DomainResult<Self> {
        match status.unwrap_or(PartStatus::Purchased) {
            PartStatus::Returned => Err(returned_refused()),
            PartStatus::Purchased if !install.is_empty() => Err(DomainError::BadRequest(
                "A purchased part takes no install details; record it as installed with them"
                    .into(),
            )),
            PartStatus::Purchased => Ok(Self::Purchased),
            PartStatus::Installed => Ok(Self::Installed(install)),
            PartStatus::Replaced => Ok(Self::Replaced(install)),
        }
    }
}

#[derive(Default)]
pub struct NewPart {
    pub name: String,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub oe_part_number_replaced: Option<String>,
    pub seller: Option<String>,
    pub purchase_date: Option<chrono::NaiveDate>,
    pub cost_cents: Option<i32>,
    pub manufacturer_url: Option<String>,
    pub retailer_url: Option<String>,
    /// The status and install details it starts with.
    pub fitting: NewFitting,
    pub notes: Option<String>,
    pub location: Option<String>,
    /// Warranty expiry: date and/or mileage.
    pub warranty_expires_on: Option<chrono::NaiveDate>,
    pub warranty_expires_miles: Option<i32>,
    /// Defaults to the owner.
    pub supplied_by: Option<Supplier>,
    /// The seller's order or confirmation number.
    pub order_ref: Option<String>,
    /// A need this part fills, honored only for a part created inline on a
    /// service invoice (`NewServiceRecord::parts`): the part is linked to the
    /// need in the same transaction. The need must be on the same vehicle and
    /// have no part linked. Plain part creation ignores this field.
    pub need_id: Option<i32>,
    /// The items inside a part bought as a kit, in the order they print.
    pub contents: Vec<NewPartContent>,
    /// What the part replaces, so its install starts a new wear series.
    pub wears_as: Option<crate::inputs::wear::WearTag>,
}

/// One item inside a kit, with the number printed on its own packaging.
#[derive(Clone, Debug, Default)]
pub struct NewPartContent {
    pub name: String,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    /// Defaults to 1.
    pub quantity: Option<i32>,
}

#[derive(Default)]
pub struct UpdatePart {
    pub name: Option<String>,
    pub manufacturer: Option<Option<String>>,
    pub part_number: Option<Option<String>>,
    pub oe_part_number_replaced: Option<Option<String>>,
    pub seller: Option<Option<String>>,
    pub purchase_date: Option<Option<chrono::NaiveDate>>,
    pub cost_cents: Option<Option<i32>>,
    pub manufacturer_url: Option<Option<String>>,
    pub retailer_url: Option<Option<String>>,
    /// The status and the install and replacement details.
    pub fitting: Fitting,
    pub notes: Option<Option<String>>,
    pub location: Option<Option<String>>,
    pub warranty_expires_on: Option<Option<chrono::NaiveDate>>,
    pub warranty_expires_miles: Option<Option<i32>>,
    /// Plain option: the column is non-nullable, so there is no "clear" case.
    pub supplied_by: Option<Supplier>,
    pub order_ref: Option<Option<String>>,
    /// The date the part arrived.
    pub received_on: Option<Option<chrono::NaiveDate>>,
    /// Kit items to add after the ones the kit already holds.
    pub add_contents: Vec<NewPartContent>,
    /// Ids of the kit items to remove.
    pub remove_contents: Vec<i32>,
    /// What the part replaces; `Some(None)` clears the tag.
    pub wears_as: Option<Option<crate::inputs::wear::WearTag>>,
}

/// Filter for listing parts within a vehicle.
#[derive(Default)]
pub struct PartFilter {
    pub status: Option<PartStatus>,
    /// `Some(true)` keeps the parts under warranty, as
    /// `services::part::is_under_warranty` judges them, and `Some(false)`
    /// keeps every other part.
    pub under_warranty: Option<bool>,
    /// The most parts returned, counted from the newest.
    pub limit: Option<u32>,
}
