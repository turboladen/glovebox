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
cd frontend && bunx playwright test e2e/dashboard.spec.ts  # Single e2e test file

# Lint / Check
just ci                   # Everything CI runs except e2e (nightly fmt-check, layering, backend, frontend)
just fmt                  # cargo +nightly fmt — rustfmt.toml uses nightly-only options; plain `cargo fmt` passes locally but CI's format job will fail
just check-layering       # Layering gate only (scripts/check-layering.sh)

# Merging PRs: gate on parsed `gh pr checks` output showing zero pending/failed —
# `--watch` exits 0 when it races a run that hasn't registered yet
cargo clippy --workspace -- -D clippy::pedantic  # Rust lints (pedantic is the CI gate)
cd frontend && bun run check  # svelte-check + TypeScript check
```

## Environment

All optional (defaults in `glovebox-shared/src/config.rs`):
- `GLOVEBOX_DB_PATH` (default `data/glovebox.db`) · `GLOVEBOX_LISTEN` (default `0.0.0.0:3003`) · `GLOVEBOX_FILES_DIR` (default `data/files`)
- `GLOVEBOX_MCP_ALLOWED_HOSTS` — extra Host-header allowlist entries for `/mcp` (defaults: localhost/127.0.0.1/::1)

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

**AppState** (`main.rs`): Axum shared state holds `DatabaseConnection` and `Arc<AppConfig>`. All handlers extract `State(state)`.

**Routing split**: Top-level CRUD resources (`vehicles`, `platforms`, `model_templates`, `schedules`, `shops`) use `.nest()` with their own `Router`. Vehicle sub-resources (`mileage`, `services`, `incidents`, `parts`, `documents`, `research`, etc.) use flat `.route()` calls directly in `main.rs` so `Path((vehicle_id, id))` tuple extraction works correctly.

**API handlers** (`api/`): Each module defines `Create*`/`Update*` DTOs (HTTP serde only), uses `Result<T> = std::result::Result<T, ApiError>`, and returns `Json<>`. Handlers are thin: map DTO → `glovebox_shared::inputs::*`, call `glovebox_shared::services::*`, return the result. Errors go through `DomainError → ApiError` (`api/error.rs`) → `IntoResponse`.

### Domain library (`glovebox-shared/src/`)

**Entities** (`entities/`): Hand-written `DeriveEntityModel` structs (not generated). Parent entities declare `has_many` relations; junction tables have `via()` impls. 20 entity files.

**Inputs** (`inputs/`): Plain domain input structs (`New*`/`Update*`) consumed by service functions. No HTTP serde.

**Services** (`services/`): One module per domain (`vehicle`, `service_record`, `platform`, ...) of free functions taking `db: &impl ConnectionTrait` first, returning `DomainResult<T>`; plus `reminders`, `budget`, `vin_decode`, `nhtsa`, `activity` (merged per-vehicle + garage-wide feeds), and `dashboard` (the garage-wide aggregation behind `GET /api/dashboard` — composes `calculate_reminders`/`forecast_from`/list fns per vehicle; the per-vehicle loop is deliberate, few cars). Planning (`work_item`, `visit`) backs the Plan tab and the MCP verbs; `visit::complete` is the transactional loop-closer (service record + reminder clears + recall/incident resolution in one unit — nested SeaORM `begin()` on a transaction is a SQLite SAVEPOINT, which it relies on); `visit::cancel`/`delete` are guarded to open visits. (The in-app AI layer was retired in 2hea unit A — Claude connects over `/mcp` instead.)

**Errors** (`error.rs`): `DomainError::{NotFound, Invalid{field,message}, BadRequest, Db, Internal}` with `DomainResult<T>` alias.

**Migrations** (`migration/`): 20 migration files, auto-run on startup via `Migrator::up()`.

**Test harness** (`test_support.rs`): `test_db()` — in-memory SQLite with migrations applied, for service-layer unit tests. Compiled under `#[cfg(any(test, feature = "test-support"))]`; sibling crates use it in their integration tests via a dev-dependency on `glovebox-shared` with the `test-support` feature (see `glovebox-mcp/Cargo.toml`).

