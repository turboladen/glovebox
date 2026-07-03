# Glovebox Test Plan

This is the living test plan for Glovebox. It covers both manual smoke tests and
Playwright e2e tests. Keep it updated as features are added.

**Run with:** `just test-e2e` (Playwright) or walk through manually with `just dev`.

---

## Prerequisites

- Backend running (`cargo run` or `just dev`)
- Frontend dev server running (included in `just dev`)
- Fresh or seeded database (the seed migration provides VW MQB platform data)

---

## TP-01: Garage (Home Page)

### With vehicles (populated DB)

| # | Step | Expected |
|---|------|----------|
| 1 | Navigate to `/` | "Garage" heading visible, "+ Add Car" button present |
| 2 | Vehicle cards | Vehicle cards shown in grid with name, year/make/model, est. mileage |
| 3 | Vehicle card badges | Overdue (red), upcoming (yellow), or "All good" (green) badge per card |
| 4 | Click a vehicle card | Navigates to `/vehicles/:id` and shows vehicle detail |
| 5 | Click "+ Add Car" | Navigates to `/vehicles/new` |

### With no vehicles (fresh DB — Welcome Dashboard)

| # | Step | Expected |
|---|------|----------|
| 1 | Navigate to `/` with empty DB | Welcome dashboard shown (not "Your garage is empty") |
| 2 | Hero banner | "Welcome to Glovebox" heading, "Your precision maintenance tracker" subtitle, Guards Red accent line |
| 3 | Primary CTA | "Add Your First Vehicle" button visible, links to `/vehicles/new` via SPA navigation |
| 4 | Feature showcase grid | Three cards: "Track Maintenance", "Research & Recalls", "Complete History" |
| 5 | Feature card hover | Border turns Guards Red, shadow appears, subtle lift |
| 6 | Quick-start checklist | Three steps shown: "Add your first vehicle" (active), "Add a trusted shop" (greyed), "Log your first service" (greyed) |
| 7 | Checklist active links | Step 1 is clickable, navigates via SPA routing |
| 8 | Checklist greyed items | Steps 2 and 3 are visually muted, not interactive |
| 10 | Responsive layout | Feature cards stack to single column below 640px |
| 11 | Staggered animations | Cards animate in with increasing delay (respects prefers-reduced-motion) |
| 12 | Add a vehicle, return to `/` | Normal vehicle grid shown instead of welcome dashboard |

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
| 3 | New vehicle appears on detail page | Vehicle name in header, "Garage" back link works |
| 4 | Return to `/` | New vehicle card visible in garage grid |
| 5 | Fill all optional fields (year, make, model, trim, engine, transmission, drivetrain, purchase date, purchase mileage) | All fields accepted, vehicle created successfully |

## TP-04: Vehicle Detail Page

| # | Step | Expected |
|---|------|----------|
| 1 | Navigate to `/vehicles/:id` | Vehicle name in heading, back link "← Garage" |
| 2 | Status bar (estimated) | Shows mileage with "mi est." and "as of" date (of last reading) when last entry is older than today |
| 2a | Status bar (exact) | Shows mileage with "mi" (no "est.") and "as of" date when last entry is from today |
| 3 | "Update Mileage" button | Toggles mileage entry form inline |
| 4 | "Log Service" button | Toggles service form inline |
| 4a | "Edit" button | Toggles vehicle edit form inline with all fields pre-populated |
| 4b | Edit form: change name and save | Vehicle name updates in heading; form closes |
| 4c | Edit form: set sold fields | Sold date, price, mileage saved; "Sold" badge appears in header |
| 4d | Edit form: clear sold fields | Sold badge disappears after save |
| 4e | Vehicle subtitle | Year/make/model/trim shown below vehicle name |
| 5 | Schedule tab (default) | Active, shows reminder groups (overdue/upcoming/ok) |
| 6 | History tab | Click switches to service history list |
| 7 | Click "← Garage" | Returns to `/` |

## TP-05: Update Mileage

| # | Step | Expected |
|---|------|----------|
| 1 | Click "Update Mileage" | Form appears with odometer input and notes field |
| 2 | Enter 0 or negative, submit | "Odometer must be greater than 0" error |
| 3 | Enter valid mileage (e.g. 45000), submit | "Saving..." then form closes, mileage in status bar updates, shows "mi" (no "est." since entry is from today) |
| 4 | Click "Cancel" | Form closes without saving |

## TP-06: Log Service

