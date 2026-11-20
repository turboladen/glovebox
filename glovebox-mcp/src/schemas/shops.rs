//! Inputs for the shop directory tools.
//!
//! A field typed `Option<Option<T>>` follows the convention in `corrections`:
//! an absent key leaves the value alone, `null` clears it, and a value sets it.

use glovebox_shared::inputs::shop::{NewShop, UpdateShop};
use schemars::JsonSchema;
use serde::Deserialize;

use super::corrections::nullable;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddShopInput {
    /// The shop's name as the owner calls it, e.g. 'KW Automotive'.
    pub name: String,
    /// Street address. Give it when the owner uses more than one location of the same chain.
    pub address: Option<String>,
    pub phone: Option<String>,
    pub website: Option<String>,
    /// What the shop does, e.g. 'VW dealer', 'tires', 'body and paint'.
    pub specialty: Option<String>,
    pub notes: Option<String>,
}

impl AddShopInput {
    pub fn into_domain(self) -> NewShop {
        NewShop {
            name: self.name,
            address: self.address,
            phone: self.phone,
            website: self.website,
            specialty: self.specialty,
            notes: self.notes,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShopIdInput {
    /// Shop id, from `list_shops`.
    pub shop_id: i32,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateShopInput {
    /// Shop id, from `list_shops`.
    pub shop_id: i32,
    /// The new name. It cannot be cleared.
    pub name: Option<String>,
    /// Street address. Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub address: Option<Option<String>>,
    /// Phone number. Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub phone: Option<Option<String>>,
    /// Website. Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub website: Option<Option<String>>,
    /// What the shop does. Send `null` to clear it; omit the key to leave it unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub specialty: Option<Option<String>>,
    /// Notes. This REPLACES the stored notes: to add to them, send the stored text plus the new. Send `null` to clear them; omit the key to leave them unchanged.
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>")]
    pub notes: Option<Option<String>>,
}

impl UpdateShopInput {
    pub fn into_domain(self) -> (i32, UpdateShop) {
        (
            self.shop_id,
            UpdateShop {
                name: self.name,
                address: self.address,
                phone: self.phone,
                website: self.website,
                specialty: self.specialty,
                notes: self.notes,
            },
        )
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MergeShopsInput {
    /// The shop to keep, from `list_shops`. It keeps its name.
    pub keep_shop_id: i32,
    /// The duplicate shops to fold into it. They are deleted.
    pub merge_shop_ids: Vec<i32>,
}
