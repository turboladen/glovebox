# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What Is This

Car maintenance tracker: Rust backend (Axum + SeaORM + SQLite) + Svelte 5 frontend SPA.

## Commands

```bash
# Development
just dev                  # Run backend (cargo run) + frontend (bun run dev) together
cargo run                 # Backend only (listens on :3000, serves SPA from frontend/dist/)
cd frontend && bun run dev # Frontend dev server only (Vite on :5173, proxies /api + /files to :3000)

# Build
cargo build               # Backend
cd frontend && bun run build  # Frontend (outputs to frontend/dist/)

# Test
cargo test                # Rust unit tests
cargo test -- test_name   # Single Rust test
just test-e2e             # Playwright e2e (needs `just dev` running)
just test-e2e-ui          # Playwright e2e with visible browser
cd frontend && bunx playwright test tests/garage.spec.ts  # Single e2e test file

# Lint / Check
cargo clippy              # Rust lints
cd frontend && bun run check  # svelte-check + TypeScript check
```

## Architecture

### Backend (`src/`)

**AppState** (`main.rs`): Axum shared state holds `DatabaseConnection`, `Arc<AppConfig>`, and `Arc<dyn AiProvider>`. All handlers extract `State(state)`.

**Routing split**: Top-level CRUD resources (`vehicles`, `platforms`, `model_templates`, `schedules`, `settings`, `shops`) use `.nest()` with their own `Router`. Vehicle sub-resources (`mileage`, `services`, `observations`, `accidents`, `parts`, `documents`, `research`, etc.) use flat `.route()` calls directly in `main.rs` so `Path((vehicle_id, id))` tuple extraction works correctly.

**API handlers** (`src/api/`): Each module defines `Create*`/`Update*` DTOs, uses `Result<T> = std::result::Result<T, ApiError>`, and returns `Json<>`. Errors go through `ApiError` enum → `IntoResponse`.

**Entities** (`src/entities/`): Hand-written `DeriveEntityModel` structs (not generated). Parent entities declare `has_many` relations; junction tables have `via()` impls. 19 entity files.

**Services** (`src/services/`): Business logic — `ai/` (pluggable provider trait), `reminders`, `vin_decode`, `nhtsa`.

**AI layer** (`src/services/ai/`): `AiProvider` trait with implementations for Claude API, OpenAI-compatible (Ollama/LM Studio), mock (testing), and noop. Provider is selected at startup from `settings` table (`ai.*` keys). Context builder in `context.rs` assembles vehicle data into system prompts.

**Migrations** (`src/migration/`): 18 migration files, auto-run on startup via `Migrator::up()`.

### Frontend (`frontend/`)

Svelte 5 SPA using `@keenmate/svelte-spa-router`. Three routes: Garage (list), VehicleNew, VehicleDetail. VehicleDetail uses tab-based navigation (History, Schedule, Parts, Costs, Documents, Observations, Chat, Research, Suggestions).

Vite dev server proxies `/api` and `/files` to the backend at `:3000`. In production, the backend serves `frontend/dist/` as SPA fallback.

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
- Use `{ exact: true }` on `getByText` when the text is a substring of another element
- Use `getByRole('textbox', { name })` instead of `getByLabel` when labels are ambiguous (e.g., "Model" vs "Model Template")
- Tests sharing a vehicle via `beforeAll` must not depend on prior test state — each test should set up what it needs
