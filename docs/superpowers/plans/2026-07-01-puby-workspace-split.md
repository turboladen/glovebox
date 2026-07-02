# puby — Workspace Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the single `glovebox` binary crate into a two-crate cargo workspace — `glovebox-shared` (domain lib) + `glovebox-backend` (thin Axum bin) — with all SQL and business logic living in shared and handlers reduced to `extract → call shared → return`.

**Architecture:** Phase A does a behavior-preserving mechanical move into the workspace (handlers still hold SQL). Phase B thins one domain per task: introduce shared domain inputs + free-function services (owning validation, ActiveModel construction, transactions) + `DomainError`, then rewrite each handler to map its HTTP DTO into the domain input and map `DomainError → ApiError`. Phase C enforces the boundary and updates docs.

**Tech Stack:** Rust (edition 2024), Axum 0.8, SeaORM 1.1 (sqlx-sqlite), sea-orm-migration, thiserror, clap, Svelte 5 + Playwright (e2e regression gate).

## Global Constraints

- Behaviorally a **no-op**: no endpoint, response shape, schema, or behavior change beyond re-homing code and moving logic down a layer.
- `glovebox-shared` MUST NOT depend on `axum` (no HTTP types in the domain).
- `glovebox-backend` MUST NOT contain SeaORM query calls (`.filter/.insert/.one/.all/.update/.delete/.exec`) outside DTO→input mapping.
- Single-deployable-binary preserved: backend serves the SPA from `frontend/dist` and runs `Migrator::up()` on startup.
- **Authoritative gate = `just ci` then `just test-e2e-ci`.** `just ci` runs `just fmt-check` (nightly `cargo +nightly fmt --all --check` — `rustfmt.toml` uses nightly-only `imports_granularity`/`format_strings`), backend build/test/clippy-pedantic, and frontend check/build. Plain `cargo fmt`/`cargo test` are NOT sufficient — CI's `format` job runs nightly rustfmt and will fail otherwise. Run `just ci` (not ad-hoc cargo commands) before declaring any task green; keep it + the 53 e2e green at every commit.
- Update DTO convention preserved: HTTP request DTOs keep `Option<Option<T>>` + `deserialize_optional` for "not sent" vs "set null". Domain input structs use plain `Option<T>` for create and a per-field `Option<Option<T>>` (or dedicated patch type) only where null-vs-absent matters.
- Currency stays `i32` cents; DB datetimes stay `String`; `updated_at` set explicitly on every update via `chrono::Utc::now().format("%Y-%m-%d %H:%M:%S")`.
- Crate-level clippy allows (`option_option`, `struct_field_names`, `wildcard_imports`) carry over to whichever crate the code lands in.

---

## File Structure

