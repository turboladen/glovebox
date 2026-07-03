# 2hea Unit F — Dashboard + Navigation Shell Redesign: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The SPA's shell rebuilds per decision ⑥: login lands on a garage-wide dashboard, vehicles live in a collapsible sidebar, per-car view is the same dashboard scoped as an Overview tab, and the tabs reorganize by intent — **Overview · Timeline · Plan · Builds · Records · Costs**.

**Architecture:** Backend first (the missing HTTP surface: visits/work-items routes + garage-wide dashboard aggregation + the `is_open` delete guard + `cancel_visit` MCP verb), then the shell (App/header/sidebar), then the six tabs (several are re-homes of existing components), then the e2e-suite rewrite. Mockups from the 2026-07-03 visual session are the layout reference: `.superpowers/brainstorm/14448-1783103102/content/{nav-layout,scoped-view,tab-rework}.html` (option A sidebar · Overview-as-first-tab · five-intents + Builds top-level). Spec: Unit F + decision ⑥ of `docs/superpowers/specs/2026-07-02-2hea-feature-reassessment-design.md`. Carry-ins recorded on bead `glovebox-1cut`'s notes — ALL are in scope here.

## Global Constraints

- Bead `glovebox-1cut`. Branch `2hea/unit-f-shell` (checked out). No push/PR. Never stage `.beads/`.
- Every task ends green; `just fmt` before commits; full `just ci` + e2e at the end. Layering: handlers stay thin; aggregation logic lives in `glovebox-shared`.
- **Design tone:** wireframe-faithful, house CSS idioms (read 2–3 existing components' styles first — this app has a consistent card/badge/chip language; extend it, don't import a framework). Deep-links: every dashboard line navigates somewhere.
- The garage has FEW vehicles (personal app) — per-vehicle loops in garage aggregation are acceptable; note where.
- Commits end `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: Backend surface (shared + HTTP + MCP)

**Shared (`glovebox-shared/src/services/`):**
- `visit.rs`: add the **`is_open` guard to `delete`** (carry-in #3 — completed/canceled visits reject deletion with the update-guard's message shape; test). Add `cancel(db, vehicle_id, id)` convenience (update-to-canceled path already detaches; expose it properly).
- `dashboard.rs` (NEW): `garage(db) -> DomainResult<GarageDashboard>` — per vehicle (loop; few vehicles): `VehicleSummary { vehicle, overdue_count, due_soon_count, open_recall_count (findings category=recall status=new), unresolved_incident_count, active_build: Option<(id, name)>, estimated_mileage }` + garage-wide rollups: `attention: Vec<AttentionItem { vehicle_id, vehicle_name, kind (overdue|due_soon|recall|incident), label, deep_link_hint }>`, `upcoming_visits` (open visits w/ est rollups across vehicles), `budget_total_cents` (Σ per-vehicle forecast totals), `active_builds`. Reuse `calculate_reminders`/`budget::forecast_from`/existing list fns — compose, don't re-query ad-hoc.
- `activity.rs`: `recent_all(db, limit) -> DomainResult<Vec<ActivityItem>>` — garage-wide merged feed (same shape + `vehicle_id`/`vehicle_name` added to `ActivityItem` — check serialization consumers; additive fields fine).
- **Forecast backlog decision (carry-in #4):** extend the first-occurrence dedupe to BACKLOG schedule-linked items too (an item on the to-do list with `schedule_item_id` skips that schedule item's first occurrence — same rationale as visits: "already planned"). Pin with the test the reviewer specified (schedule-linked backlog item no longer double-counts).

**HTTP (thin, `glovebox-backend/src/api/`):** `plan.rs` (NEW): `GET/POST /api/vehicles/{vehicle_id}/work-items`, `PUT/DELETE /{id}`; `GET/POST /api/vehicles/{vehicle_id}/visits`, `PUT /{id}`, `POST /{id}/complete`, `POST /{id}/cancel`, `DELETE /{id}`. `dashboard.rs` (NEW): `GET /api/dashboard` (garage), `GET /api/dashboard/activity?limit=`. Per-vehicle: `GET /api/vehicles/{vehicle_id}/activity?limit=` (Timeline's feed — `activity::recent` exists). Register flat-route style.

**MCP:** `cancel_visit(vehicle_id, visit_id)` (carry-in #2 — description: "Cancel a visit that won't happen; its work items return to the to-do list.") → tools **23**. `schedule_visit` gains `shop_id?` (carry-in #1's MCP half; doc: "from the shops list" — and since no list_shops tool exists, keep shop_name as the primary path in the doc). Integration tests: cancel round-trip (forecast drops the visit; items back to planned), tools/list 23, and add the **finding back-link protocol assertion** to the loop test (carry-in #5).

- [ ] Steps: shared TDD → HTTP thin → MCP + tests → `cargo test --workspace` green → commit: `feat(2hea-f): plan/dashboard HTTP surface, cancel_visit, delete guard, forecast backlog dedupe`.

### Task 2: Shell — App, header, sidebar

**Files:** `frontend/src/App.svelte` (restructure), new `frontend/src/components/{Header,Sidebar}.svelte`; `Garage.svelte` retires (its welcome/setup content moves into the dashboard's empty state); routing (`@keenmate/svelte-spa-router`): `/` → Dashboard (garage scope), `/vehicles/{id}` → VehicleDetail (Overview default), keep `/vehicles/new`.

- **Header:** logo (→ `/`), global search input (existing `GET /api/search`; results dropdown grouped by kind, each hit deep-links: service→vehicle Timeline, incident→Timeline, document→Records, build→Builds, vehicle→its Overview), sidebar toggle (hamburger).
- **Sidebar** (mockup A): "All vehicles" entry + one card per vehicle: name, year/make/model small, status hints from `GET /api/dashboard` (`N due` red when overdue>0, `build active` chip, mileage). Active state; **fully hideable** — toggle collapses to nothing with a slim reopen handle (persist collapsed state in localStorage). `+ Add vehicle` at bottom → `/vehicles/new`.
- Empty garage: dashboard shows the welcome/checklist card (port the relevant bits from Garage.svelte before deleting it).

- [ ] Steps: build → `bun run check` clean → commit: `feat(2hea-f): header + collapsible sidebar garage shell`.

### Task 3: Dashboard (garage-wide + per-car Overview)

**Files:** new `frontend/src/components/Dashboard.svelte` (parameterized: garage scope when no vehicle, car scope when given one — ONE component, per the "same dashboard scoped" decision); `VehicleDetail.svelte` gains Overview as the FIRST/default tab rendering `<Dashboard vehicleId={id}>`.

Blocks (mockup layout: attention full-width top; plan&budget + builds side-by-side; activity below):
- **Needs attention** (red-tinted): overdue + due-soon (reminders), open recalls (findings), unresolved incidents — vehicle-labeled in garage scope; each row deep-links (reminder → Plan tab Due section; recall → Records/Research; incident → Timeline) and overdue rows get a "plan it" quick-action (`POST work-items` with schedule_item_id prefilled).
- **Plan & budget** (blue): next visits w/ dates + est totals (→ Plan tab), unscheduled to-do count, 12-month forecast total (garage: summed).
- **Builds** (green, conditional on active builds): name + parts progress + spend (→ Builds tab).
- **Recent activity:** merged feed (garage: `recent_all` with vehicle names; car: existing per-vehicle), each row deep-links to Timeline.

- [ ] Steps: build → check clean → commit: `feat(2hea-f): garage + scoped dashboard (Overview tab)`.

### Task 4: The intent tabs

**Files:** `VehicleDetail.svelte` tab bar → **Overview · Timeline · Plan · Builds · Records · Costs**; new `TimelineTab.svelte`, `PlanTab.svelte`, `BuildsTab.svelte`, `RecordsTab.svelte`; existing components re-home.

- **Timeline** (subsumes History + Incidents): the per-vehicle activity feed with kind filter chips (All/Services/Incidents/Mileage), newest-first, "load more" (limit bump). Rows expand to the full existing detail UIs — REUSE `HistoryTab`/`IncidentsTab` internals rather than rewriting: acceptable interim = Timeline renders the merged stream, and expanding a service/incident opens the existing card UI (import the components' detail pieces or render the old tabs' cards inline). Old top-level History/Incidents tabs disappear. Creation actions ("Record service", "Log incident") live here (reuse existing forms).
- **Plan:** sub-nav **Due / To-do / Visits / Schedule ⚙**. Due = ScheduleTab's reminders section (with unit E's affordances + "plan it"). To-do = work items (list by status, create/edit inline, est cost, source badges w/ deep-links, drop). Visits = cards (date/shop incl. **shop select from shopsApi** — carry-in #1, status, item list, est rollup) with attach/detach, complete (form: date/mileage/costs/payer — calls the complete endpoint), cancel. Schedule ⚙ = the existing schedule CRUD (ScheduleTab's config half incl. est_cost_cents input + dismissed-overrides section).
- **Builds:** list + detail w/ `BuildProgress` (counts, installed/total, spend incl. out-of-pocket, linked records deep-linking into Timeline). Create/edit/status transitions (existing HTTP surface).
- **Records:** sub-nav **Parts / Documents / Research** — re-home `PartsTab`/`DocumentsTab`/`ResearchTab` unchanged inside it.
- **Costs:** existing `CostsTab` + forecast buckets line (carry-in: budget endpoint) — modest.

- [ ] Steps: Timeline → Plan → Builds → Records/Costs → check clean after each → commits per tab group: `feat(2hea-f): Timeline tab`, `feat(2hea-f): Plan tab (due/to-do/visits/schedule)`, `feat(2hea-f): Builds + Records + Costs tabs`.

### Task 5: e2e rewrite + docs + gates

- **Playwright:** rewrite the suite for the new shell — navigation spec (sidebar, search, deep-links), dashboard spec (blocks render, plan-it quick action), timeline spec (filters, record service, log incident — port the old history/incidents assertions), plan spec (to-do CRUD, visit schedule→complete round-trip asserting the service record + reminder clear, cancel), builds/records/costs smoke. Port assertions from the old specs where flows survived; delete retired specs. Aim ≈ the current 51 count, honest arithmetic reported.
- **TEST_PLAN.md** rewritten for the new shell; **CLAUDE.md**: tabs, 23 tools, frontend architecture note.
- **Gates:** `just ci` exit 0; `just test-e2e-ci` all green; boot smoke: `/api/dashboard` 200 with real dev-DB data shapes, tools/list 23; **manual-ish sanity via the running app on the dev DB copy**: dashboard renders Steve's real imported data (attention counts, activity feed) without console errors (check the vite/browser logs from the e2e run or a curl of the endpoints).
- [ ] Commit: `test(2hea-f): e2e suite for the new shell + docs`.

## Self-Review
- Decision ⑥ coverage: sidebar (hideable, hints) ✓, garage dashboard landing ✓, scoped Overview ✓, six intent tabs ✓, search in header ✓, garage-cards route dies ✓, mockup fidelity via referenced files ✓. Carry-ins: shop_id ✓ (T1 MCP + T4 visit form), cancel ✓ (T1 verb/route + T4 UI), delete guard ✓ (T1), backlog dedupe + test ✓ (T1), protocol back-link assertion ✓ (T1), forecast buckets in Costs ✓ (T4). Spec ⑧'s "Costs surface" lands here as planned.
- Placeholders: none — reuse-not-rewrite is explicit where old components re-home; read-first on house CSS.
- Types: `GarageDashboard`/`AttentionItem`/`VehicleSummary` named once (T1) and consumed (T2/T3); ActivityItem's additive vehicle fields flagged for serialization check.
