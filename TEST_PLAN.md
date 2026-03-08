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
```

Run: `just test-e2e`

---

## Maintaining This Plan

When adding new features:
1. Add a new `TP-XX` section covering the feature's user flows
2. Write corresponding Playwright tests in `frontend/e2e/`
3. Update existing sections if behavior changes
