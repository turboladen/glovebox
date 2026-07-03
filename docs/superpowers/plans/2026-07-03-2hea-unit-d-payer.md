# 2hea Unit D — Payer Tracking: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every service record knows who paid (`self | insurance | third_party`), and cost rollups split out-of-pocket vs covered — so the $15k insurance repair stops inflating what Steve actually spent.

**Architecture:** Two appended columns on `service_records` (migration 000017), whitelist validation in the service, split fields added to `costs::summary` + `build::progress`, threaded through MCP (`record_service` input, `cost_summary`/`get_build_progress` output) and the Costs UI. Spec: Unit D of `docs/superpowers/specs/2026-07-02-2hea-feature-reassessment-design.md`.

**Tech Stack:** Rust workspace, SeaORM migration (plain ALTER ADD — house 000009/000012/000014 pattern), rmcp, Svelte 5.

## Global Constraints

- Bead `glovebox-b5oz`. Branch `2hea/unit-d-payer` (checked out). No push/PR. Never stage `.beads/`.
- Every task ends green; `just fmt` before commits; full `just ci` + e2e at the end. Integer cents everywhere (no float).
- Entity field order: `paid_by` then `payer_note` appended as the LAST fields of `service_record` (ALTER appends physically).
- Existing rows default to `'self'` — the migration's column default handles it.
- Commits end `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: Migration 000017 + entity + service validation

**Files:**
- Create: `glovebox-shared/src/migration/m20260301_000017_add_service_payer.rs` (register, appended)
- Modify: `glovebox-shared/src/entities/service_record.rs` (append `pub paid_by: String,` then `pub payer_note: Option<String>,` LAST), `glovebox-shared/src/inputs/service_record.rs` (`NewServiceRecord.paid_by: Option<String>`, `payer_note: Option<String>`; `UpdateServiceRecord.paid_by: Option<String>` — plain option, non-nullable column — and `payer_note: Option<Option<String>>`), `glovebox-shared/src/services/service_record.rs` (validation + mapping in create/update)

- [ ] **Step 1: Migration** — two `alter_table` ADDs per the house pattern (000012 style): `paid_by TEXT NOT NULL DEFAULT 'self'`, `payer_note TEXT NULL`. No backfill needed (default covers existing rows).
- [ ] **Step 2: Failing tests first** in `service_record.rs` tests: create with `paid_by: Some("insurance".into())` round-trips; create with `paid_by: None` defaults to `"self"`; `paid_by: Some("my neighbor")` → `DomainError::BadRequest` listing valid values (mirror the build-status whitelist message shape); update changes payer + sets/clears `payer_note` via double-option.
- [ ] **Step 3: Implement** — `const VALID_PAYERS: [&str; 3] = ["self", "insurance", "third_party"];` validation fn used by create (when `Some`) and update (when `Some`); create maps `paid_by.unwrap_or_else(|| "self".into())`; update maps both fields (stamping `updated_at` already happens).
- [ ] **Step 4:** `cargo test --workspace` → PASS. Commit: `feat(2hea-d): service records track who paid (migration 000017 + validation)`.

### Task 2: Cost splits in shared rollups

**Files:**
- Modify: `glovebox-shared/src/services/costs.rs` (`CostSummary` gains `out_of_pocket_cents: i64`, `covered_cents: i64`; `MonthlyCost` gains the same pair), `glovebox-shared/src/services/build.rs` (`BuildProgress` gains `out_of_pocket_cents: i64`)

**Semantics (make the tests pin these exactly):**
- A service's cost counts as covered when `paid_by != "self"`, out-of-pocket when `"self"`.
- `covered_cents` + `out_of_pocket_cents` = the existing service-cost total per bucket (service records only — parts keep their existing extra-parts treatment and count as out-of-pocket, since parts have no payer).
- Build progress: `out_of_pocket_cents` = existing `total_cost_cents` formula but with covered services' totals (and their parts_cost components) excluded from the service side; keep `total_cost_cents` unchanged for continuity.

- [ ] **Step 1: Failing tests** — costs: seed self-$100 + insurance-$150 services + a $20 part → `total` unchanged from current formula, `out_of_pocket = 100_00-style ints + part extra`, `covered = 150...`; monthly split correct. Build: linked covered service excluded from `out_of_pocket_cents`, included in `total_cost_cents`.
- [ ] **Step 2: Implement** (single-pass iteration, integer i64 math, mirror existing code shape). 
- [ ] **Step 3:** `cargo test --workspace` → PASS. Commit: `feat(2hea-d): costs + build rollups split out-of-pocket vs covered`.

### Task 3: MCP + Costs UI

**Files:**
- Modify: `glovebox-mcp/src/schemas.rs` (`RecordServiceInput` gains `paid_by`/`payer_note` with doc: `/// Who paid: \`self\` (default), \`insurance\`, or \`third_party\` (e.g. the other driver).` + note payer_note for "who/claim #"), `glovebox-mcp/src/handler.rs` (map through; `cost_summary` output already serializes CostSummary — verify the new fields flow), `glovebox-mcp/tests/mcp_integration_test.rs` (record_service with insurance payer → cost_summary shows covered split; invalid payer → tool error)
- Modify: `frontend/src/components/CostsTab.svelte` (show out-of-pocket vs covered in totals + monthly), `frontend/src/components/HistoryTab.svelte` + its service form (payer select: Me/Insurance/Third party + optional note field — READ the form first and follow its field idioms), `frontend/src/lib/types.ts` (+ api payloads), `frontend/e2e/` (extend the vehicle-detail or costs coverage minimally: record an insurance-paid service, assert the Costs split renders), `TEST_PLAN.md`

- [ ] **Step 1:** MCP fields + integration test (tools count stays 16). `cargo test -p glovebox-mcp` → PASS.
- [ ] **Step 2:** UI: service form payer select (default Me), payer note input shown when payer != self; CostsTab renders "Out of pocket" / "Covered by others" lines (totals + per-month if the layout allows without redesign — keep it modest, unit F redoes the shell).
- [ ] **Step 3:** Full gates: `just ci` exit 0; `just test-e2e-ci` (report count arithmetic); boot smoke (health, one recorded insurance service via curl → costs endpoint shows split). Commit: `feat(2hea-d): payer through MCP + Costs/service-form UI`.

## Self-Review
- Spec coverage: columns+default ✓, whitelist ✓, costs+build splits ✓, MCP input/output ✓, Costs UI ✓. Payer on parts: deliberately none (spec scopes payer to service_record; parts count as out-of-pocket — stated in Task 2 semantics).
- Placeholders: none; read-first notes where form idioms matter.
- Type consistency: `paid_by: String` entity / `Option<String>` create+update / `payer_note` double-option on update — consistent across tasks.