### MCP surface (`glovebox-mcp/src/`)

**Connect a client:** Streamable HTTP at `http://<host>:3003/mcp` (e.g. Claude Desktop/Code custom
connector; non-localhost hosts need `GLOVEBOX_MCP_ALLOWED_HOSTS`).

**Mount** (`lib.rs`): `router(db) -> axum::Router` wraps rmcp's `StreamableHttpService` (+ `LocalSessionManager`, 7-day session keep-alive); the backend nests it at `/mcp`. LAN hostnames must be allowlisted via `GLOVEBOX_MCP_ALLOWED_HOSTS` (rmcp's DNS-rebinding defense 403s unknown `Host` headers).

**Tools** (`handler.rs`): 23 domain verbs, named as things a person would say (`record_service`, not `create_service_record`). Inputs are schemars-derived structs in `schemas.rs` (doc comments become the LLM-visible field descriptions). `LenientParameters<T>` defers deserialize errors so malformed args come back as actionable tool errors, not bare JSON-RPC `-32602`s — pair it with an explicit `input_schema = schema_for_type::<T>()` on every `#[tool]`.

**Resources**: stable URIs (`glovebox://vehicles`, `…/{id}`, `…/{id}/activity`, `…/{id}/builds/{build_id}`). `list_resources` enumerates concrete URIs from the DB; `read_resource` parses them (rmcp resource templates are not used).

**Error mapping**: one helper (`domain_error`) used by every tool — `NotFound`/`Invalid`/`BadRequest` become tool-level error results (message reaches the LLM); `Db`/`Internal` become opaque JSON-RPC internal errors with detail kept in tracing.

**Tests**: `tests/mcp_integration_test.rs` drives the real router over the Streamable HTTP handshake (initialize → session id → tools/resources).

### Frontend (`frontend/`)

Svelte 5 SPA using `@keenmate/svelte-spa-router`. Shell (2hea decision ⑥): `App.svelte` renders a
header (logo · global search over `GET /api/search` · sidebar toggle) above a collapsible
`Sidebar` (per-car status hints from the shared `/api/dashboard` store in `lib/stores.ts`;
collapsed state in localStorage) + main area. Routes: `/` → `Dashboard` (garage-wide landing;
welcome state when empty), `/shops`, `/vehicles/new`, `/vehicles/:id[/:tab[/:sub]]` →
`VehicleDetail`.

`VehicleDetail` tabs are **URL-driven** and intent-shaped: **Overview** (the same `Dashboard`
component scoped via its `vehicleId` prop) · **Timeline** (`TimelineTab` — merged
services/incidents/mileage stream with kind filters; subsumed the old History/Incidents tabs;
incident detail/form live in `IncidentDetail`/`IncidentForm`; the header's "Record service"
routes here with `?action=record` so there is ONE service form) · **Plan** (`PlanTab` — sub-nav
Due (`ScheduleTab`) / To-do / Visits / Research (`ResearchTab`) / Schedule ⚙ (`ScheduleConfig`))
· **Builds** (`BuildsTab`) · **Records** (`RecordsTab` — sub-nav `PartsTab`/`DocumentsTab`;
legacy `records/research` URLs redirect to `plan/research`) · **Costs** (`CostsTab` + the
12-month forecast buckets). Deep links (dashboard rows, search hits, source badges) always
target a tab/sub URL, usually with an `?hl=` highlight param.

Vite dev server proxies `/api` and `/files` to the backend at `:3003`. In production, the backend serves `frontend/dist/` as SPA fallback.

## Conventions

### Layering (enforced by `scripts/check-layering.sh` in CI)

