# 2hea Unit C — Parts Simplification: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drop the `part_slots` subsystem (the overkill half of parts), preserving slot names into a plain `part.location` text field, and add the `record_part` MCP verb.

**Architecture:** Migration backfills `location` from the joined slot name, then drops `parts.slot_id` and the `part_slots` table; the slot service/handler/UI/API layers are deleted; `record_part` lands as MCP tool #16. Spec: `docs/superpowers/specs/2026-07-02-2hea-feature-reassessment-design.md` Unit C.

**Tech Stack:** Rust workspace, SeaORM migrations (SQLite `ALTER TABLE ... DROP COLUMN` — house pattern per 000014's down()), rmcp, Svelte 5, Playwright.

## Global Constraints

- Bead `glovebox-2tec`. Branch `2hea/unit-c-parts-simplify` (checked out). No push/PR.
- Every task ends green (`cargo build --workspace && cargo test --workspace`; full `just ci` at the end). `just fmt` before each commit. Never stage `.beads/`.
- Build progress (`parts_installed`, cost dedupe) and `incident`-precursor observation links key on parts, NOT slots — must be untouched.
- `part_slots` is not FTS-indexed (absent from 000013 SPECS) — no FTS work.
- Commits end `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: Migration 000016 + entity update

**Files:**
- Create: `glovebox-shared/src/migration/m20260301_000016_drop_part_slots.rs` (register in `mod.rs`, appended)
- Modify: `glovebox-shared/src/entities/part.rs` (remove `slot_id` field + `part_slot` relation/Related impls; append `pub location: Option<String>` as the LAST field — physical column order rule)
- Delete: `glovebox-shared/src/entities/part_slot.rs` (+ its `pub mod` line; grep `vehicle.rs` for a part_slot relation and remove it)

- [ ] **Step 1: Write the migration** — raw SQL via `execute_unprepared` (mixed ALTER + backfill; mirror 000015's style):

```rust
//! Drops the part_slots subsystem (2hea unit C): slot names survive as a
//! plain `location` text column on parts.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("ALTER TABLE parts ADD COLUMN location TEXT")
            .await?;
        db.execute_unprepared(
            "UPDATE parts SET location = (SELECT name FROM part_slots WHERE part_slots.id = parts.slot_id) \
             WHERE slot_id IS NOT NULL",
        )
        .await?;
        db.execute_unprepared("ALTER TABLE parts DROP COLUMN slot_id")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS part_slots")
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Migration(
            "2hea unit C drops part_slots permanently; restore from a DB backup instead".into(),
        ))
    }
}
```

**Verify first:** the physical table names (`parts`, `part_slots`) and slot `name` column against the entity files BEFORE deleting them; confirm no index/FK on `parts.slot_id` blocks DROP COLUMN (check migration 000006 — if an index exists on slot_id, drop it before the column).

- [ ] **Step 2: Entity edits** — `part.rs`: delete `slot_id` field, the `belongs_to part_slot` Relation variant, and `impl Related<part_slot>`; append `pub location: Option<String>` after the current last field (`build_id`). Delete `part_slot.rs`, deregister, clean `vehicle.rs` relations if present.

- [ ] **Step 3: Fix compile fallout in `services/part.rs` ONLY enough to build** — remove `require_slot_owned` + its call sites and `slot_id` from create/update mapping (full input/DTO cleanup is Task 2; if it's simpler to do it in one pass, merge Tasks 1–2 into one commit — acceptable). Also fix the test at `part.rs:262` that seeds a `part_slot`.

- [ ] **Step 4: `cargo test --workspace`** → PASS (test_db proves 000016 applies; part tests updated). Commit: `feat(2hea-c): migration 000016 drops part_slots; parts gain location`.

### Task 2: Delete the slot layers end-to-end

**Files:**
- Delete: `glovebox-shared/src/services/part_slot.rs`, `glovebox-shared/src/inputs/part_slot.rs`, `glovebox-backend/src/api/part_slots.rs`
- Modify: `glovebox-shared/src/services/mod.rs`, `glovebox-shared/src/inputs/mod.rs`, `glovebox-backend/src/api/mod.rs`, `glovebox-backend/src/main.rs` (the two part-slot routes), `glovebox-shared/src/inputs/part.rs` (drop `slot_id`, add `location: Option<String>` to `NewPart`; `location: Option<Option<String>>` to `UpdatePart`), `glovebox-shared/src/services/part.rs` (map location in create/update), `glovebox-backend/src/api/parts.rs` (DTO fields follow)

- [ ] **Step 1:** Verify-grep `part_slot|PartSlot|slot_id` across crates → only the files above. Delete + deregister + remove routes; thread `location` through input → service → DTO (double-option on update with `deserialize_optional`).
- [ ] **Step 2:** Add/extend a part service test: create with `location: Some("Front brakes")`, update clears via `Some(None)`.
- [ ] **Step 3:** `cargo test --workspace` → PASS. Commit: `refactor(2hea-c): delete part_slot service/API layers; location threads through parts`.

### Task 3: `record_part` MCP verb (tool #16)

**Files:**
- Modify: `glovebox-mcp/src/schemas.rs` + `handler.rs`, `glovebox-mcp/tests/mcp_integration_test.rs`

- [ ] **Step 1:** Input struct per the house pattern (schemars docs, `into_domain()`), mapping to `NewPart` — READ `glovebox-shared/src/inputs/part.rs` first and mirror its actual fields (name, cost fields, status, location, installed_service_id, build_id, url, notes — whatever exists; do not invent). Description: "Record a part you bought or installed for this vehicle — purchase info, cost (integer cents), where it goes (location), and optional links to the installing service or a build project. Use record_service for the labor; this is the part itself."
- [ ] **Step 2:** Tool via `#[tool]` calling `part::create` (already paxy-guarded for installed_service_id/build_id). Errors via `domain_result`.
- [ ] **Step 3:** Integration test: tools/list 15→16 assertion; record_part round-trip (create → search or list via existing surface? parts have no MCP list — assert the tool result payload carries the created part's name/id); wrong-vehicle → tool error; cross-vehicle build_id → tool error.
- [ ] **Step 4:** `cargo test -p glovebox-mcp` → PASS. Commit: `feat(2hea-c): record_part MCP verb`.

### Task 4: Frontend + docs + gates

**Files:**
- Modify: `frontend/src/components/PartsTab.svelte` (remove slot cards/CRUD; parts list gains a location column/field in create/edit forms), `frontend/src/lib/api.ts` + `types.ts` (drop partSlots API + types; Part type gains `location`), `frontend/e2e/parts.spec.ts` (slot tests removed/rewritten: "No parts or slots yet." copy changes to "No parts yet."), `TEST_PLAN.md`, `CLAUDE.md` (16 tools, 19 entities, 16 migrations — verify counts)

- [ ] **Step 1:** READ PartsTab fully, excise slots, keep part CRUD + build/service linking; location as a plain text input.
- [ ] **Step 2:** Rewrite parts.spec.ts slot tests (create-part flow without slot; location field asserted). `bun run check` → 0 errors.
- [ ] **Step 3:** Full gates: `just ci` exit 0; `just test-e2e-ci` (state expected count: 44 ± parts-spec delta — report the arithmetic); boot smoke (health 200; `/api/vehicles/1/part-slots` → 404; `/mcp` tools/list → 16).
- [ ] **Step 4:** Commit: `refactor(2hea-c): PartsTab without slots; docs updated`.

## Self-Review
- Spec coverage: migration+backfill ✓ (T1), layer deletion ✓ (T2), record_part ✓ (T3), UI ✓ (T4), build-progress untouched (constraint + T1 keeps part linking). No gaps.
- Placeholders: none; read-first instructions where the plan can't know exact fields.
- Type consistency: `location` Option<String> new / Option<Option<String>> update, consistent T1→T4.
