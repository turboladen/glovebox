# 2hea Unit G — Planning, Budget, Warranty: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The `work_item` + `visit` planning primitives ("the list of things I'm actually gonna do"), the 12-month budget forecast, and warranty fields — closing the recall→plan→visit→service-record loop end-to-end over MCP.

**Architecture:** Migration 000020 (2 new tables + 3 ALTER sets); `services/work_item.rs` + `services/visit.rs` (with the transactional `complete` flow) + `services/budget.rs`; `reminders` migrates to `DomainResult` (the `le5y` fold); 4 new MCP verbs (tools 18→22) + offer-to-plan guidance. **No UI in this unit** — the Plan tab is unit F's; HTTP endpoints land thin so F has something to call. Spec: Unit G + decisions ⑦⑧⑩ of `docs/superpowers/specs/2026-07-02-2hea-feature-reassessment-design.md`.

## Global Constraints

- Bead `glovebox-ie9w`. Branch `2hea/unit-g-planning` (checked out). No push/PR. Never stage `.beads/`.
- Every task ends green; TDD; `just fmt` before commits; full `just ci` + e2e at the end (e2e count should hold at 51 — no UI change).
- Migration rerun-safety per house pattern: pragma-guarded ALTERs (000019 precedent), `if_not_exists` creates. Entity fields append LAST on altered tables.
- Integer cents everywhere. Status whitelists use the house message shape. Paxy discipline on every link (each source FK vehicle-scoped, wrong-parent indistinguishable, regression test each).
- Commits end `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: Migration 000020 + entities

**Files:** create `glovebox-shared/src/migration/m20260301_000020_planning.rs` (register); create `glovebox-shared/src/entities/{work_item,visit}.rs` (register); modify `maintenance_schedule_item.rs`, `vehicle.rs`, `part.rs` entities (append new fields LAST).

- **`work_items`:** `id` PK AI · `vehicle_id` INT NOT NULL FK→vehicles CASCADE · `title` TEXT NOT NULL · `notes` TEXT NULL · `schedule_item_id` INT NULL · `research_finding_id` INT NULL · `incident_id` INT NULL · `build_id` INT NULL · `est_cost_cents` INT NULL · `status` TEXT NOT NULL DEFAULT `'planned'` · `visit_id` INT NULL · `created_at`/`updated_at` defaults. Index vehicle_id. (Source FKs plain nullable INTs — service layer enforces, house 000014 pattern.)
- **`visits`:** `id` PK AI · `vehicle_id` INT NOT NULL FK→vehicles CASCADE · `planned_date` TEXT NULL · `shop_name` TEXT NULL · `shop_id` INT NULL · `notes` TEXT NULL · `status` TEXT NOT NULL DEFAULT `'planned'` · `service_record_id` INT NULL · `created_at`/`updated_at`. Index vehicle_id.
- **ALTERs (pragma-guarded each):** `maintenance_schedule_items` + `est_cost_cents` INT NULL (**verify the physical table name from the entity first** — house lesson) · `vehicles` + `warranty_expires_on` TEXT NULL, `warranty_expires_miles` INT NULL · `parts` + the same pair.
- No FTS arms (planning rows are short-lived operational data; searchable later if wanted — note as a non-goal).

- [ ] Steps: read entity table_names → write migration + entities → `cargo test --workspace` (test_db proves it) → commit: `feat(2hea-g): planning schema (work_items, visits) + est_cost + warranty fields`.

### Task 2: `services/work_item.rs` + `services/visit.rs`

**work_item fns:** `list(db, vehicle_id, include_done: bool)` (status != done/dropped unless included; created desc) · `get` · `create(db, vehicle_id, NewWorkItem)` — title non-blank (exemplar pattern); **every present source FK guarded**: `schedule_item_id` in the vehicle's scope chain (reuse the `require_schedule_items_in_scope` machinery for a single id), `research_finding_id` via finding→report→vehicle, `incident_id` vehicle-scoped, `build_id` via `build::require_owned`, `visit_id` vehicle-scoped · `update` (status whitelist `planned|scheduled|done|dropped`; same guards on changed links; double-option on nullables; `updated_at`) · `delete`.

**visit fns:** `list(db, vehicle_id, include_closed: bool)` · `get` → view `VisitWithItems { visit, items: Vec<work_item::Model>, est_total_cents: i64 }` (batch, Σ item est_cost_cents) · `create(db, vehicle_id, NewVisit { planned_date?, shop_name?, shop_id?, notes?, work_item_ids? })` — attaching items sets their `visit_id` + status `scheduled` (txn; items must be vehicle-owned) · `update` (status whitelist `planned|scheduled|completed|canceled`; `completed` only via `complete`; attaching/detaching items txn) · `delete` — txn: detach items (visit_id NULL, status back to `planned` when it was `scheduled`), then delete · **`complete(db, vehicle_id, visit_id, CompleteVisit { service_date, mileage?, description?, total_cost_cents?, parts_cost_cents?, labor_cost_cents?, paid_by?, payer_note?, notes? }) -> DomainResult<CompletedVisit>`** — the loop-closer, one txn:
  1. visit must exist (vehicle-scoped) and not already be completed/canceled (BadRequest otherwise);
  2. build the service record via `service_record::create` (description defaults to the joined item titles; `schedule_item_ids` = the items' non-null `schedule_item_id`s — clears reminders; payer-aware);
  3. incident-sourced items: link via `incident_service_links` (INSERT the pairs — or call the incident service's link mechanism if cleanly callable);
  4. research-finding-sourced items: set the finding status `completed` (closes the recall in the Research view);
  5. mark all attached items `done`; set `visit.service_record_id` + status `completed`; stamp `updated_at`s.
  Note: `service_record::create` takes `C: ConnectionTrait + TransactionTrait` and opens its own txn — CHECK whether nested `begin()` on a txn works in SeaORM (savepoints) or whether `complete` must inline the record construction instead of calling `create`. READ `service_record::create` first and pick the approach that keeps ONE atomic unit; document the choice.

**Tests:** every source-link wrong-vehicle probe (5 kinds) + happy round-trips; visit attach/detach status flips; delete detaches; complete: full-loop test (schedule-sourced item clears the reminder — assert via `calculate_reminders`; incident link lands; finding goes completed; items done; visit completed + linked) + wrong-vehicle + double-complete rejection + rollback-on-failure (e.g. bad paid_by rejects and NOTHING mutated).

- [ ] Steps: failing tests → implement → green → commit: `feat(2hea-g): work_item + visit services with transactional complete`.

### Task 3: `services/budget.rs` + reminders `DomainResult` (le5y fold) + warranty flag

- **`reminders` → `DomainResult`** (mechanical: `DbErr` → `DomainError` via `?`; the MCP handler's ad-hoc `db_error` path switches to the shared `domain_result`).
- **`budget::forecast(db, vehicle_id) -> DomainResult<BudgetForecast>`:** 12-month horizon. For each enabled resolved schedule item with `est_cost_cents`: project occurrences using `interval_months` and/or `interval_miles` ÷ the vehicle's actual mileage rate (reuse `reminders::estimate_mileage`'s rate machinery — READ it; extract a helper if needed rather than duplicating), overdue items count as one occurrence now. Plus open visits' `est_total_cents` (planned/scheduled). Output: `BudgetForecast { horizon_months: 12, projected_maintenance_cents, planned_visits_cents, total_cents, lines: Vec<ForecastLine { label, when (date-ish string), est_cents }> }` — integer math only.
- **Warranty flag:** `RemindersResponse` (or a wrapper) gains `warranty: Option<WarrantyStatus { expires_on?, expires_miles?, possibly_covered: bool }>` — covered when today ≤ expires_on OR estimated mileage ≤ expires_miles (either sufficient; document).
- HTTP: `GET /api/vehicles/{vehicle_id}/budget` (thin). Tests: forecast arithmetic pinned with a seeded interval+rate scenario (deterministic: inject the rate or seed mileage logs at known dates — whichever `estimate_mileage` allows; keep the test stable, no wall-clock flakiness — if unavoidable, assert ranges).

- [ ] Steps: failing tests → implement → green → commit: `feat(2hea-g): budget forecast + warranty status; reminders to DomainResult`.

### Task 4: MCP verbs + guidance + gates

- **New tools (18 → 22):** `plan_work(vehicle_id, title, est_cost_cents?, notes?, schedule_item_id?, research_finding_id?, incident_id?, build_id?)` — description: "Add something to the vehicle's to-do list — work the user intends to do or have done. Link the source so completing the work closes the loop (a recall finding, an overdue schedule item from check_due_maintenance, an incident, a build)." · `list_planned_work(vehicle_id, include_done?)` (returns items + open visits w/ rollups) · `schedule_visit(vehicle_id, planned_date?, shop_name?, work_item_ids?, notes?)` — "Group planned work into a shop visit (or DIY session) with a date and estimated cost." · `complete_visit(vehicle_id, visit_id, service_date, mileage?, total_cost_cents?, paid_by?, ...)` — "Close out a visit: creates the service record, clears satisfied reminders, resolves linked recalls/incidents, marks the work done."
- **Guidance:** `check_recalls` + `check_due_maintenance` descriptions gain "offer to plan_work" sentences; `check_due_maintenance`'s MCP payload now includes the budget forecast + warranty status (compose `calculate_reminders` + `budget::forecast` + warranty in the handler); `record_part` schema gains the warranty pair; server instructions updated (plan flow added to the workflow phases).
- **Integration tests:** tools/list 22; recall→plan→schedule→complete loop over the protocol (file a finding via file_research_finding, plan_work from it, schedule_visit, complete_visit, then assert: finding completed, reminders clear if schedule-linked, service record exists with payer); wrong-vehicle probes on each new tool; malformed args.
- **Docs:** CLAUDE.md (22 tools, 21 migrations, 20 entities — verify counts on disk), TEST_PLAN note (planning is MCP/HTTP-only until unit F).
- **Gates:** `just ci` exit 0; `just test-e2e-ci` = 51 (no UI change — confirm); boot smoke: budget endpoint 200 with integer cents; the full MCP loop via curl if practical (else the integration test stands).

- [ ] Steps: implement → tests green → gates → commit: `feat(2hea-g): plan_work/schedule_visit/complete_visit/list_planned_work MCP verbs (22 tools)`.

## Self-Review
- Spec ⑦ ✓ (both primitives, sources, rollups, transactional complete closing every loop: reminders via schedule_item_ids, recalls via finding status, incidents via service links). ⑧ ✓ (est_cost + forecast, surfaced in check_due_maintenance + budget endpoint; Costs UI display deferred to F with the rest of the UI — noted). ⑩ ✓ (warranty fields on vehicle+part, possibly_covered flag). le5y fold ✓ (Task 3). Plan-tab UI deliberately absent ✓ (unit F).
- Placeholders: none; the one genuinely open mechanic (nested txn vs inlined record construction in `complete`) is a documented read-first decision, not a TBD.
- Types: `NewWorkItem`/`NewVisit`/`CompleteVisit`/`VisitWithItems`/`BudgetForecast` consistent across tasks; status whitelists distinct per entity and stated once each.
