# puby — Workspace split: glovebox-shared lib + glovebox-backend bin

**Date:** 2026-07-01
**Issue:** `glovebox-puby` (Stage 1 / "the gate" of epic `glovebox-mde0`)
**Blocks:** `glovebox-mq9r` (search), `glovebox-mmiv` (glovebox-mcp), `glovebox-zb0w` (build primitive)
**Sequencing context:** `2026-06-30-mde0-phase1-retrofit-sequencing.md`
**Altitude:** implementation design for Stage 1.

---

## Goal

Split the single `glovebox` binary crate into a two-crate cargo workspace matching the
personal-domain-pattern shape:

```
glovebox-shared   (lib)  — the domain: entities, migrations, services, validation, business logic
glovebox-backend  (bin)  — thin Axum HTTP/JSON over the lib; no SQL, no business logic in handlers
```

This is behaviorally a **no-op** — it re-homes code and moves logic down a layer, it does not change
what the app does. It is the prerequisite for a second surface (`glovebox-mcp`) that calls the same
lib without re-implementing validation or mapping.

## Current state (2026-07-01)

- Single `glovebox` binary crate, edition 2024, ~10.8k LOC.
- `src/{api,entities,migration,services,config.rs,main.rs}`.
- **The handlers are the data-access layer:** ~268 SeaORM ops (`.filter/.one/.all/.insert/.update/
  .delete/.exec`) are spread across 21 files in `src/api/`. `src/api/` is 4,950 LOC.
- Real service functions today exist only in `services/reminders.rs`, `services/vin_decode.rs`,
  `services/nhtsa.rs`, and the `services/ai/` provider trait. Everything else lives in handlers.
- No `rmcp` dependency, no `*-mcp` binary, no FTS5 (those are later stages).
- Test coverage: **51 Rust unit tests** concentrated in `services/ai/`, `api/research.rs`,
  `api/ai.rs`, `services/nhtsa.rs`. CRUD handlers (vehicles, services, parts, accidents, schedules,
  …) have essentially **no** unit coverage. **53 Playwright e2e cases** across 11 specs
  (`frontend/e2e/`) are the primary behavioral guard for the CRUD paths. `TEST_PLAN.md` exists.

## Decisions

Full record of the scoping decisions for this issue:

| ID | Decision | Rationale |
| -- | -------- | --------- |
| **D-depth** | **Full thinning now.** All ~268 DB ops move into `glovebox-shared` service fns; every handler becomes `extract → call shared → return`. | Anything less leaves SQL in the backend and forces `glovebox-mcp` to re-implement logic. Full thinning is the whole point of the gate. |
| **D-inputs** | **Domain inputs live in shared; HTTP serde stays in backend.** `glovebox-shared` defines plain domain input structs (service-fn params); `glovebox-backend` keeps request DTOs with HTTP serde (`deserialize_optional`, double-option) that map into the domain inputs. | Lets `glovebox-mcp` build domain inputs directly, with no HTTP-serde baggage, and reuse the same validation. |
| **D-validation** | **Validation + ActiveModel construction + transactions move DOWN into shared service fns** — not left in handlers. | This is the deliberate improvement over the sibling repos (see below). A shared service fn is "domain-complete": hand it domain data, get back a domain result, no HTTP knowledge required. |
| **D-shape** | **Free functions per domain module:** `glovebox_shared::services::service_record::create(db, NewServiceRecord) -> Result<View, DomainError>`. | Matches glovebox's existing `services/reminders.rs` style; simplest; no empty unit structs used purely for namespacing. |
| **D-error** | **Shared owns `DomainError` (thiserror).** Backend owns `ApiError`/`IntoResponse` and a `From<DomainError> for ApiError` mapping. Shared never imports `axum`. | Keeps the domain surface-agnostic; HTTP status mapping is a backend concern. |
| **D-crates** | **Two-crate workspace.** `entities/` and `migration/` fold into `glovebox-shared` as modules (not separate crates). | The pattern/spec puts entities+migration+services in one lib; glovebox has no standalone migration CLI (migrations auto-run via `Migrator::up()`); a 2-crate workspace is less ceremony. Splitting them out later is cheap if compile times ever demand it. |

