# Glovebox Test Plan

This is the living test plan for Glovebox. It covers both manual smoke tests and
Playwright e2e tests. Keep it updated as features are added.

**Run with:** `just test-e2e` (Playwright) or walk through manually with `just dev`.

The shell follows 2hea decision ⑥: login lands on a **garage-wide dashboard**,
vehicles live in a **collapsible sidebar**, the per-car view is the same
dashboard scoped as an **Overview** tab, and the vehicle tabs are intent-shaped:
**Overview · Timeline · Plan · Builds · Records · Costs**.

---

## Prerequisites

- Backend running (`cargo run` or `just dev`)
- Frontend dev server running (included in `just dev`)
- Fresh or seeded database (the seed migration provides VW MQB platform data)

---

## TP-00: Garage Dashboard (Home Page)

### With vehicles

| # | Step | Expected |
|---|------|----------|
| 1 | Navigate to `/` | "Garage" heading; dashboard blocks render |
| 2 | Needs attention block (red tint) | One row per overdue/due-soon reminder, open recall, unresolved incident — vehicle-labeled; each deep-links WITH a `?hl=` param (reminder → Plan/Due, recall → Plan/Research, incident → Timeline) so the target row scrolls into view and flashes |
| 3 | "plan it" on an overdue/due-soon/recall row | Creates a source-linked work item; row flips to a "planned" chip that LINKS to the work item (`plan/todo?hl=work_item:{id}`), with a confirm-free ✕ (Un-plan) beside it that deletes the item and restores "plan it" |
| 4 | Plan & budget block (blue tint) | Upcoming visits with date/shop/est rollup (→ Plan/Visits), unscheduled to-do count, 12-mo forecast total (garage-wide sum) |
| 5 | Builds block (green tint, conditional) | One row per ACTIVE build: vehicle · name · parts installed/total · spend (→ Builds tab) |
| 6 | Recent activity block | Cross-vehicle merged feed (services/incidents/mileage), vehicle-labeled, newest first; rows deep-link to the vehicle's Timeline |

### With no vehicles (fresh DB — Welcome state)

| # | Step | Expected |
|---|------|----------|
| 1 | Navigate to `/` with empty DB | "Welcome to Glovebox" hero + Guards Red accent + "Add Your First Vehicle" CTA |
| 2 | Quick-start checklist | "Add your first vehicle" (active link), "Add a trusted shop" and "Log your first service" greyed |

## TP-01: Shell (Header + Sidebar)