| # | Step | Expected |
|---|------|----------|
| 1 | Click "Log Service" | Form appears: date (today default), odometer, description, cost, shop, notes |
| 2 | Date field defaults to today | Pre-filled with current date |
| 3 | Fill description "Oil Change", cost "49.99", odometer 45200, submit | "Saving..." then form closes |
| 4 | Switch to History tab | New service record visible with date, description, cost ($49.99), mileage |
| 5 | Schedule items checkboxes | If vehicle has schedule, checkboxes shown; selecting items links service to schedule |
| 6 | Click "Cancel" | Form closes without saving |
| 7 | "Paid By" select (default Me) | Choosing Insurance/Third party reveals a "Payer Note" input; record shows "Paid by: …" when expanded in History |

## TP-07: Schedule Tab (Reminders)

| # | Step | Expected |
|---|------|----------|
| 1 | Vehicle with seeded schedule data | Reminders grouped by overdue (red border), upcoming (yellow), ok (green) |
| 2 | Each reminder shows | Item name, due mileage, due date, miles/days remaining |
| 3 | Overdue items | Show "Last: [date] @ [mileage]" or "No service recorded" |
| 4 | Bundle suggestions | Dashed-border cards shown when items are due near each other |

## TP-08: History Tab

| # | Step | Expected |
|---|------|----------|
| 1 | No service records | "No service records yet." message |
| 2 | With records | Chronological list with date, cost, description, mileage, shop, notes |
| 3 | Cost formatting | Displays as "$XX.XX" (converted from cents) |

## TP-09: VIN Decode (API)

| # | Step | Expected |
|---|------|----------|
| 1 | `GET /api/vin/1VWCA7A35KC123456` | Returns decoded VIN data (year, make, model, etc.) from NHTSA |
| 2 | Invalid VIN (wrong length) | "Decode VIN" button stays disabled (frontend), 400 error (API) |
| 3 | Non-alphanumeric chars in VIN | 400 error (API) |

## TP-10: Navigation & Routing

| # | Step | Expected |
|---|------|----------|
| 1 | Click "Glovebox" logo | Returns to `/` via SPA navigation (no full page reload) |
| 2 | Navigate to `/nonexistent` | 404 page: "Page not found." with "Back to Garage" link |
| 3 | Direct URL `/vehicles/1` | Loads vehicle detail (no 404 flash) |
| 4 | Browser back/forward | SPA routing works without full page reload |

## TP-11: Dark Mode

