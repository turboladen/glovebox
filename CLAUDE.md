# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What Is This

Car maintenance tracker: Rust backend (Axum + SeaORM + SQLite) + Svelte 5 frontend SPA.
The Rust side is a three-crate cargo workspace: `glovebox-shared` (domain library) +
`glovebox-backend` (thin Axum binary) + `glovebox-mcp` (thin MCP surface, mounted at `/mcp`).

## Commands

```bash
# Development
just dev                  # Run backend + frontend (bun run dev) together
cargo run -p glovebox-backend  # Backend only (listens on :3003, serves SPA from frontend/dist/)
cd frontend && bun run dev # Frontend dev server only (Vite on :5373, proxies /api + /files to :3003)

# Build
cargo build --workspace   # Both Rust crates
cd frontend && bun run build  # Frontend (outputs to frontend/dist/)

# Test
cargo test --workspace    # Rust unit tests (bulk live in glovebox-shared service modules)
cargo test -p glovebox-shared test_name  # Single Rust test
just test-e2e             # Playwright e2e (needs `just dev` running)
just test-e2e-ci          # Self-contained e2e: boots backend+vite on a throwaway DB, runs Playwright, tears down
just test-e2e-ui          # Playwright e2e with visible browser
cd frontend && bunx playwright test e2e/garage.spec.ts  # Single e2e test file

# Lint / Check
just ci                   # Everything CI runs except e2e (nightly fmt-check, layering, backend, frontend)
just check-layering       # Layering gate only (scripts/check-layering.sh)
cargo clippy --workspace -- -D clippy::pedantic  # Rust lints (pedantic is the CI gate)
cd frontend && bun run check  # svelte-check + TypeScript check
```

## Architecture

### Workspace layout

- **`glovebox-shared/`** (lib crate `glovebox_shared`) — the HTTP-agnostic domain:
  `src/{entities,migration,services,inputs,config.rs,error.rs,test_support.rs}`. All SQL,
  validation, and business logic live here. Never depends on axum.
- **`glovebox-backend/`** (bin crate) — the thin Axum surface: `src/{api,main.rs}`. Handlers
  map HTTP DTOs to domain inputs, call shared services, and map errors back. Mounts the MCP
  router at `/mcp` (single deployable binary; two surfaces, one library).
- **`glovebox-mcp/`** (lib crate `glovebox_mcp`) — the LLM-facing MCP surface over the same
  domain library: semantic domain-verb tools (`record_service`, `check_due_maintenance`, …)
  and read-only `glovebox://` resources. As thin as the HTTP handlers: every tool body is
  arg-struct → shared-service call → serialize, with one `DomainError → MCP` mapping helper.
  May import axum (it exposes `router(db) -> axum::Router`); only `glovebox-shared` is
  axum-free. Unauthenticated by design (LAN posture) — see the crate docs in `src/lib.rs`
  before exposing `/mcp` beyond the LAN.
- `scripts/check-layering.sh` (run by CI and `just ci`) enforces the shared/backend boundary.

### Backend binary (`glovebox-backend/src/`)

**AppState** (`main.rs`): Axum shared state holds `DatabaseConnection`, `Arc<AppConfig>`, and `Arc<AiProviderRegistry>`. All handlers extract `State(state)`.

**Routing split**: Top-level CRUD resources (`vehicles`, `platforms`, `model_templates`, `schedules`, `shops`) use `.nest()` with their own `Router`. Vehicle sub-resources (`mileage`, `services`, `observations`, `accidents`, `parts`, `documents`, `research`, etc.) use flat `.route()` calls directly in `main.rs` so `Path((vehicle_id, id))` tuple extraction works correctly.

**API handlers** (`api/`): Each module defines `Create*`/`Update*` DTOs (HTTP serde only), uses `Result<T> = std::result::Result<T, ApiError>`, and returns `Json<>`. Handlers are thin: map DTO → `glovebox_shared::inputs::*`, call `glovebox_shared::services::*`, return the result. Errors go through `DomainError → ApiError` (`api/error.rs`) → `IntoResponse`.

### Domain library (`glovebox-shared/src/`)

**Entities** (`entities/`): Hand-written `DeriveEntityModel` structs (not generated). Parent entities declare `has_many` relations; junction tables have `via()` impls. 22 entity files.

**Inputs** (`inputs/`): Plain domain input structs (`New*`/`Update*`) consumed by service functions. No HTTP serde.

**Services** (`services/`): One module per domain (`vehicle`, `service_record`, `platform`, ...) of free functions taking `db: &impl ConnectionTrait` first, returning `DomainResult<T>`; plus the pre-existing `ai/` (pluggable provider trait), `reminders`, `vin_decode`, `nhtsa`.

**Errors** (`error.rs`): `DomainError::{NotFound, Invalid{field,message}, BadRequest, Db, Internal}` with `DomainResult<T>` alias.

**AI layer** (`services/ai/`): `AiProvider` trait with implementations for Claude API, OpenAI-compatible (Ollama/LM Studio), mock (testing), and noop. Provider is selected at startup from `ai_providers` table. Context builder in `context.rs` assembles vehicle data into system prompts.

**Migrations** (`migration/`): 12 migration files, auto-run on startup via `Migrator::up()`.

**Test harness** (`test_support.rs`): `test_db()` — in-memory SQLite with migrations applied, for service-layer unit tests. Compiled under `#[cfg(any(test, feature = "test-support"))]`; sibling crates use it in their integration tests via a dev-dependency on `glovebox-shared` with the `test-support` feature (see `glovebox-mcp/Cargo.toml`).

### MCP surface (`glovebox-mcp/src/`)