| # | Step | Expected |
|---|------|----------|
| 1 | Header | Logo (→ `/`), global search input, sidebar toggle (hamburger), shops icon |
| 2 | Sidebar | "Garage" label, "All vehicles" entry (active on `/`), one card per vehicle: name, year/make/model, mileage + status hints ("N due" red — a LINK to that vehicle's Plan/Due view (card nav unaffected), "N soon" amber, "recall", "build active" green chip) |
| 3 | Click a sidebar vehicle | Navigates to `/vehicles/:id` (Overview); entry gains active state; switching cars reloads in place |
| 4 | Archived vehicles | Collapsed "Archived (n)" group; entries dimmed |
| 5 | "+ Add vehicle" at the sidebar bottom | Navigates to `/vehicles/new` |
| 6 | Hamburger toggle | Sidebar fully hides; a slim reopen handle appears at the left edge; state persists across reloads (localStorage) |
| 7 | Global search (type ≥1 word) | Grouped results dropdown (Vehicles/Services/Incidents/Builds/Documents/Research); every hit deep-links: service/incident → Timeline (`?hl=` highlights the row), document → Records/Documents, research → Plan/Research (`?hl=finding:…` expands + highlights), build → Builds, vehicle → Overview |

## TP-02: Add Vehicle (VIN Flow)

| # | Step | Expected |
|---|------|----------|
| 1 | Navigate to `/vehicles/new` | Step 1 shown: "Enter VIN (optional)" |
| 2 | "Decode VIN" button disabled when input < 17 chars | Button stays disabled |
| 3 | Enter valid 17-char VIN, click "Decode VIN" | Loading state shown, then Step 2 auto-fills year/make/model/trim/engine/transmission/drivetrain |
| 4 | Vehicle name auto-generated | e.g. "2019 Volkswagen GTI" |
| 5 | Click "Skip" on Step 1 | Jumps to Step 2 with all fields empty |
| 6 | Step 2: "Back" button | Returns to Step 1, VIN input preserved |

## TP-03: Add Vehicle (Creation)

| # | Step | Expected |
|---|------|----------|
| 1 | On Step 2, leave name empty, submit | "Vehicle name is required" error shown |
| 2 | Fill in name (e.g. "Test Car"), submit | "Creating..." shown, then redirects to `/vehicles/:id` |
| 3 | New vehicle appears on detail page | Vehicle name in header, "← All vehicles" back link works |
| 4 | Return to `/` | New vehicle appears in the sidebar and dashboard data |
| 5 | Fill all optional fields | All fields accepted, vehicle created successfully |

## TP-04: Vehicle Detail Shell (Overview + intent tabs)

| # | Step | Expected |
|---|------|----------|
| 1 | Navigate to `/vehicles/:id` | Vehicle name heading, "← All vehicles" back link, status bar (mileage readout + two equal-weight actions: Update mileage / Record service, plus a ⋯ overflow menu: Edit vehicle… / Export history / Archive vehicle… — Unarchive vehicle / Delete vehicle… when archived) |
| 1a | ⋯ overflow menu | Click-toggled; closes on outside click and Esc |
| 1b | "Record service" | Routes to Timeline with the service form open (`?action=record`) — ONE record-service verb and ONE form app-wide (no "Log Service" anywhere) |
| 2 | Default tab | **Overview** active: the SAME dashboard blocks scoped to this vehicle (rows not vehicle-labeled; other cars' items absent) |
| 3 | Tab bar | Overview · Timeline · Plan · Builds · Records · Costs |
| 4 | Tabs are URL-driven | Clicking a tab pushes `/vehicles/:id/<tab>`; direct URLs (`/timeline`, `/plan/visits`, `/plan/research`) land on that view; legacy `/records/research` URLs redirect to `/plan/research` |
| 4b | Unknown `:tab` / `:sub` URLs | Fall back instead of a blank pane: bogus tab → Overview (rendered AND marked active); bogus Plan sub → Due; bogus Records sub → Parts |
| 4a | "Edit vehicle…" (⋯ menu) | Toggles vehicle edit form; name/subtitle/sold-badge behavior as before (clearing sold fields sends explicit null and removes the badge) |
| 5 | Status bar mileage | "mi est." with as-of date when estimated; plain "mi" after a same-day reading |
| 6 | Click "← All vehicles" | Returns to `/` |

## TP-05: Update Mileage

| # | Step | Expected |
|---|------|----------|
| 1 | Click "Update mileage" | Form appears with odometer input and notes field |
| 2 | Enter 0 or negative, submit | "Odometer must be greater than 0" error |
| 3 | Enter valid mileage, submit | Form closes, status bar updates, shows "mi" (no "est.") |
| 4 | Click "Cancel" | Form closes without saving |

## TP-06: Timeline Tab

One chronological stream subsuming the old History + Incidents tabs, plus
manual odometer readings. Creation actions live here.

| # | Step | Expected |
|---|------|----------|
| 1 | Empty vehicle | "No history yet." + "Record service" / "Log incident" buttons |
| 2 | Filter chips | All / Services / Incidents / Mileage narrow the stream by kind |
| 2a | Incidents filter active | A second chip row appears (All + the 10 incident categories) narrowing the incident rows by category; hidden for other kind filters |
| 3 | "Record service" | The service form (date defaults today, odometer/description/cost/shop/notes, schedule-item checkboxes, Paid By with payer note); saved record appears in the stream with cost |
| 4 | "Log incident" | Incident form (category/date/odometer/title/details); category `accident` reveals the accident fieldset (other party, claim, adjuster); category `obd_code` reveals the codes input |
| 5 | Service rows | Green "Service" badge; date, cost, mileage, shop; preview shows notes + linked Parts/Incidents chips |
| 6 | Expand a service row | Detail panel: parts/labor costs, payer, chips; **Edit** (all fields incl. Paid By; clearing a field saves null) and **Delete** (confirm). The row and its panel read as ONE contiguous card (border on the wrapper, hairline separator between the halves — no split-box seam) |
| 7 | Incident rows | Amber "Incident" badge; expanding shows the full detail: description, recurrence, odometer, OBD chips, accident grid, linked-services chips, followups |
| 8 | Incident detail actions | Edit (opens the form pre-filled; accident financial fields are edit-only), Mark Resolved (with optional service link picker), Reopen |
| 9 | Add followup | Date/method/contact/summary; entry appears in the followup timeline |
| 10 | Resolving with a service | Incident shows "Services:" chip; the linked service row shows an "Incidents:" chip |
| 11 | Mileage rows | Blue "Mileage" badge + reading; manual logs only (service-created logs are folded into their service row) |
| 12 | Load more | Stream windows at 25 rows; button reveals older entries |

## TP-07: Plan Tab (Due / To-do / Visits / Research / Schedule ⚙)

URL-driven sub-nav: `/vehicles/:id/plan[/todo|/visits|/research|/schedule]`.
Deep links into Plan may carry `?hl=<kind>:<id>` (hypermedia highlight — the
matching row scrolls into view and flashes for ~2s; see
`frontend/src/lib/highlight.ts`).

### Due (reminders)

| # | Step | Expected |
|---|------|----------|
| 1 | Vehicle with schedule data | Reminders grouped overdue (red) / upcoming (amber) / ok (green); bundle suggestions |
| 2 | "Plan it" | Creates a schedule-linked work item; the reminder shows a "planned" chip that LINKS to the work item (`plan/todo?hl=work_item:{id}`) instead of the button |
| 3 | "Record service…" | Inline minimal form (today prefilled); save links the schedule item and the reminder clears |
| 4 | "Mark done previously" | Past-dated backfill record; reminder clears; record is real Timeline history |
| 5 | "Dismiss for this vehicle" | Item leaves the reminder groups (the override lives under Schedule ⚙) |

### To-do (work items)

| # | Step | Expected |
|---|------|----------|
| 1 | Empty | Hint text + "+ Add work item" |
| 2 | Add item (title, est cost, notes) | Card with status badge (planned), est cost |
| 3 | Edit inline | Title/est/notes update; clearing est cost sends explicit null |
| 4 | Source badges | schedule / recall/finding / incident / build badges deep-link to the item's origin (with `?hl=` so the origin row highlights: schedule → Plan/Due, finding → Plan/Research, incident → Timeline) |
| 5 | Drop | Item leaves the open list; visible with "Show finished" (status dropped) |
| 6 | Delete | Item removed outright (confirm) |
| 7 | Attached items | Show a "visit …" reference |

### Visits

| # | Step | Expected |
|---|------|----------|
| 1 | "+ Schedule visit" | Form: planned date, shop **select from the shops list** or free-text name, notes, checkbox list of open work items. A SELECTED shop is authoritative — its name is stored as the visit's shop name (the free-text field is inert while a shop is selected; editing and picking a different shop replaces the name) |
| 2 | Created visit card | Status badge, date · shop, attached item list, est rollup (Σ item estimates) |
| 3 | Edit / attach items | Replace-all attach semantics; detached items return to the backlog |
| 4 | "Complete…" | Actuals form (date/odometer/total/parts/labor/paid-by + payer note); completing creates the service record, clears satisfied reminders, closes linked recalls/incidents, marks items done — visit leaves the open list |
| 5 | Completed visit's record | On the Timeline with the visit's shop and the items' joined titles |
| 6 | "Cancel visit" | Visit closes; items return to the to-do list as planned |
| 7 | "Show closed" | Completed/canceled visits visible, dimmed |

### Schedule ⚙ (config)

| # | Step | Expected |
|---|------|----------|
| 1 | Resolved items list | Name, intervals ("every N mi / every N mo"), est cost per occurrence; inherited items labeled "from <source>" (read-only) |
| 2 | "+ Add item" | Vehicle-level item: name, interval miles/months, est cost ($ → cents), notes |
| 3 | Edit / Delete | Vehicle-owned items only; clearing a field saves null |
| 4 | Dismissed section | Vehicle-level overrides with "overridden" badge; "Re-enable" restores the item to Due |
| 5 | est_cost_cents | Feeds the 12-month forecast (Costs tab buckets + dashboard totals) |

## TP-08: Builds Tab

| # | Step | Expected |
|---|------|----------|
| 1 | Empty | Explainer + "+ New build" |
| 2 | Create (name, description, target date) | Card with "planned" status badge |
| 3 | Expand a card | Progress detail: parts installed/total, services, incidents, total spend, out-of-pocket; linked-record buttons deep-link (services/incidents → Timeline, parts → Records/Parts) |
| 4 | Status select | planned/active/on_hold/completed/abandoned; entering completed stamps the date |
| 5 | Active build | Sidebar shows the "build active" chip; dashboard Builds block lists it |
| 6 | Delete | Build removed; linked records keep their history (build link clears) |

## TP-09: VIN Decode (API)

| # | Step | Expected |
|---|------|----------|
| 1 | `GET /api/vin/1VWCA7A35KC123456` | Returns decoded VIN data (year, make, model, etc.) from NHTSA |
| 2 | Invalid VIN (wrong length) | "Decode VIN" button stays disabled (frontend), 400 error (API) |
| 3 | Non-alphanumeric chars in VIN | 400 error (API) |

## TP-10: Navigation & Routing

| # | Step | Expected |
|---|------|----------|
| 1 | Click "Glovebox" logo | Returns to `/` via SPA navigation |
| 2 | Navigate to `/nonexistent` | 404 page with "Back to Garage" link |
| 3 | Direct URL `/vehicles/1` (and any tab/sub URL) | Loads the right view, no 404 flash |
| 4 | Browser back/forward | SPA routing works; tab changes are history entries |

## TP-11: Dark Mode

| # | Step | Expected |
|---|------|----------|
| 1 | System set to dark mode | Dark background, themed inputs/surfaces; block tints stay legible |
| 2 | System set to light mode | Light styling |

## TP-12: API Health & Edge Cases

| # | Step | Expected |
|---|------|----------|
| 1 | `GET /api/health` | 200 OK |
| 2 | `GET /api/vehicles` with empty DB | Returns `[]` |
| 3 | `GET /api/vehicles/99999` | 404 error |
| 4 | `POST /api/vehicles` with empty body | 400/422 error |
| 5 | `GET /api/dashboard` with empty DB | 200 with empty arrays and `budget_total_cents: 0` |

## TP-13: Shops

| # | Step | Expected |
|---|------|----------|
| 1 | `GET /api/shops` with empty DB | Returns `[]` |
| 2 | `POST /api/shops` with name | Shop created, returned with id |
| 3 | `GET /api/shops/:id` | Returns shop details |
| 4 | `PUT /api/shops/:id` | Updates shop fields |
| 5 | Visit/service with shop_id | Links to the shop entity; the visit form offers the shops list |

## TP-14: Dashboard & Activity API

| # | Step | Expected |
|---|------|----------|
| 1 | `GET /api/dashboard` | Per-vehicle summaries (counts, forecast total, active build), attention items (kind/label/schedule_item_name/entity_id/deep_link_hint/planned/planned_work_item_id — `schedule_item_name` set for overdue/due-soon rows so "plan it" titles the work item without label-splitting; `planned_work_item_id` names the linking open work item so the "planned" chip can link to it and un-plan it), upcoming visits, summed budget, active-build snapshots |
| 2 | Archived vehicles | Listed with zeroed counts; contribute nothing to attention/budget |
| 3 | `GET /api/dashboard/activity?limit=N` | Garage-wide merged feed, vehicle-labeled, newest first, capped at N |
| 4 | `GET /api/vehicles/:id/activity?limit=N` | Per-vehicle feed (Timeline's stream); service-created mileage logs excluded |

## TP-15: Records → Documents

| # | Step | Expected |
|---|------|----------|
| 1 | Records tab → Documents sub-view | Upload button visible |
| 2 | No documents | "No documents yet." message |
| 3 | Upload a file (select file, set type) | File uploaded, appears with filename and size |
| 4 | Image document | Inline thumbnail preview |
| 5 | "View" | Opens file via `/files/` path |
| 6 | "Delete" | Removes document and file |

## TP-16: Incidents API

| # | Step | Expected |
|---|------|----------|
| 1 | `POST /api/vehicles/:id/incidents` | Creates incident; category outside the whitelist is a 400 naming valid categories |
| 2 | `GET /api/vehicles/:id/incidents` | Returns list with followups and service_record_ids, occurred_at descending |
| 3 | `POST /api/vehicles/:id/incidents/:id/followups` | Followup created; wrong-vehicle incident 404s |
| 4 | Update incident with cost/resolution | Fields updated, resolved flag toggled |
| 5 | Link service records to incident | service_record_ids populated; cross-vehicle service 404s and mutates nothing |
| 6 | `recurrence_of_id` | Same-vehicle links; cross-vehicle/nonexistent 404s |

## TP-17: Planning API (work items + visits)

| # | Step | Expected |
|---|------|----------|
| 1 | `GET/POST /api/vehicles/:id/work-items` (`?include_done`) | List/create; source links vehicle-scoped (cross-vehicle 404s) |
| 2 | `PUT/DELETE /api/vehicles/:id/work-items/:id` | Double-option updates (explicit null clears); status whitelist |
| 3 | `GET/POST /api/vehicles/:id/visits` (`?include_closed`) | List with items + est rollups; create with attach; a nonexistent `shop_id` is a 404 ("Shop N not found") at write time, not a deferred FK error |
| 4 | `PUT /api/vehicles/:id/visits/:id` | Replace-all item attach; closed visits immutable (400); `shop_id` existence-checked like create |
| 5 | `POST …/visits/:id/complete` | One transaction: service record, reminder clears, recalls/incidents close, items done. A since-deleted shop degrades to `shop_id: null` on the record (shop name snapshot kept) instead of failing the close |
| 6 | `POST …/visits/:id/cancel` | Status canceled; items back to the backlog |
| 7 | `DELETE …/visits/:id` | Open visits only; completed/canceled are frozen history (400) |

## TP-18: Records → Parts

| # | Step | Expected |
|---|------|----------|
| 1 | Records tab (Parts is the default sub-view) | "+ Add Part" button |
| 2 | No parts | "No parts yet." message |
| 3 | Add a part (name, location, cost) | Card with location tag, cost, "purchased" badge |
| 4 | Edit status to "installed" | Badge changes, install date/odometer shown |
| 5 | Edit a part's location | Form pre-fills; updated location shown |
| 6 | Installed part with "Create new service" | Inline service created; card shows "via service …" |
| 7 | Delete a part | Removed from list |

## TP-19: Costs Tab

| # | Step | Expected |
|---|------|----------|
| 1 | Costs tab | "Cost of Ownership" heading |
| 2 | No spend history | "No cost data yet." message (forecast still shows if a schedule exists) |
| 3 | With services and parts | Summary cards: Total Spent, Out of Pocket, Services, Parts, Labor |
| 4 | Cost per mile | Shown when derivable |
| 5 | Monthly breakdown table | Month, service cost, parts cost, out of pocket, covered, total |
| 6 | Insurance/third-party services | "Covered by Others" card; excluded from Out of Pocket |
| 7 | Forecast buckets | "Next 12 Months (forecast)": Projected Maintenance / Planned Visits / To-do Backlog / Total — each dollar in exactly one bucket |

## TP-20: Export Service History

| # | Step | Expected |
|---|------|----------|
| 1 | ⋯ menu → "Export history" | Printable history opens (vehicle info, records table, parts table, totals, print button) |

## TP-21: Link Parts to Services

| # | Step | Expected |
|---|------|----------|
| 1 | Record service with purchased parts available | "Parts installed during this service" checkbox list |
| 2 | Select parts and save | Parts marked installed with service date/mileage |
| 3 | Records → Parts | Linked parts show "installed" badge |

## TP-22 – TP-25: Retired (in-app AI removed)

The in-app AI features these sections covered are retired; Claude replaces them
over `/mcp` (23 tools). MCP surface coverage lives in
`glovebox-mcp/tests/mcp_integration_test.rs`.

## TP-26: Plan → Research (NHTSA Recall Check)

| # | Step | Expected |
|---|------|----------|
| 1 | Plan tab → Research sub-view (`/plan/research`) | "Check Recalls" button; legacy `/records/research` URLs redirect here |
| 2 | No data initially | "No research reports yet." message |
| 3 | Click "Check Recalls" | Loading, then recall results or "No open recalls found" |
| 4 | Vehicle missing make/model/year | Error about missing required fields |
| 5 | `GET /api/vehicles/:id/recalls` | Returns recall data |
| 6 | Open recall findings | Surface in the dashboard's attention block with a "plan it" action; the row deep-links to `plan/research?hl=finding:{id}`, which expands the finding's report and highlights it |

## TP-27: Research Reports & Findings

Unchanged from before the shell redesign, now under Plan → Research (moved
from Records in the UX quick-wins pass — research is future work): reports
from recall checks and MCP `file_research_finding`; findings grouped by
category with severity/status chips, dismiss/plan/complete actions, and
service/part linking. (See `frontend/src/components/ResearchTab.svelte`.)

---

## Shared-Service Unit Tests (glovebox-shared)

Domain logic lives in `glovebox-shared/src/services/` and is unit-tested there, below the
Playwright layer.

**Harness:** `glovebox_shared::test_support::test_db()` returns a fresh in-memory SQLite
`DatabaseConnection` with all migrations applied. The pool is pinned to a single connection
(`sqlite::memory:` databases are per-connection, so a second pooled connection would see an
empty schema); each test gets its own isolated DB.

**Covered here (no UI):** full-text search internals (`services/search.rs`), the planning
primitives incl. the `visit::complete` rollback proof (`services/{work_item,visit}.rs`),
budget forecast bucket arithmetic incl. the open-visit AND backlog first-occurrence dedupe
(`services/budget.rs`), the garage dashboard aggregation (`services/dashboard.rs`), and the
merged activity feed incl. `recent_all` (`services/activity.rs`).

**Convention:** new domain logic gets service-level unit tests in the shared crate — at
minimum a create→get round-trip, an update-mutates-field test, and a rejection test per
validation rule — in addition to Playwright e2e coverage of the user flow.

Run: `cargo test -p glovebox-shared` (or `cargo test --workspace`).

---

## Playwright Test Structure

Tests live in `frontend/e2e/` and mirror this plan:

```
frontend/e2e/
  helpers.ts             # createVehicle / seedOverdueItem
  navigation.spec.ts     # TP-01, TP-10 (shell: sidebar, search, routing)      (8)
  dashboard.spec.ts      # TP-00, TP-04 (garage + scoped Overview, plan-it)    (7)
  vehicle-new.spec.ts    # TP-02, TP-03                                        (6)
  vehicle-detail.spec.ts # TP-04, TP-05 (shell, edit, mileage, tab fallbacks) (15)
  timeline.spec.ts       # TP-06 (stream, service + incident flows, filters)  (12)
  plan.spec.ts           # TP-07, TP-26 (due, to-do, visits, research, config)(13)
  builds.spec.ts         # TP-08                                               (3)
  records.spec.ts        # TP-15, TP-18, TP-19 smoke                          (10)
```

**Count reconciliation (shell rewrite, kept honest):** the pre-shell suite was
51 tests in 8 files (garage 3, navigation 3, vehicle-new 6, vehicle-detail 18,
incidents 8, parts 7, documents 3, research 3). The shell rewrite landed at 64:
flows that survived were ported (incidents/parts/documents/research under their
new homes; vehicle-detail's tab assertions rewritten for the intent tabs),
`garage.spec.ts` was retired with the garage-cards view, and dashboard/plan/
builds specs are new. Two old assertions were dropped in that port and have
since been restored: the add-vehicle affordance CLICK-THROUGH to
`/vehicles/new` (now in dashboard.spec) and the service-form cancel/toggle path
(now in timeline.spec). With those two restored plus three review-fix
regression tests (incident category chips, shop-select-authoritative visit
form, unknown tab/sub fallback), the suite reached **69 tests**.

**UX quick-wins pass (hypermedia affordances):** the 3 research tests moved
from records.spec to plan.spec (Research now lives under Plan), and 5 tests
were added — the sidebar due-badge click-through (navigation), the plan-it →
chip-link → highlight → un-plan round trip (dashboard), the legacy
`records/research` redirect (plan), the header Record-service routing, and the
one-verb no-"Log Service" regression (both vehicle-detail) — bringing the
suite to **74 tests**.

Run: `just test-e2e` (needs `just dev` running) or `just test-e2e-ci` (self-contained).

---

## Maintaining This Plan

When adding new features:
1. Add a new `TP-XX` section covering the feature's user flows
2. Write corresponding Playwright tests in `frontend/e2e/`
3. Update existing sections if behavior changes