**glovebox-shared/** (lib crate; package name `glovebox-shared`, lib name `glovebox_shared`)
- `src/lib.rs` — module decls + optional prelude re-export
- `src/config.rs` — `AppConfig` (moved from `src/config.rs`)
- `src/error.rs` — **NEW** `DomainError` (thiserror)
- `src/entities/` — moved verbatim from `src/entities/`
- `src/migration/` — moved verbatim from `src/migration/`
- `src/services/` — moved (`ai/`, `reminders.rs`, `vin_decode.rs`, `nhtsa.rs`) + **NEW** per-domain modules
- `src/inputs/` — **NEW** domain input structs (`New*`, `Update*`)
- `src/test_support.rs` — **NEW** in-memory SQLite harness for service tests

**glovebox-backend/** (bin crate; package name `glovebox-backend`)
- `src/main.rs` — `AppState`, routing, SPA serving, `Migrator::up()` (moved from `src/main.rs`)
- `src/api/` — moved from `src/api/`; handlers thinned in Phase B; `error.rs` keeps `ApiError` + `From<DomainError>`

**Workspace root**
- `Cargo.toml` — `[workspace]` with members `glovebox-shared`, `glovebox-backend`
- `frontend/` — unchanged
- `justfile` — update paths if any commands reference the old single-crate layout

---

## Phase A — Skeleton & mechanical move (behavior-preserving)

### Task A1: Create the workspace skeleton

**Files:**
- Modify: `Cargo.toml` (root — becomes workspace manifest)
- Create: `glovebox-shared/Cargo.toml`
- Create: `glovebox-backend/Cargo.toml`

**Interfaces:**
- Produces: workspace with two members that compile as empty crates before code moves.

- [ ] **Step 1: Back up the current root manifest**

```bash
cp Cargo.toml Cargo.toml.orig
```

- [ ] **Step 2: Write the workspace root `Cargo.toml`**

Replace root `Cargo.toml` with:

```toml
[workspace]
resolver = "2"
members = ["glovebox-shared", "glovebox-backend"]

[workspace.package]
edition = "2024"
version = "0.1.0"

[workspace.dependencies]
sea-orm = { version = "1.1", features = ["sqlx-sqlite", "runtime-tokio-rustls", "macros", "with-chrono"] }
sea-orm-migration = "1.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
tokio = { version = "1", features = ["full"] }
thiserror = "2"
anyhow = "1"
clap = { version = "4", features = ["derive", "env"] }
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }
rusty-money = { version = "0.4", features = ["iso"] }
async-trait = "0.1"
base64 = "0.22"
sqlx = { version = "0.8", features = ["sqlite"], default-features = false }
tracing = "0.1"

[profile.release]
lto = "thin"
codegen-units = 1
strip = true
```

- [ ] **Step 3: Write `glovebox-shared/Cargo.toml`**

```toml
[package]
name = "glovebox-shared"
edition.workspace = true
version.workspace = true

[lib]
name = "glovebox_shared"
path = "src/lib.rs"

[dependencies]
sea-orm.workspace = true
sea-orm-migration.workspace = true
serde.workspace = true
serde_json.workspace = true
chrono.workspace = true
tokio.workspace = true
thiserror.workspace = true
anyhow.workspace = true
clap.workspace = true
reqwest.workspace = true
rusty-money.workspace = true
async-trait.workspace = true
base64.workspace = true
sqlx.workspace = true
tracing.workspace = true
```

- [ ] **Step 4: Write `glovebox-backend/Cargo.toml`**

```toml
[package]
name = "glovebox-backend"
edition.workspace = true
version.workspace = true

[[bin]]
name = "glovebox-backend"
path = "src/main.rs"

[dependencies]
glovebox-shared = { path = "../glovebox-shared" }
axum = { version = "0.8", features = ["multipart"] }
tokio.workspace = true
sea-orm.workspace = true
sea-orm-migration.workspace = true
serde.workspace = true
serde_json.workspace = true
chrono.workspace = true
tower-http = { version = "0.6", features = ["fs", "cors", "trace"] }
tracing.workspace = true
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
thiserror.workspace = true
anyhow.workspace = true
clap.workspace = true
base64.workspace = true
```

- [ ] **Step 5: Create placeholder crate roots so the workspace compiles**

```bash
mkdir -p glovebox-shared/src glovebox-backend/src
printf '%s\n' '// moved in A2' > glovebox-shared/src/lib.rs
printf '%s\n' 'fn main() {}' > glovebox-backend/src/main.rs
```

- [ ] **Step 6: Verify the empty workspace builds**

Run: `cargo build`
Expected: PASS — both members compile (empty).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml glovebox-shared/Cargo.toml glovebox-backend/Cargo.toml glovebox-shared/src/lib.rs glovebox-backend/src/main.rs
git rm --cached Cargo.toml.orig 2>/dev/null; rm -f Cargo.toml.orig
git commit -m "chore(puby): scaffold two-crate workspace skeleton"
```

### Task A2: Move the domain into glovebox-shared (verbatim)

**Files:**
- Move: `src/entities/` → `glovebox-shared/src/entities/`
- Move: `src/migration/` → `glovebox-shared/src/migration/`
- Move: `src/services/` → `glovebox-shared/src/services/`
- Move: `src/config.rs` → `glovebox-shared/src/config.rs`
- Modify: `glovebox-shared/src/lib.rs`

**Interfaces:**
- Produces: `glovebox_shared::{entities, migration, services, config}`; `glovebox_shared::config::AppConfig`; `glovebox_shared::migration::Migrator`; `glovebox_shared::services::ai::registry::AiProviderRegistry`.

- [ ] **Step 1: Move the directories with git**

```bash
git mv src/entities glovebox-shared/src/entities
git mv src/migration glovebox-shared/src/migration
git mv src/services glovebox-shared/src/services
git mv src/config.rs glovebox-shared/src/config.rs
```

- [ ] **Step 2: Write `glovebox-shared/src/lib.rs`**

```rust
// Intentional conventions that conflict with clippy::pedantic (see CLAUDE.md):
#![allow(clippy::option_option, clippy::struct_field_names, clippy::wildcard_imports)]

pub mod config;
pub mod entities;
pub mod migration;
pub mod services;
```

- [ ] **Step 3: Fix intra-crate paths inside moved code**

Within `glovebox-shared/src/**`, any `use crate::entities` / `crate::services` / `crate::config` references still resolve (same crate), so no change is expected. If any moved file referenced `crate::api` or `crate::AppState`, that is a business-logic leak — note it for its Phase B domain and, for now, make it compile by inlining the needed value. Search:

```bash
grep -rn 'crate::api\|crate::AppState\|AppState' glovebox-shared/src || echo "clean"
```
Expected: `clean` (services today take `&db`/params, not `AppState`).

- [ ] **Step 4: Verify glovebox-shared compiles alone**

Run: `cargo build -p glovebox-shared`
Expected: PASS.

- [ ] **Step 5: Run the moved unit tests**

Run: `cargo test -p glovebox-shared`
Expected: PASS — the ai/nhtsa tests that moved with `services/` still pass.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor(puby): move entities/migration/services/config into glovebox-shared"
```

### Task A3: Move the backend and re-point imports

**Files:**
- Move: `src/api/` → `glovebox-backend/src/api/`
- Move: `src/main.rs` → `glovebox-backend/src/main.rs`
- Modify: `glovebox-backend/src/main.rs`, `glovebox-backend/src/api/**`

**Interfaces:**
- Consumes: `glovebox_shared::{entities, services, config, migration}`.
- Produces: a working `glovebox-backend` binary identical in behavior to the old `glovebox` binary.

- [ ] **Step 1: Move the files**

```bash
git mv src/api glovebox-backend/src/api
git mv src/main.rs glovebox-backend/src/main.rs
rmdir src 2>/dev/null || true
```

- [ ] **Step 2: Rewrite the module/`use` preamble of `main.rs`**

In `glovebox-backend/src/main.rs`, delete the `mod config; mod entities; mod migration; mod services;` lines (keep `mod api;`). Change the domain `use`s to shared paths:

```rust
mod api;

use std::{path::Path, sync::Arc};

use axum::{Router, routing::get};
use clap::Parser;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;
use tower_http::{cors::CorsLayer, services::{ServeDir, ServeFile}};

use glovebox_shared::config::AppConfig;
use glovebox_shared::migration::Migrator;
use glovebox_shared::services::ai::registry::AiProviderRegistry;
```

Leave the `AppState` struct and the whole `main()` body unchanged (routing, `Migrator::up`, `frontend/dist` serving all stay).

- [ ] **Step 3: Re-point domain imports across the api layer**

Every handler references `crate::entities::…` and `crate::AppState`. `AppState` still lives in the backend crate (fine as `crate::AppState`), but `entities`/`services`/`config` now live in shared. Replace crate-domain paths in `glovebox-backend/src/api/`:

```bash
cd glovebox-backend
grep -rl 'crate::entities' src/api | xargs sed -i '' 's/crate::entities/glovebox_shared::entities/g'
grep -rl 'crate::services' src/api | xargs sed -i '' 's/crate::services/glovebox_shared::services/g'
grep -rl 'crate::config'  src/api | xargs sed -i '' 's/crate::config/glovebox_shared::config/g'
cd ..
```

Handlers that import via `use crate::{AppState, entities::…}` need the split preserved — verify each such `use` compiles; where a combined `use crate::{AppState, entities::foo}` remains, change to two lines: `use crate::AppState;` and `use glovebox_shared::entities::foo;`.

- [ ] **Step 4: Build the backend**

Run: `cargo build -p glovebox-backend`
Expected: PASS. Fix any remaining unresolved paths reported by the compiler (they will name the exact file/line).

- [ ] **Step 5: Full workspace build + test**

Run: `cargo build && cargo test`
Expected: PASS — all 51 unit tests green.

- [ ] **Step 6: Boot + SPA smoke check (R2 gate)**

```bash
cargo run -p glovebox-backend &
sleep 3
curl -sf http://localhost:3003/api/health && echo " health OK"
curl -sf http://localhost:3003/ | grep -q '<div id="app"' && echo "SPA served OK"
kill %1
```
Expected: `health OK` and `SPA served OK` — confirms baked `frontend/dist` + `Migrator::up()` survived the move. (Build the SPA first with `cd frontend && bun run build` if `frontend/dist` is stale.)

- [ ] **Step 7: e2e regression gate**

Run: `just dev` in one shell, then `just test-e2e` in another.
Expected: all 53 Playwright cases PASS.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor(puby): move api+main into glovebox-backend, re-point domain imports"
```

### Task A4: Update justfile and CI paths

**Files:**
- Modify: `justfile`
- Modify: any CI workflow under `.github/` that runs `cargo run`/`cargo build`

**Interfaces:**
- Produces: `just dev`, `just test-e2e`, and CI green against the workspace layout.

- [ ] **Step 1: Point `cargo run` at the backend bin**

In `justfile`, change the backend run recipe to `cargo run -p glovebox-backend` (and any `cargo build` to `cargo build --workspace`). Leave frontend recipes unchanged.

- [ ] **Step 2: Verify `just dev` boots both**

Run: `just dev`
Expected: backend on :3003, frontend dev on :5373, `/api/health` OK.

- [ ] **Step 3: Verify CI invocations locally**

Run the exact build/test commands your CI uses (e.g. `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D clippy::pedantic`).
Expected: PASS. Fix any hard-coded single-crate paths.

- [ ] **Step 4: Commit**

```bash
git add justfile .github 2>/dev/null; git add -A
git commit -m "chore(puby): update justfile + CI for workspace layout"
```

**Phase A complete when:** `cargo build --workspace && cargo test` green, backend boots serving the SPA with migrations applied, and all 53 e2e pass. Handlers still contain SQL — expected. Open PR #A for Phase A; merge before starting Phase B.

---

## Phase B — Per-domain thinning

Phase B thins one domain per task. Task B0 lays the shared foundation (error type, module scaffolding, test harness, HTTP error mapping). Task B1 is the **fully-worked exemplar** (`platforms`). Tasks B2+ apply the **Phase B Slice Recipe** with per-domain values from the delta table.

### Task B0: Shared foundation — DomainError, inputs/services modules, test harness, error mapping

**Files:**
- Create: `glovebox-shared/src/error.rs`
- Create: `glovebox-shared/src/inputs/mod.rs`
- Create: `glovebox-shared/src/test_support.rs`
- Modify: `glovebox-shared/src/lib.rs`, `glovebox-shared/src/services/mod.rs`
- Modify: `glovebox-shared/Cargo.toml` (dev-dependency), `glovebox-backend/src/api/error.rs`

**Interfaces:**
- Produces:
  - `glovebox_shared::error::DomainError` with variants `NotFound(String)`, `Invalid { field: String, message: String }`, `Db(sea_orm::DbErr)`, `Internal(String)`; `impl From<sea_orm::DbErr> for DomainError`; `pub type DomainResult<T> = Result<T, DomainError>`.
  - `glovebox_shared::test_support::test_db() -> DatabaseConnection` (in-memory SQLite, migrations applied).
  - `glovebox-backend`'s `ApiError` gains `impl From<DomainError> for ApiError`.

- [ ] **Step 1: Write `glovebox-shared/src/error.rs`**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("{0}")]
    NotFound(String),
    #[error("{field}: {message}")]
    Invalid { field: String, message: String },
    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
    #[error("{0}")]
    Internal(String),
}

pub type DomainResult<T> = Result<T, DomainError>;

impl DomainError {
    pub fn invalid(field: impl Into<String>, message: impl Into<String>) -> Self {
        DomainError::Invalid { field: field.into(), message: message.into() }
    }
}
```

- [ ] **Step 2: Add modules to `glovebox-shared/src/lib.rs`**

Add below the existing module decls:

```rust
pub mod error;
pub mod inputs;

#[cfg(test)]
pub mod test_support;
```

- [ ] **Step 3: Create `glovebox-shared/src/inputs/mod.rs`**

```rust
//! Domain input structs (plain data) consumed by service functions.
//! HTTP request DTOs in glovebox-backend map INTO these; the MCP surface builds them directly.
```

- [ ] **Step 4: Add the in-memory test harness `glovebox-shared/src/test_support.rs`**

```rust
use sea_orm::{Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;

use crate::migration::Migrator;

/// A fresh in-memory SQLite DB with all migrations applied. For service-layer unit tests.
pub async fn test_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("connect in-memory sqlite");
    Migrator::up(&db, None).await.expect("run migrations");
    db
}
```

- [ ] **Step 5: Wire `services/mod.rs` to expose new domain modules**

In `glovebox-shared/src/services/mod.rs`, keep existing (`pub mod ai; pub mod reminders; pub mod vin_decode; pub mod nhtsa;`). New domain modules (`pub mod platform;` etc.) are added by each Phase B task.

- [ ] **Step 6: Add `From<DomainError>` to the backend's ApiError**

In `glovebox-backend/src/api/error.rs`, add below the existing `From<DbErr>`:

```rust
impl From<glovebox_shared::error::DomainError> for ApiError {
    fn from(err: glovebox_shared::error::DomainError) -> Self {
        use glovebox_shared::error::DomainError;
        match err {
            DomainError::NotFound(m) => ApiError::NotFound(m),
            DomainError::Invalid { field, message } => {
                ApiError::BadRequest(format!("{field}: {message}"))
            }
            DomainError::Db(e) => ApiError::Internal(e.to_string()),
            DomainError::Internal(m) => ApiError::Internal(m),
        }
    }
}
```

- [ ] **Step 7: Build + verify harness compiles under test**

Run: `cargo build --workspace && cargo test -p glovebox-shared`
Expected: PASS (no new tests yet; harness compiles under `#[cfg(test)]`).

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat(puby): shared DomainError + inputs/test_support scaffolding + ApiError mapping"
```

---

### Phase B Slice Recipe (followed by B1–BN)

For a domain `D` with entity module `d` (e.g. `platform`), do exactly this. **All code steps show real code in the domain's own task; this recipe is the procedure, not a substitute for that code.**

1. **Add domain input structs** to `glovebox-shared/src/inputs/<d>.rs` (and `pub mod <d>;` in `inputs/mod.rs`): a `New<D>` (plain `Option<T>`, mirrors the create DTO fields minus HTTP serde) and, if the domain has an update endpoint, an `Update<D>` using `Option<Option<T>>` only for fields where null-vs-absent matters.
2. **Add the service module** `glovebox-shared/src/services/<d>.rs` (and `pub mod <d>;` in `services/mod.rs`) with free functions: `list`, `get`, `create`, `update`, `delete` as the domain has them. Signatures take `db: &impl ConnectionTrait` first. `create`/`update` take the domain input; `get`/`update`/`delete` return `DomainError::NotFound` when the row is absent. Move validation, `ActiveModel` construction, `updated_at` stamping, and transactions here. Return entity `Model`s or domain view structs.
3. **Write service unit tests** in `services/<d>.rs` `#[cfg(test)]` using `crate::test_support::test_db()`: at minimum a create→get round-trip and an update-mutates-field test; add a rejection test for every validation rule introduced. Run them red→green.
4. **Thin the handler** `glovebox-backend/src/api/<d>.rs`: keep the request DTOs + HTTP serde; each handler maps its DTO into the domain input, calls `glovebox_shared::services::<d>::…(&state.db, input)?`, and returns `Json(..)`. Replace the local `type Result<T> = …ApiError…` usage so `?` on a `DomainResult` converts via the `From<DomainError>` added in B0. Remove all SeaORM calls from the handler. Preserve `require_vehicle` ownership checks for sub-resources (call it in the handler before delegating, OR pass `vehicle_id` into the service fn which performs the check — pick per domain and note it).
5. **Gate:** `cargo test --workspace` + the domain's e2e spec(s) green. Commit.

**Ownership-check note:** for vehicle sub-resources, move the `require_vehicle` check into the service fn (so the MCP surface gets it too): add `glovebox_shared::services::vehicle::require(db, vehicle_id) -> DomainResult<vehicle::Model>` in the `vehicle` slice, and have sub-resource service fns call it first. Until the `vehicle` slice lands, keep `require_vehicle` in the backend handler.

---

### Task B1: Thin `platforms` (worked exemplar)

**Files:**
- Create: `glovebox-shared/src/inputs/platform.rs`
- Create: `glovebox-shared/src/services/platform.rs`
- Modify: `glovebox-shared/src/inputs/mod.rs`, `glovebox-shared/src/services/mod.rs`
- Modify: `glovebox-backend/src/api/platforms.rs`

**Interfaces:**
- Consumes: `glovebox_shared::error::{DomainError, DomainResult}`, `glovebox_shared::entities::platform`, `glovebox_shared::test_support::test_db`.
- Produces:
  - `glovebox_shared::inputs::platform::{NewPlatform, UpdatePlatform}`
  - `glovebox_shared::services::platform::{list, get, create, update}` with signatures:
    - `async fn list(db: &impl ConnectionTrait) -> DomainResult<Vec<platform::Model>>`
    - `async fn get(db: &impl ConnectionTrait, id: i32) -> DomainResult<platform::Model>`
    - `async fn create(db: &impl ConnectionTrait, input: NewPlatform) -> DomainResult<platform::Model>`
    - `async fn update(db: &impl ConnectionTrait, id: i32, input: UpdatePlatform) -> DomainResult<platform::Model>`

- [ ] **Step 1: Write domain inputs `glovebox-shared/src/inputs/platform.rs`**

```rust
pub struct NewPlatform {
    pub name: String,
    pub website_url: Option<String>,
    pub api_base_url: Option<String>,
    pub notes: Option<String>,
}

#[derive(Default)]
pub struct UpdatePlatform {
    pub name: Option<String>,
    pub website_url: Option<Option<String>>,
    pub api_base_url: Option<Option<String>>,
    pub notes: Option<Option<String>>,
}
```

Add `pub mod platform;` to `glovebox-shared/src/inputs/mod.rs`.

- [ ] **Step 2: Write the failing service test in `glovebox-shared/src/services/platform.rs`**

```rust
use sea_orm::*;

use crate::entities::platform;
use crate::error::{DomainError, DomainResult};
use crate::inputs::platform::{NewPlatform, UpdatePlatform};

// (functions added in Step 4)

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_db;

    #[tokio::test]
    async fn create_then_get_round_trips() {
        let db = test_db().await;
        let created = create(&db, NewPlatform {
            name: "RockAuto".into(),
            website_url: Some("https://rockauto.com".into()),
            api_base_url: None,
            notes: None,
        }).await.unwrap();
        let fetched = get(&db, created.id).await.unwrap();
        assert_eq!(fetched.name, "RockAuto");
        assert_eq!(fetched.website_url.as_deref(), Some("https://rockauto.com"));
    }

    #[tokio::test]
    async fn create_rejects_blank_name() {
        let db = test_db().await;
        let err = create(&db, NewPlatform {
            name: "   ".into(), website_url: None, api_base_url: None, notes: None,
        }).await.unwrap_err();
        assert!(matches!(err, DomainError::Invalid { .. }));
    }

    #[tokio::test]
    async fn update_sets_name_and_clears_notes() {
        let db = test_db().await;
        let p = create(&db, NewPlatform {
            name: "A".into(), website_url: None, api_base_url: None, notes: Some("x".into()),
        }).await.unwrap();
        let updated = update(&db, p.id, UpdatePlatform {
            name: Some("B".into()),
            notes: Some(None),               // explicit null clears it
            ..Default::default()
        }).await.unwrap();
        assert_eq!(updated.name, "B");
        assert_eq!(updated.notes, None);
    }

    #[tokio::test]
    async fn get_missing_is_not_found() {
        let db = test_db().await;
        assert!(matches!(get(&db, 999).await.unwrap_err(), DomainError::NotFound(_)));
    }
}
```

- [ ] **Step 3: Run the tests to confirm they fail**

Run: `cargo test -p glovebox-shared services::platform`
Expected: FAIL — `create`/`get`/`update` not defined.

- [ ] **Step 4: Implement the service functions (above the `#[cfg(test)]` block)**

```rust
pub async fn list(db: &impl ConnectionTrait) -> DomainResult<Vec<platform::Model>> {
    Ok(platform::Entity::find().all(db).await?)
}

pub async fn get(db: &impl ConnectionTrait, id: i32) -> DomainResult<platform::Model> {
    platform::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Platform {id} not found")))
}

pub async fn create(db: &impl ConnectionTrait, input: NewPlatform) -> DomainResult<platform::Model> {
    if input.name.trim().is_empty() {
        return Err(DomainError::invalid("name", "must not be blank"));
    }
    let model = platform::ActiveModel {
        name: Set(input.name),
        website_url: Set(input.website_url),
        api_base_url: Set(input.api_base_url),
        notes: Set(input.notes),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

pub async fn update(
    db: &impl ConnectionTrait,
    id: i32,
    input: UpdatePlatform,
) -> DomainResult<platform::Model> {
    let existing = get(db, id).await?;
    let mut active: platform::ActiveModel = existing.into();
    if let Some(v) = input.name {
        if v.trim().is_empty() {
            return Err(DomainError::invalid("name", "must not be blank"));
        }
        active.name = Set(v);
    }
    if let Some(v) = input.website_url { active.website_url = Set(v); }
    if let Some(v) = input.api_base_url { active.api_base_url = Set(v); }
    if let Some(v) = input.notes { active.notes = Set(v); }
    Ok(active.update(db).await?)
}
```

Add `pub mod platform;` to `glovebox-shared/src/services/mod.rs`.

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test -p glovebox-shared services::platform`
Expected: PASS (4 tests).

- [ ] **Step 6: Thin the handler `glovebox-backend/src/api/platforms.rs`**

Replace the file body with (DTOs keep HTTP serde; handlers delegate):

```rust
use axum::{Json, Router, extract::{Path, State}, routing::get};
use serde::Deserialize;

use crate::AppState;
use glovebox_shared::entities::platform;
use glovebox_shared::inputs::platform::{NewPlatform, UpdatePlatform as UpdatePlatformInput};
use glovebox_shared::services::platform as svc;

use super::{error::ApiError, serde_helpers::deserialize_optional};

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct CreatePlatform {
    pub name: String,
    pub website_url: Option<String>,
    pub api_base_url: Option<String>,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdatePlatform {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub website_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub api_base_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional")]
    pub notes: Option<Option<String>>,
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<platform::Model>>> {
    Ok(Json(svc::list(&state.db).await?))
}

async fn get_one(State(state): State<AppState>, Path(id): Path<i32>) -> Result<Json<platform::Model>> {
    Ok(Json(svc::get(&state.db, id).await?))
}

async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreatePlatform>,
) -> Result<Json<platform::Model>> {
    let created = svc::create(&state.db, NewPlatform {
        name: input.name,
        website_url: input.website_url,
        api_base_url: input.api_base_url,
        notes: input.notes,
    }).await?;
    Ok(Json(created))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(input): Json<UpdatePlatform>,
) -> Result<Json<platform::Model>> {
    let updated = svc::update(&state.db, id, UpdatePlatformInput {
        name: input.name,
        website_url: input.website_url,
        api_base_url: input.api_base_url,
        notes: input.notes,
    }).await?;
    Ok(Json(updated))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).put(update))
}
```

- [ ] **Step 7: Build + confirm no SQL remains in the handler**

Run: `cargo build --workspace && ! grep -nE '\.(filter|insert|one|all|update|delete|exec)\(' glovebox-backend/src/api/platforms.rs && echo "no SQL in handler"`
Expected: build PASS and `no SQL in handler`.

- [ ] **Step 8: Full test + platform e2e gate**

Run: `cargo test --workspace` then (with `just dev` running) `cd frontend && bunx playwright test e2e/garage.spec.ts` (platforms surface in garage/vehicle-new).
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "refactor(puby): thin platforms handler onto glovebox-shared::services::platform"
```

### Tasks B2–BN: Remaining domains (apply the Slice Recipe)

Each task follows the **Phase B Slice Recipe** and the B1 exemplar's code shape, substituting the values below. Each is its own PR/commit and must leave `cargo test --workspace` + its e2e spec(s) green. Order runs simplest/best-covered → riskiest/weakest-covered.

| # | Domain | Handler file | Entity module(s) | Service fns | Sub-resource? (ownership) | Gating e2e spec | Notes |
|---|--------|--------------|------------------|-------------|---------------------------|-----------------|-------|
| B2 | shops | `api/shops.rs` | `shop` | list/get/create/update/delete | no (top-level) | `garage`, `vehicle-detail` | mirror B1 exactly |
| B3 | model_templates | `api/model_templates.rs` | `model_template` | list/get/create/update/delete | no | `vehicle-new` | |
| B4 | mileage | `api/mileage.rs` | `mileage_log` | list/create | yes → `require_vehicle` | `vehicle-detail` | smallest; keep `require` in handler until B12 |
| B5 | costs | `api/costs.rs` | (read-only aggregation) | `costs(db, vehicle_id)` view | yes | `vehicle-detail` | pure read; move the cents math into shared |
| B6 | observations | `api/observations.rs` | `observation` | list/get/create/update | yes | `observations` | |
| B7 | part_slots | `api/part_slots.rs` | `part_slot` | list/get/create/update/delete | yes | `parts` | |
| B8 | parts | `api/parts.rs` | `part` | list/get/create/update/delete | yes | `parts` | preserve `installed_service_id` links |
| B9 | documents | `api/documents.rs` | `document` | list/get/upload/delete | query-filtered | `documents`, `invoice-parse` | multipart stays in handler; only persistence + text-extraction call move to shared |
| B10 | schedules | `api/schedules.rs` | `maintenance_schedule_item`, `service_schedule_link` | list/get/create/update/delete + `resolve(db, vehicle_id)` | mixed (top-level + `resolve` per-vehicle) | `vehicle-detail`, `suggestions` | resolve() interval logic → shared |
| B11 | **services** | `api/services.rs` | `service_record`, `service_record_line_item`, `service_schedule_link`, `part` | list/get/create/update/delete | yes | `vehicle-detail` | **characterization tests first** (txn: line items + schedule links + part links). Move `load_service_links`/`insert_line_items` into shared. |
| B12 | vehicles | `api/vehicles.rs` | `vehicle`, `vehicle_attribute` | list/get/create/update/archive/delete + `require(db, id)` | defines ownership check | `garage`, `vehicle-new`, `vehicle-detail` | adds `services::vehicle::require`; then migrate B4–B11 sub-resources to call it (follow-up step in this task) |
| B13 | **accidents** | `api/accidents.rs` | `accident`, `accident_correspondence`, `accident_service_link` | list/get/create/update + correspondence list/create | yes | (add spec if missing) | **characterization tests first**; multi-table |
| B14 | **research** | `api/research.rs` | `research_report`, `research_finding` | list_reports/get_report/generate/list_findings/update_finding + `check_recalls` | yes | `research`, `suggestions` | **characterization tests first**; keep existing unit tests green; AI provider passed as explicit param |
| B15 | ai / conversations | `api/ai.rs`, `api/conversations.rs` | `conversation`, `chat_message`, `ai_provider_config` | conversation CRUD + chat/parse/providers | mixed | `chat`, `suggestions`, `invoice-parse` | services take `&dyn AiProvider`/`&AiProviderRegistry` + `&AppConfig` as explicit params; heaviest, do last |
| B16 | vin / export / reminders | `api/vin.rs`, `api/export.rs`, `api/reminders.rs` | (uses existing `vin_decode`/`reminders`/`nhtsa` services) | wire handlers to existing shared services; move `export` assembly + `reminders` handler glue into shared | mixed | `navigation`, `vehicle-detail` | mostly already service-backed; thin the remaining glue |

For each B-task: (1) create inputs + service module + tests (red→green), (2) thin handler, (3) grep-assert no SQL remains in that handler file, (4) `cargo test --workspace` + gating e2e green, (5) commit `refactor(puby): thin <domain> onto glovebox-shared`.

---

## Phase C — Enforcement & cleanup

### Task C1: Boundary enforcement gate

**Files:**
- Create: `scripts/check-layering.sh`
- Modify: CI workflow under `.github/`

**Interfaces:**
- Produces: a CI-runnable check that fails if SQL leaks into the backend or axum leaks into shared.

- [ ] **Step 1: Write `scripts/check-layering.sh`**

```bash
#!/usr/bin/env bash
set -euo pipefail

# No SeaORM query calls in backend handlers (DTO mapping only allowed).
if grep -rnE '\.(filter|insert|one|all|update|delete|exec)\(' glovebox-backend/src/api; then
  echo "FAIL: SeaORM query call found in glovebox-backend/src/api (must live in glovebox-shared)"
  exit 1
fi

# glovebox-shared must not depend on axum.
if grep -qE '^\s*axum' glovebox-shared/Cargo.toml; then
  echo "FAIL: glovebox-shared depends on axum (domain must stay HTTP-agnostic)"
  exit 1
fi

echo "layering OK"
```

- [ ] **Step 2: Make it executable and run it**

```bash
chmod +x scripts/check-layering.sh
./scripts/check-layering.sh
```
Expected: `layering OK`. If it fails, the named handler still holds SQL — finish thinning it.

- [ ] **Step 3: Add the check to CI**

Add a step running `./scripts/check-layering.sh` to the backend CI job.

- [ ] **Step 4: Commit**

```bash
git add scripts/check-layering.sh .github
git commit -m "chore(puby): CI gate enforcing shared/backend layering boundary"
```

### Task C2: Docs — conventions + sequencing record

**Files:**
- Modify: `CLAUDE.md`
- Modify: `TEST_PLAN.md`
- Modify: `docs/superpowers/specs/2026-06-30-mde0-phase1-retrofit-sequencing.md`

**Interfaces:**
- Produces: written conventions so future work (mq9r, mmiv) follows the layering.

- [ ] **Step 1: Add a layering convention to `CLAUDE.md`**

Under Conventions, add: domain inputs + validation + SQL live in `glovebox-shared::{inputs,services}`; backend handlers only map DTO↔input and `DomainError→ApiError`; `glovebox-shared` never imports axum; service fns take `&impl ConnectionTrait` (+ `&AppConfig`/`&dyn AiProvider`) as explicit params, never `AppState`.

- [ ] **Step 2: Note the new shared-service test harness in `TEST_PLAN.md`**

Document `glovebox_shared::test_support::test_db()` and that new domain logic gets service-level unit tests there in addition to e2e.

- [ ] **Step 3: Mark Stage 1 method in the sequencing spec**

Add a one-line status to the `puby` section: split done via Phase A mechanical move + Phase B per-domain thinning; layering enforced by `scripts/check-layering.sh`.

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md TEST_PLAN.md docs/superpowers/specs/2026-06-30-mde0-phase1-retrofit-sequencing.md
git commit -m "docs(puby): record layering convention + shared test harness + Stage 1 method"
```

### Task C3: Final full-suite verification

- [ ] **Step 1: Clean build + clippy**

Run: `cargo build --workspace && cargo clippy --workspace -- -D clippy::pedantic`
Expected: PASS, zero warnings.

- [ ] **Step 2: Full test suite**

Run: `cargo test --workspace`
Expected: PASS (moved unit tests + new service tests).

- [ ] **Step 3: Full e2e**

Run: `just dev` then `just test-e2e`.
Expected: all 53 cases PASS.

- [ ] **Step 4: Layering + boot gate**

Run: `./scripts/check-layering.sh` and the Task A3 boot/SPA smoke check.
Expected: `layering OK`, `health OK`, `SPA served OK`.

- [ ] **Step 5: Confirm the acceptance criteria**

Verify against `puby`: workspace created; shared holds entities/migration/services/validation/inputs/DomainError; backend thin; build+test green; single binary bakes `frontend/dist`; migrations auto-run; e2e green; behaviorally a no-op. Close `puby` (on `main`, per the bead-state-on-main rule) after the final PR merges.

---

## Self-Review

- **Spec coverage:** workspace + fold entities/migration (A1–A2, D-crates ✓); thin backend / no SQL (Phase B + C1 ✓); domain inputs in shared, serde in backend (B0/B1 D-inputs ✓); validation+mapping in shared (B1 D-validation ✓); free functions (B1 D-shape ✓); DomainError + From mapping (B0 D-error ✓); single binary + Migrator::up (A3 Step 6, R2 ✓); e2e gate throughout (R3 ✓); characterization tests before extraction for services/accidents/research (B11/B13/B14, R3 ✓); handler audit is Phase B (R1 ✓); one-domain-per-PR (R4 ✓); enforcement gate (C1 ✓); docs (C2 ✓). All spec sections mapped.
- **Placeholder scan:** exemplar (B1) and foundation (B0) carry full code; B2–B16 are a delta table over the B1 recipe with exact files/entities/fns/gates per row (intentionally DRY, not placeholder). No "TBD"/"add error handling"/"write tests for the above" left.
- **Type consistency:** `DomainError`, `DomainResult`, `NewPlatform`/`UpdatePlatform`, `svc::{list,get,create,update}` signatures, `test_db()`, and `From<DomainError> for ApiError` names match across B0/B1 and the recipe.
