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