### Why not follow the sibling repos verbatim

`../chorez` and `../kammerz` are at an earlier point on the same evolution: they extract `entity` and
`migration` as member crates, but the Axum backend + services + business logic still live in the root
crate, and there is no `-shared`/`-mcp` split. kammerz (the more evolved) has a per-domain service
layer, but its "service" is a **thin DB-CRUD wrapper that takes an `ActiveModel`**
(`CameraService::create(db, camera::ActiveModel) → model.insert(db)`, returning `DbErr`); the
validation, DTO→ActiveModel mapping, and transaction shaping still live in the **handler**. chorez is
earlier still (several routes hit the DB directly with no service).

That split is fine for a single surface but wrong for a **two-surface** world: an MCP tool calling
kammerz-style `create(db, active_model)` would have to re-implement all the handler's validation and
mapping. glovebox is the first repo doing the three-way split, so it pioneers the sharper paradigm —
**validation + mapping below the surface, in shared** — which chorez/kammerz can converge on later.

## Architecture

### Workspace layout

```
glovebox/
  Cargo.toml                 # [workspace] members = ["glovebox-shared", "glovebox-backend"]
  glovebox-shared/
    Cargo.toml
    src/
      lib.rs
      entities/              # moved verbatim from src/entities/
      migration/             # moved verbatim from src/migration/
      services/              # ai/ (trait+registry), reminders, vin_decode, nhtsa, + new per-domain modules
      inputs/                # NEW: domain input structs (New*/Update*)
      config.rs              # AppConfig
      error.rs               # NEW: DomainError (thiserror)
  glovebox-backend/
    Cargo.toml
    src/
      api/                   # thin handlers + request DTOs + ApiError + From<DomainError>
      main.rs                # AppState, routing, bake frontend/dist, Migrator::up()
  frontend/                  # unchanged
```

### Layering contract (per domain, e.g. `service_record`)

```
glovebox-shared::inputs::NewServiceRecord          // plain domain data, no HTTP serde
glovebox-shared::services::service_record::
    create(db, NewServiceRecord) -> Result<ServiceRecordView, DomainError>
    // owns: validation, ActiveModel build, transaction, link loading
glovebox-shared::error::DomainError                // thiserror: NotFound, Invalid{field}, Db(..), ...

glovebox-backend::api::services::
    CreateServiceRecord { .. }                     // request DTO, HTTP serde (deserialize_optional)
        --map--> NewServiceRecord
    handler: extract -> shared::services::service_record::create(&db, input) -> Json(view)
    impl From<DomainError> for ApiError            // -> IntoResponse
```

**glovebox-shared owns:** `AppConfig`, the `AiProvider` trait + registry, all entities, all migrations,
all validation, all SQL, all transactions, domain input structs, domain view structs (response shapes
like `ServiceRecordWithLinks`), `DomainError`. **Never imports `axum`.**

**glovebox-backend owns:** `AppState` (`DatabaseConnection` + `Arc<AppConfig>` + `Arc<dyn AiProvider>`),
routing, request DTOs + HTTP serde, `ApiError`/`IntoResponse`, `From<DomainError>`, static-file
serving of baked `frontend/dist`, `Migrator::up()` on boot. **No SQL, no validation.**

**Service-fn signature convention:** take `&impl ConnectionTrait` (so callers can pass a `&db` or a
`&txn`), plus `&AppConfig` / `&dyn AiProvider` as **explicit params** where a domain operation needs
config or the AI provider. Shared service fns never see `AppState`.

## Execution plan

The refactor is a no-op, so the discipline is: **keep `cargo test` + the 53 e2e green at every
commit**, and never move + rewrite in the same step.

### Phase A — Skeleton & mechanical move (zero logic change). One PR.

1. Create the workspace: `glovebox-shared` (lib) + `glovebox-backend` (bin); root `Cargo.toml`
   becomes `[workspace]`. Backend depends on shared. Split the current `[dependencies]` — shared gets
   sea-orm/sea-orm-migration/chrono/serde/reqwest/etc.; backend gets axum/tower-http/clap/etc.
