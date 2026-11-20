# ADR-0013: Platforms and templates come from migrations, and shops wait for their own design

- **Status:** Accepted; the shop part is superseded by ADR-0021
- **Date:** 2026-11-17

## Context

The schedule hierarchy has three layers: a platform (VW MQB), a model template (Golf GTI, Golf
Alltrack) and the vehicle. The platform and the templates, with their schedule items, are written
by the seed migration `m20261112_000002_seed_vw_mqb`, and `list_model_templates` reads them. The
question is whether the platform and template for another kind of car should come from a tool or
from a migration.

The `shops` table and the `shop_id` columns on services and appointments are in the schema, and
a service names its shop in the free-text `shop_name`, which already holds one dealer under two
names.

## Decision

**A platform or template for another kind of car is added by a migration**, the way the VW one
was. Its schedule comes from the manufacturer's maintenance book, it changes once per model, and a
migration puts it in every environment at once. `glovebox-shared` keeps only the reads:
`model_template::list` and `list_with_platform`.

**A car with no template gets its schedule through `add_schedule_item`.** Each item then belongs
to the vehicle alone.

**The shop schema stays, with no write path.** `shop_id` stays on the service and
appointment inputs, and both writes refuse an id that names no shop. Shops become first-class (one record per shop, merging
the duplicate names `shop_name` already holds, and history per shop) as a feature of its own.

## Alternatives considered

- **Tools to create and edit platforms and templates.** They would let Claude Desktop add a
  second car's book in conversation. A template's schedule is a long list copied from the
  maintenance book, which a reviewed migration gets right more reliably than a series of tool
  calls, and a vehicle's own items already cover one car with no template.
- **Dropping the shops table and the `shop_id` columns.** Both columns carry a foreign key, so
  SQLite needs a table rebuild of `service_records` and `appointments`, each with search
  triggers, to remove data that is always empty. The first-class shop feature would need them back.

## Consequences

- Adding a second kind of car is a code change and a release, not a conversation.
- No tool writes a shop, so `shop_id` stays empty until the shop feature lands. Services and
  appointments still check a `shop_id` they are given (`shop::require_if_named`).