**Mount** (`lib.rs`): `router(db) -> axum::Router` wraps rmcp's `StreamableHttpService` (+ `LocalSessionManager`, 7-day session keep-alive); the backend nests it at `/mcp`. LAN hostnames must be allowlisted via `GLOVEBOX_MCP_ALLOWED_HOSTS` (rmcp's DNS-rebinding defense 403s unknown `Host` headers).

**Tools** (`handler.rs`): 14 domain verbs, named as things a person would say (`record_service`, not `create_service_record`). Inputs are schemars-derived structs in `schemas.rs` (doc comments become the LLM-visible field descriptions). `LenientParameters<T>` defers deserialize errors so malformed args come back as actionable tool errors, not bare JSON-RPC `-32602`s — pair it with an explicit `input_schema = schema_for_type::<T>()` on every `#[tool]`.

**Resources**: stable URIs (`glovebox://vehicles`, `…/{id}`, `…/{id}/activity`, `…/{id}/builds/{build_id}`). `list_resources` enumerates concrete URIs from the DB; `read_resource` parses them (rmcp resource templates are not used).

**Error mapping**: one helper (`domain_error`) used by every tool — `NotFound`/`Invalid`/`BadRequest` become tool-level error results (message reaches the LLM); `Db`/`Internal` become opaque JSON-RPC internal errors with detail kept in tracing.

**Tests**: `tests/mcp_integration_test.rs` drives the real router over the Streamable HTTP handshake (initialize → session id → tools/resources).

### Frontend (`frontend/`)

Svelte 5 SPA using `@keenmate/svelte-spa-router`. Three routes: Garage (list), VehicleNew, VehicleDetail. VehicleDetail uses tab-based navigation (History, Schedule, Parts, Costs, Documents, Observations, Chat, Research, Suggestions).

Vite dev server proxies `/api` and `/files` to the backend at `:3003`. In production, the backend serves `frontend/dist/` as SPA fallback.

## Conventions

### Layering (enforced by `scripts/check-layering.sh` in CI)

- Domain inputs + validation + SQL live in `glovebox-shared::{inputs,services}`. Backend handlers ONLY map HTTP DTO ↔ domain input and rely on `DomainError → ApiError` conversion — no SeaORM queries, `ActiveModel`s, or business logic in `glovebox-backend/src/api/`
- `glovebox-shared` never imports axum (domain stays HTTP-agnostic)
- Service fns take `db: &impl ConnectionTrait` as the first param (+ `&AppConfig` / `&AiProviderRegistry` as explicit params where needed) — never `AppState`
- **Error model**: services return `DomainResult<T>`; `DomainError::{NotFound, Invalid{field,message}, BadRequest, Db, Internal}`. `Invalid` renders as `"{field}: {message}"` (400); `BadRequest` renders its message verbatim (400); `Db`/`Internal` become 500
- **updated_at stamping lives in shared service update fns**, not handlers: every update fn sets `active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string())` — SeaORM `ActiveModel::update()` does NOT auto-set it
- New domain logic gets service-level unit tests in `glovebox-shared` (via `test_support::test_db()`) plus e2e coverage

### General

- **Issue tracking**: Use `bd` (beads), never markdown TODOs
- **Testing**: Update `TEST_PLAN.md` and add Playwright tests when changing UI
- **Router**: `@keenmate/svelte-spa-router` uses `routeParams` (not `params`) in Svelte 5
- **Currency**: Stored as cents (`i32`), displayed as dollars in frontend. Use integer division for formatting (not `as f64 / 100.0` which loses precision)
- **DB datetimes**: `String` type at SeaORM boundary (SQLite TEXT)
- **Migrations**: Use `Expr::cust()` for SQL expressions, not string literals
- **Entity field order**: Must match physical DB column order. ALTER TABLE appends columns to the end, so new fields go after `created_at`/`updated_at` in the entity struct
- **Axum routing**: Vehicle sub-resources use flat routes in `main.rs` (not nested), so `Path((vehicle_id, id))` tuple extraction works correctly
- **Update DTOs**: Use `Option<Option<T>>` (double-option) to distinguish "not sent" vs "explicitly set to null"
- **Vehicle ownership checks**: All vehicle sub-resource handlers must call `glovebox_shared::services::vehicle::require(&state.db, vehicle_id).await?` before delegating to the sub-resource service (moving the check into service fns is tracked as `glovebox-paxy`)
- **N+1 queries**: List endpoints that load related data must use batch loading with `is_in()` queries, not per-record queries in a loop
- **Svelte 5 bind:this**: Elements used with `bind:this` must be declared with `$state(undefined)`, not bare `let`
- **Imports**: Use `sea_orm::*` glob imports (idiomatic for SeaORM). Explicit imports create maintenance burden with unused-import warnings

## Clippy

Crate-level `#![allow]`s live in BOTH crate roots (`glovebox-backend/src/main.rs` and `glovebox-shared/src/lib.rs`) and suppress intentional pedantic lints:
- `clippy::option_option` — update DTO convention (see above)
- `clippy::struct_field_names` — entity fields map to DB column names
- `clippy::wildcard_imports` — `sea_orm::*` is idiomatic

`glovebox-shared/src/lib.rs` additionally allows `clippy::missing_errors_doc`, `clippy::missing_panics_doc`, `clippy::must_use_candidate`, and `clippy::implicit_hasher` — these lints target public-API surface and only fired once the domain became a library crate; rationale is in a comment in `lib.rs`.

Run `cargo clippy --workspace -- -D clippy::pedantic` to verify zero warnings. Add per-function `#[allow]` for unavoidable cases (e.g., `too_many_lines` on update handlers with many fields).

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