| # | Step | Expected |
|---|------|----------|
| 1 | System set to dark mode | Background dark (#1a1a2e), text light, inputs/surfaces themed |
| 2 | System set to light mode | Background white, standard light styling |
| 3 | Forms in dark mode | Inputs have dark background with light text, borders visible |

## TP-12: API Health & Edge Cases

| # | Step | Expected |
|---|------|----------|
| 1 | `GET /api/health` | 200 OK |
| 2 | `GET /api/vehicles` with empty DB | Returns `[]` |
| 3 | `GET /api/vehicles/99999` | 404 error |
| 4 | `POST /api/vehicles` with empty body | 400/422 error |
| 5 | `GET /api/ai/status` | 404 (in-app AI retired; Claude connects over `/mcp`) |

## TP-13: Shops

| # | Step | Expected |
|---|------|----------|
| 1 | `GET /api/shops` with empty DB | Returns `[]` |
| 2 | `POST /api/shops` with name | Shop created, returned with id |
| 3 | `GET /api/shops/:id` | Returns shop details |
| 4 | `PUT /api/shops/:id` | Updates shop fields |
| 5 | Service record with shop_id | Links service to a shop entity |

## TP-14: Incidents (interim tab)

| # | Step | Expected |
|---|------|----------|
| 1 | Click "Incidents" tab on vehicle detail | Incidents tab visible with "+ Add Incident" button |
| 2 | No incidents | "No incidents yet." message |
| 3 | Add incident (fill title, category) | Form submits, incident appears in list |
| 4 | Incident shows category badge and date | Category in uppercase, date formatted |
| 5 | OBD code category shows code input | Extra field for JSON array of codes |
| 6 | Category `accident` reveals accident fieldset | Other-party + insurance claim fields shown; values render on the expanded card |
| 7 | "Mark Resolved" button on expanded card | Toggles resolved state, card becomes dimmed; "Reopen" reverts |
| 8 | Add followup to an expanded incident | Followup entry appears with date, method, and summary |
| 9 | Incidents appear in History tab | Interleaved with services, tagged "Incident" |
| 10 | Category filter chips | Filter the list to one category |

## TP-15: Documents & Upload

| # | Step | Expected |
|---|------|----------|
| 1 | Click "Docs" tab on vehicle detail | Documents tab visible with "Upload" button |
| 2 | No documents | "No documents yet." message |
| 3 | Upload a file (select file, set type) | File uploaded, appears in list with filename and size |
| 4 | Image document | Shows inline thumbnail preview |
| 5 | "View" button | Opens file in new tab via `/files/` path |
| 6 | "Delete" button | Removes document from list and deletes file from disk |
| 7 | Document with doc_type | Type badge shown (invoice, receipt, photo, etc.) |

## TP-16: Incidents API

| # | Step | Expected |
|---|------|----------|
| 1 | `POST /api/vehicles/:id/incidents` | Creates incident; category outside the whitelist is a 400 naming valid categories |
| 2 | `GET /api/vehicles/:id/incidents` | Returns list with followups and service_record_ids, occurred_at descending |
| 3 | `POST /api/vehicles/:id/incidents/:id/followups` | Followup created with contact method and summary; wrong-vehicle incident 404s |
| 4 | Update incident with cost/resolution | Fields updated, resolved flag toggled |
| 5 | Link service records to incident | service_record_ids populated; cross-vehicle service 404s and mutates nothing |
| 6 | `recurrence_of_id` | Same-vehicle incident links; cross-vehicle/nonexistent 404s |

## TP-17: Interleaved Timeline

| # | Step | Expected |
|---|------|----------|
| 1 | History tab with both services and incidents | Both types shown, sorted by date descending |
| 2 | Service entries | Tagged "Service" (green badge), show cost and mileage |
| 3 | Incident entries | Tagged "Incident" (yellow badge), show resolved status |
| 4 | Filter: "All" | Shows both types |
| 5 | Filter: "Services" | Shows only service records |
| 6 | Filter: "Incidents" | Shows only incidents |

## TP-18: Parts Tab

| # | Step | Expected |
|---|------|----------|
| 1 | Click "Parts" tab on vehicle detail | Parts tab visible with "+ Add Part" button |
| 2 | No parts | "No parts yet." message |
| 3 | Add a part (name, location, cost) | Part card appears with location tag, cost, status badge "purchased" |
| 4 | Edit part status to "installed" | Status badge changes, install date/odometer shown |
| 5 | Edit a part's location | Form pre-fills existing location; updated location shown on the card |
| 6 | Installed part with "Create new service" | Inline service created; part card shows "via service <date> — <description>" |
| 7 | Delete a part | Part removed from list |

## TP-19: Cost of Ownership

| # | Step | Expected |
|---|------|----------|
| 1 | Click "Costs" tab on vehicle detail | Cost of Ownership heading visible |
| 2 | No service records or parts | "No cost data yet." message |
| 3 | With services and parts | Summary cards: Total Spent, Out of Pocket, Services, Parts, Labor |
| 4 | Cost per mile | Shown when vehicle has purchase_mileage and service mileage |
| 5 | Monthly breakdown table | Rows with month, service cost, parts cost, out of pocket, covered, total |
| 6 | Service paid by insurance/third party | "Covered by Others" card appears; covered amount excluded from Out of Pocket (totals and monthly rows) |

## TP-20: Export Service History

| # | Step | Expected |
|---|------|----------|
| 1 | Click "Export History" on vehicle detail | New window/tab opens with printable history |
| 2 | Export includes vehicle info | Name, year/make/model, VIN shown at top |
| 3 | Service records table | Date, mileage, description, cost, shop columns |
| 4 | Installed parts table | Part name, manufacturer, part #, install date, mileage, cost |
| 5 | Totals section | Services total, parts total, grand total |
| 6 | Print button | Window.print() triggered, @media print hides button |

## TP-21: Link Parts to Services

| # | Step | Expected |
|---|------|----------|
| 1 | Log Service with purchased parts available | "Parts installed during this service" checkbox list shown |
| 2 | Select parts and save service | Parts marked as "installed" with service date/mileage |
| 3 | Parts status updated | On Parts tab, linked parts show "installed" badge |

## TP-22 – TP-25: Retired (in-app AI removed)

The in-app AI features these sections covered are retired; Claude replaces them over `/mcp`:

- **TP-22 AI Provider Management** — removed; there is no provider config (MCP client brings its own model).
- **TP-23 AI Chat** — removed; chat happens in the MCP client (e.g. Claude) using the `/mcp` tools.
- **TP-24 Invoice PDF Parsing** — removed; document upload + extracted text stay (TP-15), extraction questions go through MCP `find_documents`.
- **TP-25 Proactive Suggestions** — removed; MCP `check_due_maintenance` answers "what does it need?".

MCP surface coverage lives in `glovebox-mcp/tests/mcp_integration_test.rs` (15 tools, incl. `file_research_finding`).

## TP-26: NHTSA Recall Check

| # | Step | Expected |
|---|------|----------|
| 1 | Click "Research" tab on vehicle detail | Research tab visible with "Check Recalls" button |
| 2 | No data initially | "No research reports yet." message |
| 3 | Click "Check Recalls" | Loading state, then recall results or "No open recalls found" message |
| 4 | Vehicle missing make/model/year | Error message about missing required fields |
| 5 | `GET /api/vehicles/:id/recalls` | Returns recall data with campaign numbers, subjects, remedies |

## TP-27: Research Reports & Findings

Reports are created by recall checks (`recalls_only`) and by findings filed over MCP
(`file_research_finding` → `external_research` anchor report). There is no in-app
"generate report" action anymore.

| # | Step | Expected |
|---|------|----------|
| 1 | Report shows summary | Summary text describes the report contents |
| 2 | Findings grouped by category | Section headers for Recall, Suggested Maintenance, Forum Report, Upgrade Idea |
| 3 | Severity filter chips | Critical, recommended, optional, informational toggle chips; clicking filters findings |
| 4 | Each finding shows | Severity badge, status badge, title, description |
| 5 | Sources/citations | Collapsible "Sources" disclosure on findings with source_url |
| 6 | Finding status actions | "Dismiss", "Plan", "Complete" buttons update status |
| 7 | Dismissed findings | Card becomes dimmed |
| 8 | Completed findings | Card border turns green, opacity reduced |
| 9 | Reports list | Reports shown with type, date, and summary preview |
| 10 | Click a report row | Expands to show findings for that report |
| 11 | `GET /api/vehicles/:id/research` | Returns list of reports ordered by generated_at desc |
| 12 | `GET /api/vehicles/:id/research/:id` | Returns report with findings array |
| 13 | Complete with linking | "Complete" opens picker to link finding to service record or part |
| 14 | Complete without linking | "Complete without linking" option marks done without association |
| 15 | Linked finding display | Completed + linked findings show "Linked to service #N" label |
| 16 | `PUT /api/vehicles/:id/research/:rid/findings/:id` | Updates finding status and linked entity |
| 17 | MCP `file_research_finding` | Filed finding appears under an "External Research" report in this tab |

---

## Shared-Service Unit Tests (glovebox-shared)

Domain logic lives in `glovebox-shared/src/services/` and is unit-tested there, below the
Playwright layer.

**Harness:** `glovebox_shared::test_support::test_db()` returns a fresh in-memory SQLite
`DatabaseConnection` with all migrations applied. The pool is pinned to a single connection
(`sqlite::memory:` databases are per-connection, so a second pooled connection would see an
empty schema); each test gets its own isolated DB.

```rust
#[cfg(test)]
mod tests {
    use crate::test_support::test_db;

    #[tokio::test]
    async fn create_then_get_round_trips() {
        let db = test_db().await;
        // call service fns against `db` ...
    }
}
```

**Covered here (no UI yet):** `services::search::search()` (FTS5 full-text search, `GET
/api/search`) is unit-tested in `glovebox-shared/src/services/search.rs` — kind/id/vehicle_id
correctness, scope + vehicle filtering, trigger sync on update/delete, line-item folding,
FTS5 operator/quote injection safety, empty-query and missing-vehicle rejection.

**Convention:** new domain logic gets service-level unit tests in the shared crate — at
minimum a create→get round-trip, an update-mutates-field test, and a rejection test per
validation rule — in addition to Playwright e2e coverage of the user flow.

Run: `cargo test -p glovebox-shared` (or `cargo test --workspace`).

---

## Playwright Test Structure

Tests live in `frontend/e2e/` and mirror this plan:

```
frontend/e2e/
  garage.spec.ts        # TP-01
  vehicle-new.spec.ts   # TP-02, TP-03
  vehicle-detail.spec.ts # TP-04, TP-05, TP-06
  schedule.spec.ts      # TP-07
  history.spec.ts       # TP-08
  navigation.spec.ts    # TP-10
  incidents.spec.ts     # TP-14
  documents.spec.ts     # TP-15
  parts.spec.ts         # TP-18, TP-19, TP-21
  research.spec.ts      # TP-26, TP-27
```

Run: `just test-e2e`

---

## Maintaining This Plan

When adding new features:
1. Add a new `TP-XX` section covering the feature's user flows
2. Write corresponding Playwright tests in `frontend/e2e/`
3. Update existing sections if behavior changes
