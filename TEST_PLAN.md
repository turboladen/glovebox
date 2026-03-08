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

| # | Step | Expected |
|---|------|----------|
| 1 | Navigate to `/` | "Garage" heading visible, "+ Add Car" button present |
| 2 | With no vehicles | "No vehicles yet." message and "Add your first car" link shown |
| 3 | With vehicles | Vehicle cards shown in grid with name, year/make/model, est. mileage |
| 4 | Vehicle card badges | Overdue (red), upcoming (yellow), or "All good" (green) badge per card |
| 5 | Click a vehicle card | Navigates to `/vehicles/:id` and shows vehicle detail |
| 6 | Click "+ Add Car" | Navigates to `/vehicles/new` |

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
| 2 | Status bar | Shows estimated mileage and "as of" date (or empty if no data) |
| 3 | "Update Mileage" button | Toggles mileage entry form inline |
| 4 | "Log Service" button | Toggles service form inline |
| 5 | Schedule tab (default) | Active, shows reminder groups (overdue/upcoming/ok) |
| 6 | History tab | Click switches to service history list |
| 7 | Click "← Garage" | Returns to `/` |

## TP-05: Update Mileage

| # | Step | Expected |
|---|------|----------|
| 1 | Click "Update Mileage" | Form appears with odometer input and notes field |
| 2 | Enter 0 or negative, submit | "Odometer must be greater than 0" error |
| 3 | Enter valid mileage (e.g. 45000), submit | "Saving..." then form closes, estimated mileage in status bar updates |
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
| 1 | Click "Glovebox" logo | Returns to `/` |
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
| 5 | `GET /api/settings` | Returns settings list (seeded defaults) |

## TP-13: Shops

| # | Step | Expected |
|---|------|----------|
| 1 | `GET /api/shops` with empty DB | Returns `[]` |
| 2 | `POST /api/shops` with name | Shop created, returned with id |
| 3 | `GET /api/shops/:id` | Returns shop details |
| 4 | `PUT /api/shops/:id` | Updates shop fields |
| 5 | Service record with shop_id | Links service to a shop entity |

## TP-14: Observations

| # | Step | Expected |
|---|------|----------|
| 1 | Click "Obs." tab on vehicle detail | Observations tab visible with "Add Observation" button |
| 2 | No observations | "No observations yet." message |
| 3 | Add observation (fill title, category) | Form submits, observation appears in list |
| 4 | Observation shows category badge and date | Category in uppercase, date formatted |
| 5 | OBD code category shows code input | Extra field for JSON array of codes |
| 6 | "Mark Resolved" button | Toggles resolved state, card becomes dimmed |
| 7 | Observations appear in History tab | Interleaved with services, tagged "Observation" |

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

## TP-16: Accidents

| # | Step | Expected |
|---|------|----------|
| 1 | `POST /api/vehicles/:id/accidents` | Creates accident with description and fault |
| 2 | `GET /api/vehicles/:id/accidents` | Returns list with correspondence and service_record_ids |
| 3 | Add correspondence to accident | Correspondence entry created with contact method and summary |
| 4 | Update accident with cost/resolution | Fields updated, resolved flag toggled |
| 5 | Link service records to accident | service_record_ids populated in response |

## TP-17: Interleaved Timeline

| # | Step | Expected |
|---|------|----------|
| 1 | History tab with both services and observations | Both types shown, sorted by date descending |
| 2 | Service entries | Tagged "Service" (green badge), show cost and mileage |
| 3 | Observation entries | Tagged "Observation" (yellow badge), show resolved status |
| 4 | Filter: "All" | Shows both types |
| 5 | Filter: "Services" | Shows only service records |
| 6 | Filter: "Observations" | Shows only observations |

## TP-18: Parts Tab

| # | Step | Expected |
|---|------|----------|
| 1 | Click "Parts" tab on vehicle detail | Parts tab visible with "+ Add Slot" and "+ Add Part" buttons |
| 2 | No slots or parts | "No parts or slots yet." message |
| 3 | Add a slot (name, category, OE spec) | Slot appears grouped under category heading |
| 4 | Slot with no part installed | "No part installed" shown in slot card |
| 5 | Add a part to a slot (name, manufacturer, cost) | Part appears under slot, status badge "purchased" |
| 6 | Edit part status to "installed" | Status badge changes, install date/odometer shown |
| 7 | Expand part history on slot | Shows all parts for that slot with status, cost |
| 8 | Add unslotted part | Part appears under "Unslotted Parts" heading |
| 9 | Edit a slot | Updates slot name, category, OE spec |
| 10 | Delete a slot | Slot removed, parts become unslotted |
| 11 | Delete a part | Part removed from list |

## TP-19: Cost of Ownership

