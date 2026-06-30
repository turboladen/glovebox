# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What Is This

Car maintenance tracker: Rust backend (Axum + SeaORM + SQLite) + Svelte 5 frontend SPA.

## Commands

```bash
# Development
just dev                  # Run backend (cargo run) + frontend (bun run dev) together
cargo run                 # Backend only (listens on :3003, serves SPA from frontend/dist/)
cd frontend && bun run dev # Frontend dev server only (Vite on :5373, proxies /api + /files to :3003)

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

**Routing split**: Top-level CRUD resources (`vehicles`, `platforms`, `model_templates`, `schedules`, `shops`) use `.nest()` with their own `Router`. Vehicle sub-resources (`mileage`, `services`, `observations`, `accidents`, `parts`, `documents`, `research`, etc.) use flat `.route()` calls directly in `main.rs` so `Path((vehicle_id, id))` tuple extraction works correctly.

**API handlers** (`src/api/`): Each module defines `Create*`/`Update*` DTOs, uses `Result<T> = std::result::Result<T, ApiError>`, and returns `Json<>`. Errors go through `ApiError` enum → `IntoResponse`.

**Entities** (`src/entities/`): Hand-written `DeriveEntityModel` structs (not generated). Parent entities declare `has_many` relations; junction tables have `via()` impls. 20 entity files.

**Services** (`src/services/`): Business logic — `ai/` (pluggable provider trait), `reminders`, `vin_decode`, `nhtsa`.

**AI layer** (`src/services/ai/`): `AiProvider` trait with implementations for Claude API, OpenAI-compatible (Ollama/LM Studio), mock (testing), and noop. Provider is selected at startup from `ai_providers` table. Context builder in `context.rs` assembles vehicle data into system prompts.

**Migrations** (`src/migration/`): 8 consolidated migration files, auto-run on startup via `Migrator::up()`.

### Frontend (`frontend/`)

Svelte 5 SPA using `@keenmate/svelte-spa-router`. Three routes: Garage (list), VehicleNew, VehicleDetail. VehicleDetail uses tab-based navigation (History, Schedule, Parts, Costs, Documents, Observations, Chat, Research, Suggestions).

Vite dev server proxies `/api` and `/files` to the backend at `:3003`. In production, the backend serves `frontend/dist/` as SPA fallback.

## Conventions

- **Issue tracking**: Use `bd` (beads), never markdown TODOs
- **Testing**: Update `TEST_PLAN.md` and add Playwright tests when changing UI
- **Router**: `@keenmate/svelte-spa-router` uses `routeParams` (not `params`) in Svelte 5
- **Currency**: Stored as cents (`i32`), displayed as dollars in frontend. Use integer division for formatting (not `as f64 / 100.0` which loses precision)
- **DB datetimes**: `String` type at SeaORM boundary (SQLite TEXT)
- **Migrations**: Use `Expr::cust()` for SQL expressions, not string literals
- **Entity field order**: Must match physical DB column order. ALTER TABLE appends columns to the end, so new fields go after `created_at`/`updated_at` in the entity struct
- **Axum routing**: Vehicle sub-resources use flat routes in `main.rs` (not nested), so `Path((vehicle_id, id))` tuple extraction works correctly
- **Update DTOs**: Use `Option<Option<T>>` (double-option) to distinguish "not sent" vs "explicitly set to null"
- **Vehicle ownership checks**: All vehicle sub-resource handlers must call `require_vehicle(&state.db, vehicle_id).await?` before accessing sub-resources
- **updated_at**: All update handlers must explicitly set `active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string())` — SeaORM `ActiveModel::update()` does NOT auto-set it
- **N+1 queries**: List endpoints that load related data must use batch loading with `is_in()` queries, not per-record queries in a loop
- **Svelte 5 bind:this**: Elements used with `bind:this` must be declared with `$state(undefined)`, not bare `let`
- **Imports**: Use `sea_orm::*` glob imports (idiomatic for SeaORM). Explicit imports create maintenance burden with unused-import warnings

## Clippy

Crate-level `#![allow]` in `main.rs` suppresses intentional pedantic lints:
- `clippy::option_option` — update DTO convention (see above)
- `clippy::struct_field_names` — entity fields map to DB column names
- `clippy::wildcard_imports` — `sea_orm::*` is idiomatic

Run `cargo clippy -- -D clippy::pedantic` to verify zero warnings. Add per-function `#[allow]` for unavoidable cases (e.g., `too_many_lines` on update handlers with many fields).

## Playwright Patterns

- Always wait for async loading before asserting (e.g., `await expect(locator).toBeVisible()` not `count()` immediately)
- Use `{ exact: true }` on `getByText` when the text is a substring of another element
- Use `getByRole('textbox', { name })` instead of `getByLabel` when labels are ambiguous (e.g., "Model" vs "Model Template")
- Tests sharing a vehicle via `beforeAll` must not depend on prior test state — each test should set up what it needs


<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:ca08a54f -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd dolt push
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
<!-- END BEADS INTEGRATION -->
