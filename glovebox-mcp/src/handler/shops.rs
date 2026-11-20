//! Tools for the shop directory.

use rmcp::{ErrorData as McpError, model::CallToolResult, tool, tool_router};

use super::{GloveboxMcp, Parameters, deleted_result, domain_result};
use glovebox_shared::services::{shop, today};

use crate::schemas::{AddShopInput, EmptyParams, MergeShopsInput, ShopIdInput, UpdateShopInput};

#[tool_router(router = shops_router, vis = "pub(super)")]
impl GloveboxMcp {
    #[tool(
        name = "list_shops",
        description = "Every shop in the directory, in name order, each with its `service_count`, \
                       `appointment_count` and `last_service_on`. Call it before adding a shop, \
                       and when two entries look like one shop under two names, ask the owner and \
                       fold them together with `merge_shops`."
    )]
    async fn list_shops(
        &self,
        params: Parameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        tool_input!(params, "list_shops");
        domain_result(shop::list(&*self.db).await)
    }

    #[tool(
        name = "add_shop",
        description = "Add a shop to the directory. Check `list_shops` first: a shop of the same \
                       name (ignoring case and surrounding spaces) is refused unless both have \
                       different addresses, as two locations of one chain do. When a new name \
                       looks like a shop already listed, ask the owner whether it is the same one \
                       before adding it."
    )]
    async fn add_shop(&self, params: Parameters<AddShopInput>) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "add_shop");
        domain_result(shop::create(&*self.db, p.into_domain()).await)
    }

    #[tool(
        name = "get_shop",
        description = "One shop with its history across every vehicle: its `services` (newest \
                       first, each with `total_cost_cents` and the owner's \
                       `out_of_pocket_cents`), its `appointments`, and the totals. Use it to \
                       answer who did a job, what a shop has done, or what it has cost."
    )]
    async fn get_shop(&self, params: Parameters<ShopIdInput>) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "get_shop");
        domain_result(shop::history(&*self.db, p.shop_id, today()).await)
    }

    #[tool(
        name = "update_shop",
        description = "Change a shop's name, address, phone, website, specialty or notes. A \
                       change that would make it the same shop as another listed one is refused; \
                       merge the two with `merge_shops` instead."
    )]
    async fn update_shop(
        &self,
        params: Parameters<UpdateShopInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "update_shop");
        let (id, input) = p.into_domain();
        domain_result(shop::update(&*self.db, id, input).await)
    }

    #[tool(
        name = "merge_shops",
        description = "Fold duplicate shops into one, after the owner confirms they are the same \
                       shop. Every service and appointment that names a merged shop then names \
                       `keep_shop_id`, which keeps its name and takes each detail it lacks; the \
                       notes are joined, and the merged shops are deleted. The result lists the \
                       moved `moved_service_ids` and `moved_appointment_ids`, and in `discarded` \
                       each value the kept shop already had a different one of: tell the owner \
                       about those, and use `update_shop` if one should win."
    )]
    async fn merge_shops(
        &self,
        params: Parameters<MergeShopsInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "merge_shops");
        domain_result(shop::merge(&*self.db, p.keep_shop_id, &p.merge_shop_ids).await)
    }

    #[tool(
        name = "delete_shop",
        description = "Delete a shop that no service or appointment names, such as one added by \
                       mistake. A shop with history is refused; merge it into the shop that \
                       replaces it with `merge_shops` instead."
    )]
    async fn delete_shop(
        &self,
        params: Parameters<ShopIdInput>,
    ) -> Result<CallToolResult, McpError> {
        let p = tool_input!(params, "delete_shop");
        deleted_result("shop", p.shop_id, shop::delete(&*self.db, p.shop_id).await)
    }
}
