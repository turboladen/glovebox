# 2hea Unit E — Overdue Resolvability + Mileage FK: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Overdue maintenance items become resolvable (link a service via MCP, record a minimal service, or dismiss via the existing vehicle-level override), and `mileage_log` gains the `service_record_id` FK that fixes the activity-feed dedupe holes.

**Architecture:** Migration 000019 adds the FK column; `service_record` create/delete maintain it; activity/reminders key on it. `record_service` (MCP) exposes `schedule_item_ids`; a new `dismiss_schedule_item` verb creates the vehicle-level `enabled=false` override that `schedule::resolve`'s name-shadowing already honors; Schedule UI surfaces overrides + minimal-record + link affordances. Spec: Unit E of `docs/superpowers/specs/2026-07-02-2hea-feature-reassessment-design.md` (decision ⑤ — no new state).

## Global Constraints

- Bead `glovebox-y0ut`. Branch `2hea/unit-e-overdue` (checked out). No push/PR. Never stage `.beads/`.
- Every task ends green; `just fmt` before commits; full `just ci` + e2e at the end; migration idempotency/rerun-safety per the unit-B lessons (untransacted SQLite migrations).
- Read-first: `schedule::resolve`'s shadowing mechanics, `reminders::calculate_reminders`'s mileage sourcing, `service_record::create`'s auto-mileage-log block, `activity.rs`'s dedupe comment (it names this FK as the durable fix).
- Commits end `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: Mileage FK (migration 000019) + maintenance + dedupe rekey

**Files:**
- Create: `glovebox-shared/src/migration/m20260301_000019_mileage_service_fk.rs` (register): ALTER `mileage_logs` ADD `service_record_id` INT NULL (house 000012 pattern — plain column, service layer enforces). **Backfill best-effort:** `UPDATE mileage_logs SET service_record_id = (SELECT s.id FROM service_records s WHERE s.vehicle_id = mileage_logs.vehicle_id AND s.mileage = mileage_logs.mileage AND mileage_logs.source = 'service' LIMIT 1) WHERE source = 'service'` — heuristic match for existing auto-logs; document that ambiguity picks one (acceptable: pre-ship data).
- Modify: `glovebox-shared/src/entities/mileage_log.rs` (append field LAST + relation), `glovebox-shared/src/services/service_record.rs` (create: auto-log sets `service_record_id`; **delete: remove the auto-log(s) `WHERE service_record_id = record.id` inside the existing txn**; update: if mileage changes, update the linked auto-log's mileage — read what create does and mirror), `glovebox-shared/src/services/activity.rs` (dedupe: exclude mileage logs `WHERE service_record_id IS NOT NULL` instead of `source == "service"`; update the doc comment — the holes it documents are now closed), `glovebox-shared/src/services/mileage.rs` (create keeps `source` for display but the FK is the linkage; NewMileageEntry doesn't expose the FK — internal only).
- Tests: deleted-service's log disappears from DB (not just feed); manual log with `source: "service"` (via HTTP-style input) now VISIBLE in the feed; service create→feed shows service not duplicate mileage; update-mileage syncs the log.

- [ ] Steps: read-first → failing tests → migration+code → `cargo test --workspace` green → commit: `feat(2hea-e): mileage_log.service_record_id FK closes the activity dedupe holes`.

### Task 2: dismiss/link/minimal-record in shared + MCP

**Files:**
- Modify: `glovebox-shared/src/services/schedule.rs`: new `dismiss_for_vehicle(db, vehicle_id, schedule_item_id, reason: Option<String>) -> DomainResult<maintenance_schedule_item::Model>` — read the item (any owner in the vehicle's chain: reuse `require_schedule_items_in_scope`'s logic or the resolve chain); if vehicle-owned → set `enabled=false` (+ append reason to notes if the column exists — read the entity); if inherited → create a vehicle-owned shadow item (same `name`, `enabled=false`, reason in notes) which `resolve()`'s name-shadowing already hides. Return the override. Also `undismiss_for_vehicle` (re-enable or delete the shadow — pick the simpler: set `enabled=true` on the vehicle-owned row; document why). Tests: dismiss inherited item hides it from `resolve()`; dismiss vehicle-owned disables in place; wrong-vehicle → NotFound; undismiss restores.
- MCP (`glovebox-mcp`): `record_service` input gains `schedule_item_ids: Option<Vec<i32>>` (doc: "ids from `check_due_maintenance` this work satisfies — linking clears the reminder"); `check_due_maintenance` description gains the guidance sentence; new tool **`dismiss_schedule_item(vehicle_id, schedule_item_id, reason?)`** (doc: "Waive a maintenance item for this vehicle — for items done before tracking, use record_service with a minimal past-dated entry instead so history stays honest"). Tools 17 → **18**. Integration tests: record_service with schedule_item_ids → check_due_maintenance no longer lists it as overdue; dismiss → resolve() output excludes it; wrong-vehicle probes.

- [ ] Steps: failing tests → implement → green → commit: `feat(2hea-e): dismiss_schedule_item + schedule linking via MCP`.

### Task 3: Schedule UI affordances + gates

**Files:**
- Modify: `frontend/src/components/ScheduleTab.svelte` (READ fully first): each due/overdue row gains a small action menu: "Record service…" (prefilled minimal service form: today's date, description = item name, linked schedule_item_id — reuse/adapt the existing service-form component if importable, else a compact inline form), "Mark done previously" (creates the minimal record with a user-picked past date, zero cost, notes "recorded retroactively"), "Dismiss for this vehicle" (calls the new endpoint; dismissed/overridden items render greyed with an "overridden" badge + re-enable action). New thin HTTP endpoints in `glovebox-backend/src/api/schedules.rs`: `POST /api/vehicles/{vehicle_id}/schedule/{item_id}/dismiss` + `DELETE .../dismiss` (undismiss) — thin per layering.
- e2e: extend the schedule/vehicle-detail spec: dismiss → item greyed + reminders drop it; record-service-from-schedule → reminder clears. TEST_PLAN + CLAUDE.md (18 tools).

- [ ] Steps: UI → e2e → `just ci` exit 0 + `just test-e2e-ci` (report arithmetic vs 48) + boot smoke (dismiss round-trip via curl; `/mcp` tools/list = 18) → commit: `feat(2hea-e): schedule dismiss/link/minimal-record affordances`.

## Self-Review
- Spec ⑤ coverage: link ✓ (MCP schedule_item_ids — shared already supported), minimal record ✓ (UI affordance + MCP doc guidance), dismiss ✓ (existing override mechanism surfaced; no new state), plan-it deferred to G ✓. w6ws fold ✓ (FK + maintenance + rekey). No gaps.
- Placeholders: none; read-first where mechanics matter (resolve shadowing, auto-log block, notes column existence).
- Types: `dismiss_for_vehicle`/`undismiss_for_vehicle` naming consistent Task 2↔3; FK name `service_record_id` consistent Task 1.