| # | Step | Expected |
|---|------|----------|
| 1 | Click "Costs" tab on vehicle detail | Cost of Ownership heading visible |
| 2 | No service records or parts | "No cost data yet." message |
| 3 | With services and parts | Summary cards: Total Spent, Services, Parts, Labor |
| 4 | Cost per mile | Shown when vehicle has purchase_mileage and service mileage |
| 5 | Monthly breakdown table | Rows with month, service cost, parts cost, total |

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

## TP-22: AI Status & Configuration

| # | Step | Expected |
|---|------|----------|
| 1 | `GET /api/ai/status` with default settings | Returns `{ provider: "none", configured: false }` |
| 2 | Set `ai.provider` to `claude` without API key | `configured: false` |
| 3 | Set `ai.provider` to `claude` with API key | `configured: true, provider: "claude"` |
| 4 | AI-dependent endpoints when not configured | Return 400 with "AI is not configured" message |

## TP-23: AI Chat

| # | Step | Expected |
|---|------|----------|
| 1 | Click "AI" tab on vehicle detail | Chat tab visible with message input |
| 2 | AI not configured | "AI is not configured" message shown, input disabled |
| 3 | Send a chat message (AI configured) | Message appears right-aligned, loading spinner shown, assistant response appears left-aligned |
| 4 | Chat history persists | Reload page, previous messages still shown |
| 5 | `GET /api/ai/chat/history?vehicle_id=N` | Returns messages ordered by created_at |
| 6 | `POST /api/ai/chat` without vehicle_id | Works for general (non-vehicle) chat |

## TP-24: Invoice PDF Parsing

| # | Step | Expected |
|---|------|----------|
| 1 | Upload a PDF document | "Parse with AI" button appears on PDF documents |
| 2 | Non-PDF documents | No "Parse with AI" button shown |
| 3 | Click "Parse with AI" (AI not configured) | Error message shown |
| 4 | Click "Parse with AI" (AI configured) | Loading state, then parsed results shown in review modal |
| 5 | Review modal shows extracted fields | Date, shop, mileage, line items, costs editable |
| 6 | Click "Create Service Record" | Service created with parsed/edited data, redirects to history |

## TP-25: Proactive Suggestions

| # | Step | Expected |
|---|------|----------|
| 1 | Schedule tab with AI configured | Suggestions card shown with AI-generated recommendations |
| 2 | AI not configured | Suggestions card not shown (or shows setup prompt) |
| 3 | Each suggestion shows | Title, reason, urgency badge (high/medium/low) |
| 4 | `GET /api/vehicles/:id/suggestions` | Returns JSON array of suggestions with title, reason, urgency |
| 5 | Suggestions cached | Second request within 24h returns same results without AI call |

## TP-26: NHTSA Recall Check

| # | Step | Expected |
|---|------|----------|
| 1 | Click "Research" tab on vehicle detail | Research tab visible with "Check Recalls" and "Run Full Check" buttons |
| 2 | No data initially | "No research reports yet." message |
| 3 | Click "Check Recalls" | Loading state, then recall results or "No open recalls found" message |
| 4 | Vehicle missing make/model/year | Error message about missing required fields |
| 5 | `GET /api/vehicles/:id/recalls` | Returns recall data with campaign numbers, subjects, remedies |

## TP-27: Research Reports

| # | Step | Expected |
|---|------|----------|
| 1 | Click "Run Full Check" | Loading state ("Generating..."), then report with findings shown |
| 2 | Report shows summary | Summary text describes number of findings |
| 3 | Each finding shows | Category badge, severity badge, status badge, title, description |
| 4 | Finding status actions | "Dismiss", "Plan", "Complete" buttons update status |
| 5 | Dismissed findings | Card becomes dimmed |
| 6 | Completed findings | Card border turns green, opacity reduced |
| 7 | Reports list | Previous reports shown with type, date, and summary preview |
| 8 | Click a report row | Expands to show findings for that report |
| 9 | `POST /api/vehicles/:id/research` | Creates report with findings from NHTSA + AI |
| 10 | `GET /api/vehicles/:id/research` | Returns list of reports ordered by generated_at desc |
| 11 | `GET /api/vehicles/:id/research/:id` | Returns report with findings array |
| 12 | `PUT /api/vehicles/:id/research/:rid/findings/:id` | Updates finding status and linked entity |

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
  observations.spec.ts  # TP-14
  documents.spec.ts     # TP-15
  parts.spec.ts         # TP-18, TP-19, TP-21
  chat.spec.ts          # TP-22, TP-23
  invoice-parse.spec.ts # TP-24
  suggestions.spec.ts   # TP-25
  research.spec.ts      # TP-26, TP-27
```

Run: `just test-e2e`

---

## Maintaining This Plan

When adding new features:
1. Add a new `TP-XX` section covering the feature's user flows
2. Write corresponding Playwright tests in `frontend/e2e/`
3. Update existing sections if behavior changes