2. Move `entities/`, `migration/`, `services/` (incl. `ai/` + registry), `config.rs` into
   `glovebox-shared` **verbatim**. Move `api/`, `main.rs` into `glovebox-backend`. Fix imports only
   (`crate::entities` → `glovebox_shared::entities`, etc.). Re-export a shared prelude if convenient.
3. `AppConfig` + `AiProvider` trait/registry resolve from shared; `AppState` stays in backend.
4. Backend still bakes `frontend/dist` and runs `Migrator::up()` on boot — unchanged.
5. **Gate:** `cargo build && cargo test` green; `just dev` serves the SPA; all 53 e2e green.
   Handlers still hold SQL at this checkpoint — that is expected. Phase A isolates "does the split
   compile and boot" from "did we break behavior while extracting."

### Phase B — Per-domain thinning. One domain per PR (~10–14 slices).

For each domain, simplest/best-covered first, riskiest/weakest-covered last:

1. Add `glovebox_shared::inputs::New*/Update*`, `glovebox_shared::services::<domain>::*` free fns, and
   domain view structs. Port validation + ActiveModel construction + transaction logic **down** from
   the handler, translating handler-local `ApiError` uses into `DomainError`.
2. Rewrite the handler: request DTO (HTTP serde stays) → map to domain input → call shared fn →
   `Json(view)`; add/extend `From<DomainError> for ApiError`.
3. **Gate per slice:** `cargo test` + the e2e specs touching that domain green before merge.

Suggested slice order (prove the pattern on easy domains before the hard ones):

```
platforms · shops · model_templates · mileage · costs · observations
  → parts · part_slots · documents
  → schedules · services · accidents
  → research · conversations/ai · vin/export/reminders
```

### Phase C — Enforcement & cleanup.

1. Grep-gate: `glovebox-backend` contains no SeaORM query calls (`.filter/.insert/.one/.all/.update/
   .delete/.exec`) outside DTO mapping; `glovebox-shared` has no `axum` dependency.
2. Update `TEST_PLAN.md` and `CLAUDE.md` (new layering convention: where inputs/validation/SQL live).
3. Update `2026-06-30-mde0-phase1-retrofit-sequencing.md` to record the Stage 1 method.

## Risks

| ID | Risk | Mitigation |
| -- | ---- | ---------- |
| **R1** | Business logic embedded in `src/api/` handlers | Phase B **is** the audit — each handler is read and its logic pulled down, one domain per PR, not a big-bang. |
| **R3** | Regression during a large refactor; thin CRUD unit coverage | Green-gate every Phase A/B step against the 53 e2e. For the 3 meatiest low-coverage domains (`services`, `accidents`, `research`) add characterization tests in shared **before** extracting them. |
| **R2** | Single-deployable-binary + startup migration must survive | Explicit Phase A gate: SPA served from baked `frontend/dist`; `Migrator::up()` on boot — verified before any thinning begins. |
| **R4** | Scope creep touching 21 files at once | One domain per PR keeps each diff reviewable and revertible; `puby` stays the umbrella, per-slice work tracked as child tasks under it. |

## Acceptance criteria (from `puby`)

- Cargo workspace created.
- `glovebox-shared` holds entities/, migration/, services/ (incl. `ai/` trait), validation +
  business logic, domain inputs, `DomainError`.
- `glovebox-backend` is thin Axum (extract → call shared → typed out), **no SQL / no business logic
  in handlers**.
- `cargo build` + `cargo test` green.
- Single binary still bakes `frontend/dist`; migrations still auto-run on startup (`Migrator::up()`).
- Playwright e2e green (all 53 cases).
- Behaviorally a no-op.

## Non-goals

- FTS5 / `search()` — Stage 2a (`mq9r`).
- `glovebox-mcp` / `rmcp` — Stage 2b (`mmiv`).
- The `build` primitive — `zb0w`.
- Phase 2 extraction of a cross-app `personal-domain-core` crate.
- Any behavior change, new endpoint, or schema change beyond re-homing code.

## Provenance

Scoped 2026-07-01 from `glovebox-puby` and the Stage 1 section of
`2026-06-30-mde0-phase1-retrofit-sequencing.md`. Sibling repos `../chorez` and `../kammerz` inspected
for prior art; glovebox deliberately advances the paradigm (validation + mapping in shared) rather
than copying their handler-heavy split.
