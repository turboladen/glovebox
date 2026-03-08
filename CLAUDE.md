# Glovebox

Car maintenance tracker: Rust backend (Axum + SeaORM + SQLite) + Svelte 5 frontend.

## Quick Start

```bash
just dev         # Run backend + frontend together
just test-e2e    # Run Playwright e2e tests (needs `just dev` running)
```

## Project Structure

- `src/` — Rust backend (Axum routes, SeaORM entities, migrations, services)
- `frontend/` — Svelte 5 SPA (Vite, `@keenmate/svelte-spa-router`)
- `frontend/e2e/` — Playwright e2e tests
- `TEST_PLAN.md` — Living test plan (manual + e2e), keep updated with features

## Conventions

- **Issue tracking**: Use `bd` (beads), never markdown TODOs
- **Testing**: Update `TEST_PLAN.md` and add Playwright tests when changing UI
- **Router**: `@keenmate/svelte-spa-router` uses `routeParams` (not `params`) in Svelte 5
- **Currency**: Stored as cents (`i32`), displayed as dollars in frontend
- **DB datetimes**: `String` type at SeaORM boundary (SQLite TEXT)
- **Migrations**: Use `Expr::cust()` for SQL expressions, not string literals
- **Entity field order**: Must match physical DB column order. ALTER TABLE appends columns to the end, so new fields go after `created_at`/`updated_at` in the entity struct
- **Axum routing**: Vehicle sub-resources use flat routes in `main.rs` (not nested), so `Path((vehicle_id, id))` tuple extraction works correctly
- **Update DTOs**: Use `Option<Option<T>>` (double-option) to distinguish "not sent" vs "explicitly set to null"

## Playwright Patterns

- Always wait for async loading before asserting (e.g., `await expect(locator).toBeVisible()` not `count()` immediately)
- Use `{ exact: true }` on `getByText` when the text is a substring of another element (e.g., "receipt" inside "Test Receipt")
- Use `getByRole('textbox', { name })` instead of `getByLabel` when labels are ambiguous (e.g., "Model" vs "Model Template")
- Tests sharing a vehicle via `beforeAll` must not depend on prior test state — each test should set up what it needs