- Domain inputs + validation + SQL live in `glovebox-shared::{inputs,services}`. Backend handlers ONLY map HTTP DTO ↔ domain input and rely on `DomainError → ApiError` conversion — no SeaORM queries, `ActiveModel`s, or business logic in `glovebox-backend/src/api/`
- `glovebox-shared` never imports axum (domain stays HTTP-agnostic)
- Service fns take `db: &impl ConnectionTrait` as the first param (+ `&AppConfig` as an explicit param where needed) — never `AppState`
- **Error model**: services return `DomainResult<T>`; `DomainError::{NotFound, Invalid{field,message}, BadRequest, Db, Internal}`. `Invalid` renders as `"{field}: {message}"` (400); `BadRequest` renders its message verbatim (400); `Db`/`Internal` become 500
- **updated_at stamping lives in shared service update fns**, not handlers: every update fn sets `active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string())` — SeaORM `ActiveModel::update()` does NOT auto-set it
- New domain logic gets service-level unit tests in `glovebox-shared` (via `test_support::test_db()`) plus e2e coverage

### General

- **Hypermedia affordances** (UX): state displays LINK to their source — a "planned" chip links to the work item, the sidebar's "N due" badge links to Plan/Due, never dead-end text. Deep links carry `?hl=<kind>:<id>`; target views render `id="<kind>-<id>"` anchors (underscores → dashes) and call `flashHighlightFromQuery(kind)` from `frontend/src/lib/highlight.ts` once rows are in the DOM (scroll + 2s flash, `prefers-reduced-motion` respected). ONE verb per action app-wide: "Record service" (never "Log Service"; "Log incident" is a different action)
- **Issue tracking**: Use `bd` (beads), never markdown TODOs
- **Testing**: Update `TEST_PLAN.md` and add Playwright tests when changing UI
- **Router**: `@keenmate/svelte-spa-router` uses `routeParams` (not `params`) in Svelte 5
- **Currency**: Stored as cents (`i32`), displayed as dollars in frontend. Use integer division for formatting (not `as f64 / 100.0` which loses precision)
- **DB datetimes**: `String` type at SeaORM boundary (SQLite TEXT)
- **Migrations**: `Expr::cust()` for expressions inside builder queries; raw `execute_unprepared` only where no builder form exists (FTS5 DDL, cross-table copies)
- **Migrations are NOT transactional on SQLite** (sea-orm-migration only wraps Postgres) — write them rerun-safe: `IF NOT EXISTS` DDL, `INSERT OR IGNORE`/`NOT EXISTS`-gated DML, pragma-guarded ALTERs (000019 is the pattern)
- **SQLite can't `DROP COLUMN` named in an FK clause** — use the table-rebuild pattern (000016). FTS5: copy 000013's canonical external-content trigger trio; aggregating over `bm25()` arms needs a `MATERIALIZED` CTE (query flattening breaks bm25)
- **Verify data-moving migrations against a populated copy of `data/glovebox.db`** (boot the branch binary against the copy), not just `test_db()`
- **Entity field order**: Must match physical DB column order. ALTER TABLE appends columns to the end, so new fields go after `created_at`/`updated_at` in the entity struct
- **Axum routing**: Vehicle sub-resources use flat routes in `main.rs` (not nested), so `Path((vehicle_id, id))` tuple extraction works correctly
- **Update DTOs**: Use `Option<Option<T>>` (double-option) to distinguish "not sent" vs "explicitly set to null"
- **Vehicle ownership checks**: All vehicle sub-resource handlers must call `glovebox_shared::services::vehicle::require(&state.db, vehicle_id).await?` before delegating to the sub-resource service (moving the check into service fns is tracked as `glovebox-paxy`)
- **Cross-reference guards**: services self-guard EVERY foreign id they accept (build_id, service ids, schedule items, recurrence links, …) — MCP calls services directly with no handler pre-checks. Wrong-parent errors must be byte-identical `NotFound` to nonexistent (no ownership oracles), with a wrong-parent regression test per link
- **Svelte edit forms clearing a field must send explicit `null`** — the `|| undefined` idiom omits the key, which the double-option backend reads as "not sent", silently keeping the stale value
- **Hypermedia affordances**: any UI element displaying state (badges, counts, status chips like "6 DUE" or "PLANNED") must LINK to where that state lives, and deep-link targets must highlight the linked record (scroll + flash). No dead-end facts. One verb per action app-wide (e.g. "Record service", never a "Log Service" twin)
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
