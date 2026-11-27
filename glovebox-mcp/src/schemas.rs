//! Tool input/output shapes for the MCP surface, one module per handler area.
//!
//! Inputs derive `Deserialize` + `JsonSchema`; doc comments become field
//! descriptions in the advertised schema, so write them for an LLM. Each
//! input maps 1:1 onto a `glovebox_shared::inputs` struct (fields the MCP
//! surface doesn't expose are filled with `None`) — no business logic here.
//!
//! A field typed `Option<Option<T>>` distinguishes three cases: an absent key
//! leaves the stored value alone, an explicit JSON `null` clears it, and a
//! value sets it.

use glovebox_shared::{
    entities::wear_reading::{Axle, LengthUnit, WearComponent},
    error::{DomainError, DomainResult},
    inputs::{part::NewPartContent, shop::ShopChoice, wear::WearTag},
    units::Measure,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

// The modules that hold a correct or update input use `Option<Option<T>>` to
// tell an absent key from an explicit `null`.
#[allow(clippy::option_option)]
mod checklist;
#[allow(clippy::option_option)]
mod incidents;
#[allow(clippy::option_option)]
mod parts;
#[allow(clippy::option_option)]
mod planning;
mod reads;
#[allow(clippy::option_option)]
mod recalls;
#[allow(clippy::option_option)]
mod schedule;
#[allow(clippy::option_option)]
mod services;
#[allow(clippy::option_option)]
mod shops;
#[allow(clippy::option_option)]
mod vehicles;
#[allow(clippy::option_option)]
mod wear;

pub use checklist::*;
pub use incidents::*;
pub use parts::*;
pub use planning::*;
pub use reads::*;
pub use recalls::*;
pub use schedule::*;
pub use services::*;
pub use shops::*;
pub use vehicles::*;
pub use wear::*;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyParams {}

const BOTH_SHOP_FORMS: &str = "Name the shop by shop_id or by shop_name, not both";

/// The shop a new record names, from its two arguments.
pub(crate) fn shop_choice(
    shop_id: Option<i32>,
    shop_name: Option<String>,
) -> DomainResult<Option<ShopChoice>> {
    match (shop_id, shop_name) {
        (Some(_), Some(_)) => Err(DomainError::BadRequest(BOTH_SHOP_FORMS.into())),
        (Some(id), None) => Ok(Some(ShopChoice::Id(id))),
        (None, name) => Ok(name.map(ShopChoice::Name)),
    }
}

/// The change to a record's shop, from its two arguments: `null` clears it,
/// and a value in one beside anything in the other is refused, unless both
/// are `null`.
#[expect(
    clippy::option_option,
    reason = "an absent change differs from clearing"
)]
pub(crate) fn shop_change(
    shop_id: Option<Option<i32>>,
    shop_name: Option<Option<String>>,
) -> DomainResult<Option<Option<ShopChoice>>> {
    match (shop_id, shop_name) {
        (None, None) => Ok(None),
        (Some(None), None | Some(None)) | (None, Some(None)) => Ok(Some(None)),
        (Some(Some(id)), None) => Ok(Some(Some(ShopChoice::Id(id)))),
        (None, Some(Some(name))) => Ok(Some(Some(ShopChoice::Name(name)))),
        (Some(_), Some(_)) => Err(DomainError::BadRequest(BOTH_SHOP_FORMS.into())),
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VehicleParams {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
}

/// One item inside a kit, such as the filter in an oil change kit.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PartContentInput {
    /// What the item is, e.g. "Oil filter".
    pub name: String,
    /// Manufacturer/brand, e.g. "Mann".
    pub manufacturer: Option<String>,
    /// The number printed on the item's own box or label, e.g. "HU 7020 z".
    pub part_number: Option<String>,
    /// How many of it one kit holds, whatever the need's quantity. Defaults
    /// to 1.
    pub quantity: Option<i32>,
    /// The `id` an item carries in a reply. It is ignored, so an item copied
    /// from a reply can be sent as it is; a new item gets a new id.
    pub id: Option<i32>,
}

/// A kit's items as the shared service takes them; none when the argument
/// is left out.
fn kit_contents(items: Option<Vec<PartContentInput>>) -> Vec<NewPartContent> {
    items.into_iter().flatten().map(Into::into).collect()
}

impl From<PartContentInput> for NewPartContent {
    fn from(item: PartContentInput) -> Self {
        NewPartContent {
            name: item.name,
            manufacturer: item.manufacturer,
            part_number: item.part_number,
            quantity: item.quantity,
        }
    }
}

/// Deserializes a present key into `Some(value)`, so that `null` becomes
/// `Some(None)` and only an absent key stays `None` (via `serde(default)`).
#[expect(
    clippy::option_option,
    reason = "an absent key differs from a null one"
)]
fn nullable<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// A length as the shop wrote it, such as 4 mm or 7/32 in.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeasureInput {
    /// The number the shop wrote: `4` for 4 mm, `7` for 7/32 in.
    pub value: f64,
    /// `mm`, `in_32` for thirty-seconds of an inch (tread gauges), or `in`.
    pub unit: LengthUnit,
    /// The `text` a reply carries beside the value. It is ignored, so a
    /// measure copied from a reply can be sent as it is.
    pub text: Option<String>,
}

impl From<MeasureInput> for Measure {
    fn from(m: MeasureInput) -> Self {
        Measure::new(m.value, m.unit)
    }
}

/// What a part replaces, so its install starts a new wear series.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WearTagInput {
    /// `brake_pads`, `brake_rotors` or `tire_tread`.
    pub component: WearComponent,
    /// `front` or `rear` for brake pads and rotors. Leave it out for tires,
    /// which are tracked as one set.
    pub axle: Option<Axle>,
}

impl WearTagInput {
    pub(crate) fn into_domain(self) -> DomainResult<WearTag> {
        WearTag::new("wears_as", self.component, self.axle)
    }
}
