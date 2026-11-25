//! Inputs for the maintenance schedule tools.

use glovebox_shared::inputs::schedule::{NewScheduleItem, UpdateScheduleItem};
use schemars::JsonSchema;
use serde::Deserialize;

use super::nullable;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DismissScheduleItemInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Schedule item id, from `whats_coming`.
    pub schedule_item_id: i32,
    /// Why it's being waived, e.g. "independent shop handles this".
    pub reason: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreScheduleItemInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// The dismissed item's id: the `id` `dismiss_schedule_item` returned, or
    /// an item with `enabled: false` in `list_schedule` with
    /// `include_dismissed`.
    pub schedule_item_id: i32,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListScheduleInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Also list dismissed items, shown with `enabled: false`. Default false.
    pub include_dismissed: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddScheduleItemInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Item name, e.g. 'Smog check'.
    pub name: String,
    /// Whether the manufacturer recommends it.
    pub is_factory_recommended: Option<bool>,
    /// What the item covers.
    pub description: Option<String>,
    /// Repeat every this many miles. Must be greater than zero.
    pub interval_miles: Option<i32>,
    /// Repeat every this many months. Must be greater than zero.
    pub interval_months: Option<i32>,
    /// Start warning this many miles before due. Must be zero or more. Defaults to 1000.
    pub warning_miles: Option<i32>,
    /// Start warning this many days before due. Must be zero or more. Defaults to 30.
    pub warning_days: Option<i32>,
    /// Where the interval comes from, e.g. 'owner manual'.
    pub source: Option<String>,
    /// Free-form notes.
    pub notes: Option<String>,
    /// Estimated cost per occurrence, integer cents.
    pub est_cost_cents: Option<i32>,
}

impl AddScheduleItemInput {
    pub fn into_domain(self) -> (i32, NewScheduleItem) {
        (
            self.vehicle_id,
            NewScheduleItem {
                name: self.name,
                is_factory_recommended: self.is_factory_recommended,
                description: self.description,
                interval_miles: self.interval_miles,
                interval_months: self.interval_months,
                warning_miles: self.warning_miles,
                warning_days: self.warning_days,
                source: self.source,
                notes: self.notes,
                est_cost_cents: self.est_cost_cents,
                ..Default::default()
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateScheduleItemInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Schedule item id, from `list_schedule` or `whats_coming`. An inherited
    /// item's id and the id of this car's copy of it reach the same item.
    pub schedule_item_id: i32,
    /// New name. Only an item added with `add_schedule_item` can be renamed;
    /// an inherited item keeps its name.
    pub name: Option<String>,
    /// `false` stops the item appearing in `whats_coming`.
    pub enabled: Option<bool>,
    /// Whether the manufacturer recommends it.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<bool>")]
    pub is_factory_recommended: Option<Option<bool>>,
    /// What the item covers.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub description: Option<Option<String>>,
    /// Repeat every this many miles. Must be greater than zero.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub interval_miles: Option<Option<i32>>,
    /// Repeat every this many months. Must be greater than zero.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub interval_months: Option<Option<i32>>,
    /// Start warning this many miles before due. Must be zero or more.
    ///
    /// Send `null` to restore the default of 1000; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub warning_miles: Option<Option<i32>>,
    /// Start warning this many days before due. Must be zero or more.
    ///
    /// Send `null` to restore the default of 30; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub warning_days: Option<Option<i32>>,
    /// Where the interval comes from, e.g. 'owner manual'.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub source: Option<Option<String>>,
    /// Free-form notes.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub notes: Option<Option<String>>,
    /// Estimated cost per occurrence, integer cents.
    ///
    /// Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<i32>")]
    pub est_cost_cents: Option<Option<i32>>,
}

impl UpdateScheduleItemInput {
    pub fn into_domain(self) -> (i32, i32, UpdateScheduleItem) {
        (
            self.vehicle_id,
            self.schedule_item_id,
            UpdateScheduleItem {
                name: self.name,
                enabled: self.enabled,
                is_factory_recommended: self.is_factory_recommended,
                description: self.description,
                interval_miles: self.interval_miles,
                interval_months: self.interval_months,
                warning_miles: self.warning_miles,
                warning_days: self.warning_days,
                source: self.source,
                notes: self.notes,
                est_cost_cents: self.est_cost_cents,
                ..Default::default()
            },
        )
    }
}
