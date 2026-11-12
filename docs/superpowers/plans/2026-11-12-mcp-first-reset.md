# Glovebox MCP-first Reset Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Claude Desktop the only read and write interface to glovebox, over a stdio MCP server whose tools and schema hold the detail a real invoice carries.

**Architecture:** Two crates remain: `glovebox-shared` (domain library: entities, one squashed migration, services) and `glovebox-mcp` (rmcp tool surface plus a stdio binary that Claude Desktop launches). The frontend, the Playwright suite, the HTTP API and `glovebox-backend` are deleted. Every tool body stays arg-struct → shared service → serialize.

**Tech Stack:** Rust 2024, SeaORM 1.1 + SQLite (FTS5), rmcp 2.2 (stdio transport), schemars 1, `just`.

**Spec:** `docs/superpowers/specs/2026-11-12-mcp-first-reset-design.md`

## Global Constraints

- No data or migration compatibility. The existing `data/glovebox.db` is disposable.
- No document storage, no file paths on any record, no web UI, no HTTP surface.
- All SQL, validation and business logic live in `glovebox-shared`. Service fns take `db: &impl ConnectionTrait` first and return `DomainResult<T>`.
- Every foreign id a service accepts is checked against the vehicle. A wrong-vehicle id returns a `NotFound` byte-identical to a nonexistent id, with a regression test per link.
- Every update fn stamps `updated_at` with `chrono::Utc::now().format("%Y-%m-%d %H:%M:%S")`.
- Currency is integer cents (`i32`). Dates and datetimes are `String` at the SeaORM boundary.
- Entity field order matches the physical column order of the squashed migration.
- Tool names are verbs a person would say. Every `#[tool]` pairs `LenientParameters<T>` with `input_schema = schema_for_type::<T>()`.
- The stdio binary writes nothing to stdout except protocol frames. Logs go to stderr.
- Gate for every task: `just ci` passes (`cargo +nightly fmt --all --check`, layering script, `cargo build/test --workspace --locked`, `cargo clippy --workspace -- -D clippy::pedantic`).
- Comments follow the owner's global rules: full sentences, no history narration, no `//` block over 10 lines, tests use `//` not `///`.
- Never stage `.beads/issues.jsonl` in these commits (`git restore --staged .beads/issues.jsonl` if it gets picked up). Commits end with `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.
- rmcp's exact stdio and in-process client API must be read from the rmcp 2.2 docs (Context7) or the crate source under `~/.cargo/registry/src/index.crates.io-*/rmcp-2.2*/` before Task 4 is written. Do not write it from memory.

## Review Focus

1. **Anything printed to stdout corrupts the protocol.** A stray `println!`, a tracing subscriber on stdout, or migration output breaks every tool call. Expected: all diagnostics on stderr. Pinned in Task 4.
2. **One bad id in `record_service` must commit nothing.** A work item, recall, incident, schedule item or part id from another vehicle returns `NotFound`, and no service, line item, part, mileage reading or link is left behind. Pinned in Tasks 5–8.
3. **Re-filing an invoice already recorded.** The same `invoice_ref` with different content returns the existing record flagged as already recorded, and creates no second set of line items or parts. Pinned in Task 5.
4. **Deleting a service that closed things.** Recalls it performed go back to open, incidents it resolved go back to unresolved when no other resolving service remains, and work items it completed go back to `planned`. Pinned in Task 10.
5. **"What's coming" on a car with thin data.** No mileage readings, no schedule template, a work item with neither revisit field, and a revisit date in the past must each produce a sensible answer and never an error. Pinned in Task 9.

---

## Final schema (authored in Task 3, used by every later task)

Kept tables keep their current columns except as listed. "Removed" columns are simply absent from the squashed migration.

| Table | Change |
|---|---|
| `service_records` | Add `performed_by TEXT NOT NULL DEFAULT 'shop'`, `end_date TEXT NULL`. Remove `build_id`. Keep `invoice_ref` and its partial unique index on `(vehicle_id, invoice_ref)`. |
| `service_record_line_items` | Add `work_kind TEXT NOT NULL`. Remove `category`. |
| `parts` | Add `supplied_by TEXT NOT NULL DEFAULT 'self'`. Remove `build_id`, `invoice_url`. |
| `work_items` | Add `revisit_by_date TEXT NULL`, `revisit_by_miles INTEGER NULL`, `completed_service_id INTEGER NULL REFERENCES service_records(id) ON DELETE SET NULL`. Remove `visit_id`, `build_id`. |
| `incidents` | Remove `build_id`. |
| `incident_service_links` | Add `role TEXT NOT NULL`; primary key becomes `(incident_id, service_record_id, role)`. Both FKs `ON DELETE CASCADE`. |
| `service_recall_links` (new) | `service_record_id`, `research_finding_id`, composite primary key, both FKs `ON DELETE CASCADE`. |
| `research_findings` | Remove `linked_entity_type`, `linked_entity_id`. |
| `vehicles` | Remove `photo_path`. |
| Removed entirely | `builds`, `visits`, `documents`, `vehicle_attributes`, and their FTS tables and triggers. |

Whitelists (constants in the owning service module):

- `service_record::VALID_PERFORMED_BY = ["shop", "self"]`
- `service_record::VALID_WORK_KINDS = ["maintenance", "mod", "repair", "inspection"]`
- `part::VALID_SUPPLIED_BY = ["self", "shop"]`
- `work_item::VALID_STATUSES = ["planned", "done", "dropped"]`
- `incident::VALID_CATEGORIES` gains `"shop_observation"`
- `incident::VALID_LINK_ROLES = ["noticed", "resolved"]`

## File map

- Delete: `frontend/`, `glovebox-backend/`, `TEST_PLAN.md`, `.claude/skills/new-e2e-test/`, `.claude/skills/new-entity/`, empty dirs `git/` and `pull/`, `data/e2e.db*`, `data/e2e-files/`.
- `glovebox-shared/src/migration/`: replace the 22 files with `m20261112_000001_initial_schema.rs` and `m20261112_000002_seed_vw_mqb.rs`.
- `glovebox-shared/src/entities/`: delete `build.rs`, `visit.rs`, `document.rs`, `vehicle_attribute.rs`; create `service_recall_link.rs`; modify the rest per the schema table.
- `glovebox-shared/src/services/`: delete `build.rs`, `visit.rs`, `document.rs`, `dashboard.rs`, `export.rs`, `vin_decode.rs`; create `recall.rs`, `upcoming.rs`; create `glovebox-shared/src/db.rs`.
- `glovebox-shared/src/inputs/`: delete `build.rs`, `visit.rs`, `document.rs`.
- `glovebox-mcp/src/`: create `main.rs`; `lib.rs` shrinks to module declarations and `pub use handler::GloveboxMcp`; `handler.rs` is split into `handler/{mod,vehicles,services,parts,incidents,planning,schedule,recalls,reads}.rs` in Task 4 (it is already 954 lines and roughly doubles).
- `glovebox-mcp/tests/`: replace `mcp_integration_test.rs` with `common/mod.rs` plus one test file per handler module.
- Create: `docs/claude-desktop/{README.md,claude_desktop_config.snippet.json,glovebox-project-prompt.md}`.

---

### Task 1: Remove the web surface

**Files:**
- Delete: `frontend/`, `glovebox-backend/`, `TEST_PLAN.md`, `.claude/skills/new-e2e-test/`, `.claude/skills/new-entity/`, `git/`, `pull/`, `data/e2e.db`, `data/e2e.db-shm`, `data/e2e.db-wal`, `data/e2e-files/`
- Modify: `Cargo.toml`, `Justfile`, `.github/workflows/ci.yml`, `scripts/check-layering.sh`, `.claude/skills/preflight/SKILL.md`, `.gitignore`

**Interfaces:**
- Produces: a workspace with members `["glovebox-shared", "glovebox-mcp"]` where `just ci` runs `fmt-check`, `check-layering` and `ci-backend` only.

- [ ] **Step 1: Tag the pre-reset state.** Run `git tag pre-mcp-reset main && git push origin pre-mcp-reset`. Expected: the tag exists on the remote.
- [ ] **Step 2: Delete the paths listed above** with `git rm -r` (plain `rm -r` for the untracked ones).
- [ ] **Step 3: Edit `Cargo.toml`** so `members = ["glovebox-shared", "glovebox-mcp"]`. Remove workspace dependencies that no remaining crate uses (check with `cargo build` after removal; `clap`, `sha2` and `anyhow` are the likely ones).
- [ ] **Step 4: Edit `Justfile`.** Remove `test-e2e`, `test-e2e-ui`, `test-e2e-ci`, `dev`, `ci-frontend`. `ci` becomes `ci: fmt-check check-layering ci-backend`. Keep `reset-db`, `fmt`, `fmt-check`, `check-layering`, `ci-backend`.
- [ ] **Step 5: Edit `.github/workflows/ci.yml`.** Remove the frontend and e2e jobs and drop them from the "Verify required jobs" list.
- [ ] **Step 6: Rewrite `scripts/check-layering.sh`** to one rule: `glovebox-shared/src` contains no `axum` and no `rmcp` import. Exit non-zero with the offending lines otherwise.
- [ ] **Step 7: Update the preflight skill** to drop its frontend and Playwright steps.
- [ ] **Step 8: Verify.** Run `just ci`. Expected: all three recipes pass. `glovebox-mcp` still builds its HTTP router at this point because it depends on `axum` directly.
- [ ] **Step 9: Commit.** `git commit -m "chore: remove the web UI, HTTP API and e2e suite"`

### Task 2: Remove builds, visits and documents from the domain and the tools

**Files:**
- Delete: `glovebox-shared/src/services/{build,visit,document,dashboard,export,vin_decode}.rs`, `glovebox-shared/src/inputs/{build,visit,document}.rs`
- Modify: `glovebox-shared/src/services/mod.rs`, `glovebox-shared/src/inputs/mod.rs`, `glovebox-shared/src/services/{search,activity,costs,work_item,incident,part,service_record,research}.rs`, `glovebox-shared/src/test_support.rs`, `glovebox-shared/src/config.rs`, `glovebox-mcp/src/{handler,schemas}.rs`, `glovebox-mcp/tests/mcp_integration_test.rs`

**Interfaces:**
- Produces: `AppConfig { db_path: String }` only. The MCP tool list without `list_builds`, `get_build_progress`, `update_build_status`, `schedule_visit`, `complete_visit`, `cancel_visit`, `attach_document`, `find_documents`. `SearchScope` without its builds and documents variants. `work_item::VALID_STATUSES = ["planned", "done", "dropped"]`.

Entities and tables for the removed concepts stay until Task 3, so this task changes no schema.

- [ ] **Step 1: Write the failing test** in `glovebox-mcp/tests/mcp_integration_test.rs`, replacing `tools_list_advertises_the_full_verb_set`:

```rust
#[tokio::test]
async fn tools_list_has_no_build_visit_or_document_verbs() {
    let names = tool_names(&app).await; // existing helper pattern in this file
    for gone in ["list_builds", "get_build_progress", "update_build_status", "schedule_visit",
                 "complete_visit", "cancel_visit", "attach_document", "find_documents"] {
        assert!(!names.contains(&gone.to_string()), "{gone} must be removed");
    }
    for kept in ["record_service", "plan_work", "check_recalls", "log_incident"] {
        assert!(names.contains(&kept.to_string()), "{kept} must remain");
    }
}
```

- [ ] **Step 2: Run it.** `cargo test -p glovebox-mcp tools_list_has_no_build` — expected FAIL on `list_builds`.
- [ ] **Step 3: Remove the eight tools,** their input structs in `schemas.rs`, and every `build_id`, `visit_id`, deep-link and attach mention in the remaining tool descriptions and in the server instructions string. Remove `base_urls` from `GloveboxMcp` and `public_url`, `files_dir`, `inbox_dir`, `listen` from `AppConfig` (the router keeps a hardcoded port until Task 4 deletes it).
- [ ] **Step 4: Delete the service and input modules listed above.** Remove `build_id`/`visit_id` from every `New*`/`Update*` input and from the service fns that validate them (set the entity columns to `NotSet`/`None` until Task 3 removes them). Drop the `scheduled` work item status and the `visit::is_open` checks.
- [ ] **Step 5: Delete the tests that exercised removed code** in both crates, including `seed_linked_document` in `test_support.rs` and the build, visit, document, deep-link and attachment tests in the MCP integration test.
- [ ] **Step 6: Verify.** `just ci` passes.
- [ ] **Step 7: Commit.** `git commit -m "refactor: remove builds, visits and document storage"`

### Task 3: Squash the schema and open the database in one place

**Files:**
- Delete: all 22 files under `glovebox-shared/src/migration/` except `mod.rs`
- Create: `glovebox-shared/src/migration/m20261112_000001_initial_schema.rs`, `glovebox-shared/src/migration/m20261112_000002_seed_vw_mqb.rs`, `glovebox-shared/src/entities/service_recall_link.rs`, `glovebox-shared/src/db.rs`
- Delete: `glovebox-shared/src/entities/{build,visit,document,vehicle_attribute}.rs`
- Modify: `glovebox-shared/src/migration/mod.rs`, `glovebox-shared/src/entities/{mod,service_record,service_record_line_item,part,work_item,incident,incident_service_link,research_finding,vehicle}.rs`, `glovebox-shared/src/lib.rs`, `glovebox-shared/src/test_support.rs`, plus every service that names a removed or renamed column

**Interfaces:**
- Produces: `pub async fn glovebox_shared::db::connect(db_path: &str) -> DomainResult<DatabaseConnection>`, which creates the parent directory, opens SQLite with `create_if_missing`, WAL journal mode and `foreign_keys = ON`, and runs `Migrator::up`. `test_db()` uses the same options on an in-memory database.
- Produces: the entities in the "Final schema" table above, with these new fields: `service_record::Model { performed_by: String, end_date: Option<String> }`, `service_record_line_item::Model { work_kind: String }`, `part::Model { supplied_by: String }`, `work_item::Model { revisit_by_date: Option<String>, revisit_by_miles: Option<i32>, completed_service_id: Option<i32> }`, `incident_service_link::Model { role: String }`, `service_recall_link::Model { service_record_id: i32, research_finding_id: i32 }`.

To author the migration, dump the present schema once for reference (`sqlite3 "file:$PWD/data/glovebox.db?immutable=1" .schema`) and write the kept tables with the sea-query builder. Use raw `execute_unprepared` only for the FTS5 virtual tables and their trigger trios, copying the existing canonical pattern. The seed migration carries over the VW MQB platform, model templates and schedule items unchanged.

In this task the new columns get the minimum behavior needed to compile: services write `performed_by = "shop"`, `work_kind = "maintenance"`, `supplied_by = "self"` and `role = "resolved"` wherever they insert. Tasks 5–8 replace those placeholders with real inputs.

- [ ] **Step 1: Write the failing tests** in `glovebox-shared/src/db.rs`:

```rust
#[tokio::test]
async fn connect_creates_the_file_enables_foreign_keys_and_migrates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/glovebox.db");
    let db = connect(path.to_str().unwrap()).await.unwrap();
    let fk: i32 = scalar(&db, "PRAGMA foreign_keys").await;
    assert_eq!(fk, 1);
    let tables = table_names(&db).await;
    for t in ["vehicles", "service_records", "service_recall_links", "work_items"] {
        assert!(tables.contains(&t.to_string()), "missing {t}");
    }
    for t in ["builds", "visits", "documents", "vehicle_attributes"] {
        assert!(!tables.contains(&t.to_string()), "{t} must not exist");
    }
}

#[tokio::test]
async fn deleting_a_vehicle_cascades_to_its_service_records() {
    // Insert a vehicle and one service through the services, delete the
    // vehicle, then assert service_records has zero rows for that vehicle_id.
}

#[tokio::test]
async fn the_seed_gives_the_gti_template_a_spark_plug_item_at_60k_miles() {
    // schedule::resolve for a vehicle on the seeded GTI model template
    // contains an item named "Spark Plugs" with interval_miles == Some(60000).
}
```

Add `tempfile = "3"` to `glovebox-shared` dev-dependencies. Confirm the seeded item's exact name against the current seed migration before writing the third assertion.

- [ ] **Step 2: Run them.** `cargo test -p glovebox-shared db::` — expected FAIL, `connect` not defined.
- [ ] **Step 3: Write the two migrations, the entity changes and `db::connect`.** Update `migration/mod.rs` to list exactly the two new migrations.
- [ ] **Step 4: Fix every service and test that named a removed column.** Finding status tracking through `linked_entity_type`/`linked_entity_id` is removed here; Task 8 replaces it.
- [ ] **Step 5: Verify.** `just ci` passes. Then `just reset-db` and confirm `data/glovebox.db` no longer exists.
- [ ] **Step 6: Commit.** `git commit -m "refactor: squash migrations into one initial schema"`

### Task 4: Serve MCP over stdio

**Files:**
- Create: `glovebox-mcp/src/main.rs`, `glovebox-mcp/src/handler/{mod,vehicles,services,parts,incidents,planning,schedule,recalls,reads}.rs`, `glovebox-mcp/tests/common/mod.rs`, `glovebox-mcp/tests/{vehicles,services,parts,incidents,planning,schedule,recalls,reads,stdio}.rs`
- Delete: `glovebox-mcp/src/handler.rs`, `glovebox-mcp/tests/mcp_integration_test.rs`
- Modify: `glovebox-mcp/src/lib.rs`, `glovebox-mcp/Cargo.toml`, `Justfile`

**Interfaces:**
- Produces: binary `glovebox-mcp` reading `GLOVEBOX_DB_PATH` (default `data/glovebox.db`), calling `glovebox_shared::db::connect`, and serving `GloveboxMcp` on stdin/stdout.
- Produces: `pub fn GloveboxMcp::new(db: DatabaseConnection) -> Self`.
- Produces: test helper `pub async fn connect() -> TestClient` in `tests/common/mod.rs`, where `TestClient` wraps an rmcp client connected to a `GloveboxMcp` over `tokio::io::duplex` and exposes `db: DatabaseConnection`, `async fn call(&self, tool: &str, args: serde_json::Value) -> serde_json::Value` (panics on a tool-level error), `async fn call_err(&self, tool: &str, args: serde_json::Value) -> String` (returns the error text, panics on success) and `async fn tool_names(&self) -> Vec<String>`.
- Produces: `just install`, which runs `cargo build --release -p glovebox-mcp` and prints the absolute binary path and the Claude Desktop config path.

`Cargo.toml`: replace the rmcp feature `transport-streamable-http-server` with the stdio transport feature; add the rmcp client feature under dev-dependencies; remove `axum`, `if-addrs` and `tower`; add `tokio` and `tracing-subscriber` as dependencies. Splitting `handler.rs` is a move with no behavior change: each new file holds one `#[tool_router]`-compatible group of the existing tools, combined the way rmcp 2.2 documents for multi-file routers. Resources keep `glovebox://vehicles`, `glovebox://vehicles/{id}` and `glovebox://vehicles/{id}/activity`; the builds URI is gone.

- [ ] **Step 1: Read the rmcp 2.2 docs** for the stdio server transport, combining tool routers across impl blocks, and an in-process client over a duplex stream. Note the exact function and feature names in the commit message body.
- [ ] **Step 2: Write the failing tests.** In `tests/stdio.rs`:

```rust
#[tokio::test]
async fn the_binary_speaks_only_protocol_on_stdout() {
    // Spawn env!("CARGO_BIN_EXE_glovebox-mcp") with GLOVEBOX_DB_PATH in a
    // tempdir, write one initialize request line to stdin, read stdout
    // until the first newline, and close stdin.
    let v: serde_json::Value = serde_json::from_str(first_line.trim()).unwrap();
    assert_eq!(v["jsonrpc"], "2.0");
    assert!(v["result"]["serverInfo"]["name"].is_string());
    // Every further stdout line before exit must also parse as JSON.
}
```

In `tests/vehicles.rs`, port `list_vehicles_empty_then_after_seed` to `common::connect()`.

- [ ] **Step 3: Run them.** `cargo test -p glovebox-mcp --test stdio` — expected FAIL, the binary target does not exist.
- [ ] **Step 4: Write `main.rs`, the `common` harness and the handler split.** Initialize tracing with a stderr writer before anything else runs.
- [ ] **Step 5: Port the surviving integration tests** from the deleted file into the per-module test files using `TestClient`. Drop `tool_calls_survive_a_server_restart` and `rejects_unknown_host_header`, which test the HTTP transport.
- [ ] **Step 6: Verify.** `just ci` passes; `just install` prints a path to an existing executable.
- [ ] **Step 7: Commit.** `git commit -m "feat(mcp): serve over stdio for Claude Desktop"`

### Task 5: Service records carry invoice detail

**Files:**
- Modify: `glovebox-shared/src/inputs/{service_record,part}.rs`, `glovebox-shared/src/services/{service_record,part}.rs`, `glovebox-mcp/src/schemas.rs`, `glovebox-mcp/src/handler/{services,parts}.rs`
- Test: unit tests in the two service modules, `glovebox-mcp/tests/services.rs`

**Interfaces:**
- Produces, in `inputs::service_record`:
  - `NewLineItem { description: String, work_kind: String, quantity: Option<f64>, unit_cost_cents: Option<i32>, cost_cents: Option<i32> }`
  - `NewServiceRecord` gains `performed_by: Option<String>` (default `"shop"`), `end_date: Option<String>`, `parts: Option<Vec<NewPart>>`; `UpdateServiceRecord` gains `performed_by: Option<String>`, `end_date: Option<Option<String>>`.
- Produces, in `inputs::part`: `NewPart` gains `supplied_by: Option<String>` (default `"self"`); `UpdatePart` gains `supplied_by: Option<String>`.
- Produces: `ServiceRecordWithLinks` gains `already_recorded: bool` and `parts: Vec<part::Model>` (the full rows, replacing nothing; `part_ids` stays).
- Produces: `service_record::create` requires `C: ConnectionTrait + TransactionTrait` and commits the record, lines, inline parts, mileage reading and links in one transaction.

Rules: `performed_by` and `work_kind` and `supplied_by` are validated against their whitelists with `DomainError::Invalid { field, .. }`. `end_date`, when present, must not be before `service_date`. An inline part is created with `installed_service_id` set to the new record, `installed_date` set to `service_date`, `installed_odometer` set to the record's mileage and `status = "installed"`. `performed_by = "self"` with a `shop_name` is rejected as `Invalid { field: "shop_name" }`.

- [ ] **Step 1: Write the failing unit tests** in `services/service_record.rs`:

```rust
#[tokio::test]
async fn create_stores_who_did_it_the_date_range_and_line_kinds() {
    // service_date "2025-03-24", end_date "2025-03-25", performed_by "shop",
    // lines: ("Replace suction jet pump", "repair"), ("Intake upgrade", "mod")
    assert_eq!(got.record.performed_by, "shop");
    assert_eq!(got.record.end_date.as_deref(), Some("2025-03-25"));
    assert_eq!(kinds(&got), vec!["repair", "mod"]);
}

#[tokio::test]
async fn create_rejects_an_end_date_before_the_start() {
    // service_date "2025-03-25", end_date "2025-03-24"
    assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "end_date"));
}

#[tokio::test]
async fn create_rejects_an_unknown_work_kind_and_performer() {
    // work_kind "upgrade" -> Invalid field "work_kind"; performed_by "dealer" -> "performed_by"
}

#[tokio::test]
async fn inline_parts_are_installed_by_the_service_and_keep_their_supplier() {
    // parts: [{ name "Front brake pads", seller "FCP Euro", supplied_by "self" },
    //         { name "Brake fluid", supplied_by "shop" }], mileage 52747
    assert_eq!(got.parts.len(), 2);
    assert_eq!(got.parts[0].supplied_by, "self");
    assert_eq!(got.parts[0].installed_service_id, Some(got.record.id));
    assert_eq!(got.parts[0].installed_odometer, Some(52747));
    assert_eq!(got.parts[1].supplied_by, "shop");
}

#[tokio::test]
async fn refiling_an_invoice_number_returns_the_first_record_untouched() {
    // Create with invoice_ref "07447" and one line + one part; create again
    // with invoice_ref "07447" and two different lines + two parts.
    assert!(second.already_recorded);
    assert_eq!(second.record.id, first.record.id);
    assert_eq!(count_line_items(&db).await, 1);
    assert_eq!(count_parts(&db).await, 1);
}

#[tokio::test]
async fn a_bad_part_supplier_rolls_back_the_whole_service() {
    // One valid line, one part with supplied_by "dealer".
    assert!(create(..).await.is_err());
    assert_eq!(count_services(&db).await, 0);
    assert_eq!(count_mileage_readings(&db).await, 0);
}

#[tokio::test]
async fn a_self_performed_service_records_an_ecu_flash() {
    // performed_by "self", no shop_name, line ("IE Stage 1 tune flash", "mod")
    assert_eq!(got.record.performed_by, "self");
    assert!(got.record.shop_name.is_none());
}
```

- [ ] **Step 2: Run them.** `cargo test -p glovebox-shared service_record` — expected FAIL to compile (`work_kind` missing on `NewLineItem`).
- [ ] **Step 3: Implement the inputs, validation and transactional create and update.**
- [ ] **Step 4: Run the unit tests.** Expected PASS.
- [ ] **Step 5: Write the failing MCP test** in `tests/services.rs`:

```rust
#[tokio::test]
async fn record_service_files_a_multi_line_invoice_in_one_call() {
    let out = c.call("record_service", json!({
        "vehicle_id": v, "service_date": "2025-03-24", "end_date": "2025-03-25",
        "performed_by": "shop", "shop_name": "Example VW", "mileage": 53010,
        "invoice_ref": "RO-1182", "total_cost_cents": 48250,
        "line_items": [
            {"description": "Brake fluid flush", "work_kind": "maintenance", "cost_cents": 14900},
            {"description": "Install owner-supplied pads", "work_kind": "repair", "cost_cents": 33350}],
        "parts": [{"name": "Front brake pads", "seller": "FCP Euro", "supplied_by": "self",
                   "cost_cents": 8900}]
    })).await;
    assert_eq!(out["record"]["performed_by"], "shop");
    assert_eq!(out["parts"][0]["supplied_by"], "self");
    assert_eq!(out["already_recorded"], false);
}
```

- [ ] **Step 6: Update `RecordServiceInput`, `RecordPartInput` and the two tool descriptions.** The `record_service` description says: one call per invoice, `work_kind` per line, `parts` for anything installed, `invoice_ref` is the number printed on the paperwork.
- [ ] **Step 7: Verify.** `just ci` passes.
- [ ] **Step 8: Commit.** `git commit -m "feat: record who did the work, line kinds and part suppliers"`

### Task 6: To-do items with a revisit-by, completed by a service

**Files:**
- Modify: `glovebox-shared/src/inputs/{work_item,service_record}.rs`, `glovebox-shared/src/services/{work_item,service_record}.rs`, `glovebox-mcp/src/schemas.rs`, `glovebox-mcp/src/handler/{planning,services}.rs`
- Test: unit tests in `services/work_item.rs`, `glovebox-mcp/tests/planning.rs`

**Interfaces:**
- Produces: `NewWorkItem` and `UpdateWorkItem` gain `revisit_by_date` and `revisit_by_miles` (`Option<String>`/`Option<i32>` and their double-option forms).
- Produces: `NewServiceRecord.work_item_ids: Option<Vec<i32>>`.
- Produces: `pub(crate) async fn work_item::complete_with_service(db: &impl ConnectionTrait, vehicle_id: i32, ids: &[i32], service_record_id: i32) -> DomainResult<Vec<work_item::Model>>`, which sets `status = "done"` and `completed_service_id`, rejects an id from another vehicle with `NotFound`, and rejects an item that is already `done` or `dropped` with `BadRequest`.
- Produces: `pub fn work_item::revisit_status(item: &work_item::Model, today: chrono::NaiveDate, estimated_mileage: Option<i32>) -> Option<&'static str>` returning `None` when neither revisit field is set, else `"overdue"`, `"due_soon"` (within 30 days or 1000 miles) or `"ok"`. The worse of the date and mileage readings wins.

`service_record::create` calls `complete_with_service` inside its transaction and then, for each completed item, links the item's `schedule_item_id` to the service through the existing schedule link path so the reminder clears.

- [ ] **Step 1: Write the failing unit tests** in `services/work_item.rs`:

```rust
#[test]
fn revisit_status_takes_the_worse_of_date_and_mileage() {
    let today = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
    assert_eq!(revisit_status(&item(None, None), today, Some(64000)), None);
    assert_eq!(revisit_status(&item(Some("2026-10-01"), None), today, None), Some("overdue"));
    assert_eq!(revisit_status(&item(Some("2026-12-01"), None), today, None), Some("due_soon"));
    assert_eq!(revisit_status(&item(Some("2027-06-01"), None), today, None), Some("ok"));
    assert_eq!(revisit_status(&item(Some("2027-06-01"), Some(64500)), today, Some(64000)), Some("due_soon"));
    assert_eq!(revisit_status(&item(None, Some(60000)), today, Some(64000)), Some("overdue"));
    assert_eq!(revisit_status(&item(None, Some(60000)), today, None), Some("ok"));
}

#[tokio::test]
async fn a_service_completes_the_work_items_it_names_and_clears_their_reminders() {
    // Work item linked to an overdue 12-month schedule item; record a
    // service today with work_item_ids [item.id].
    assert_eq!(item_after.status, "done");
    assert_eq!(item_after.completed_service_id, Some(svc.record.id));
    assert_eq!(reminder_status(&db, v, "Brake fluid flush").await, "ok");
}

#[tokio::test]
async fn a_work_item_from_another_vehicle_is_not_found_and_nothing_commits() {
    let err = create(&db, v1, svc_with_work_items(vec![other_vehicle_item])).await.unwrap_err();
    assert_eq!(err.to_string(), nonexistent_item_error.to_string());
    assert_eq!(count_services(&db).await, 0);
}
```

- [ ] **Step 2: Run them.** `cargo test -p glovebox-shared work_item` — expected FAIL, `revisit_status` not defined.
- [ ] **Step 3: Implement** the inputs, `revisit_status`, `complete_with_service` and the call from `service_record::create`.
- [ ] **Step 4: Write the failing MCP test** in `tests/planning.rs`: `plan_work` with `revisit_by_date "2027-05-01"` and `revisit_by_miles 70000`, then `list_planned_work` returns the item with both fields and a `revisit_status` of `"ok"` at an estimated mileage of 64000; `record_service` with `work_item_ids` then removes it from the open list.
- [ ] **Step 5: Update `PlanWorkInput`, `RecordServiceInput`, `list_planned_work`'s payload and the descriptions.**
- [ ] **Step 6: Verify.** `just ci` passes.
- [ ] **Step 7: Commit.** `git commit -m "feat: revisit-by on planned work, completed by a service"`

### Task 7: Incidents link to the service that noticed them and the service that fixed them

**Files:**
- Modify: `glovebox-shared/src/inputs/{incident,service_record}.rs`, `glovebox-shared/src/services/{incident,service_record}.rs`, `glovebox-mcp/src/schemas.rs`, `glovebox-mcp/src/handler/{incidents,services,reads}.rs`
- Test: unit tests in `services/incident.rs` and `services/service_record.rs`, `glovebox-mcp/tests/incidents.rs`

**Interfaces:**
- Produces: `NewIncident.noticed_during_service_id: Option<i32>`; `NewServiceRecord.resolved_incident_ids: Option<Vec<i32>>` and `noticed_incident_ids: Option<Vec<i32>>`.
- Produces, in `services::incident`:
  - `pub struct ServiceRef { pub id: i32, pub service_date: String, pub description: Option<String> }`
  - `IncidentWithDetails` gains `noticed_during: Option<ServiceRef>` and `resolved_by: Vec<ServiceRef>`, replacing its untyped service link list.
  - `pub(crate) async fn link_service(db: &impl ConnectionTrait, vehicle_id: i32, incident_id: i32, service_record_id: i32, role: &str) -> DomainResult<()>`. Role `"resolved"` also sets `incident.resolved = true`. A second `"noticed"` link for the same incident is rejected with `BadRequest`.
- Produces, in `services::service_record`:
  - `pub struct IncidentRef { pub id: i32, pub title: String, pub category: String, pub resolved: bool, pub other_service: Option<incident::ServiceRef> }`
  - `ServiceRecordWithLinks` gains `noticed_incidents: Vec<IncidentRef>` (`other_service` is the resolving service, if any) and `resolved_incidents: Vec<IncidentRef>` (`other_service` is the service where it was noticed, if any).
- Produces: MCP tools `get_service { vehicle_id, service_id }` and `get_incident { vehicle_id, incident_id }` in `handler/reads.rs`.

A work item completed by a service (Task 6) that has an `incident_id` resolves that incident with the same service.

- [ ] **Step 1: Write the failing unit tests:**

```rust
#[tokio::test]
async fn a_shop_observation_links_the_service_that_noticed_it_and_the_one_that_fixed_it() {
    // 2023-05-03 service; incident category "shop_observation", title
    // "N80 purge valve failing", noticed_during_service_id = svc_2023.
    // 2024-06-12 service with resolved_incident_ids [incident.id].
    let s23 = service_record::get(&db, v, svc_2023).await.unwrap();
    assert_eq!(s23.noticed_incidents[0].title, "N80 purge valve failing");
    assert!(s23.noticed_incidents[0].resolved);
    assert_eq!(s23.noticed_incidents[0].other_service.as_ref().unwrap().service_date, "2024-06-12");
    let s24 = service_record::get(&db, v, svc_2024).await.unwrap();
    assert_eq!(s24.resolved_incidents[0].other_service.as_ref().unwrap().service_date, "2023-05-03");
    let inc = incident::get(&db, v, incident_id).await.unwrap();
    assert_eq!(inc.noticed_during.unwrap().id, svc_2023);
    assert_eq!(inc.resolved_by[0].id, svc_2024);
}

#[tokio::test]
async fn an_unresolved_observation_shows_as_open_on_the_service_that_noticed_it() {
    assert!(!s23.noticed_incidents[0].resolved);
    assert!(s23.noticed_incidents[0].other_service.is_none());
}

#[tokio::test]
async fn completing_an_incident_sourced_work_item_resolves_the_incident() { /* via work_item_ids */ }

#[tokio::test]
async fn incident_and_service_ids_from_another_vehicle_are_not_found() {
    // noticed_during_service_id from another vehicle on create;
    // resolved_incident_ids from another vehicle on record. Each error string
    // equals the nonexistent-id error, and no rows are written.
}
```

- [ ] **Step 2: Run them.** `cargo test -p glovebox-shared incident` — expected FAIL to compile.
- [ ] **Step 3: Implement** the role-aware links, the two detail shapes with batch loading (`is_in`, no per-row queries) and the `shop_observation` category.
- [ ] **Step 4: Write the failing MCP test** in `tests/incidents.rs`: drive the same 2023/2024 story through `record_service`, `log_incident` and `get_service`, asserting the same four facts on the JSON.
- [ ] **Step 5: Add `get_service` and `get_incident`; extend `LogIncidentInput` and `RecordServiceInput`.** `log_incident`'s description gains: use `shop_observation` with `noticed_during_service_id` for anything a shop flagged but did not fix, then `plan_work` the fix with a revisit-by.
- [ ] **Step 6: Verify.** `just ci` passes.
- [ ] **Step 7: Commit.** `git commit -m "feat: link incidents to the services that noticed and resolved them"`

### Task 8: Recalls carry a status and link to the work that addresses them

**Files:**
- Create: `glovebox-shared/src/services/recall.rs`
- Modify: `glovebox-shared/src/services/{mod,research,service_record,work_item}.rs`, `glovebox-shared/src/inputs/service_record.rs`, `glovebox-mcp/src/schemas.rs`, `glovebox-mcp/src/handler/{recalls,services}.rs`
- Test: unit tests in `services/recall.rs`, `glovebox-mcp/tests/recalls.rs`

**Interfaces:**
- Produces, in `services::recall`:
  - `pub struct RecallWithStatus { pub finding_id: i32, pub campaign_number: Option<String>, pub title: String, pub description: Option<String>, pub source_url: Option<String>, pub status: String, pub planned_work_item_id: Option<i32>, pub done_service: Option<incident::ServiceRef> }`
  - `pub async fn list(db: &impl ConnectionTrait, vehicle_id: i32) -> DomainResult<Vec<RecallWithStatus>>`
  - `pub async fn set_status(db: &impl ConnectionTrait, vehicle_id: i32, finding_id: i32, status: &str) -> DomainResult<RecallWithStatus>` accepting `"open"`, `"done"`, `"not_applicable"`.
  - `pub(crate) async fn link_service(db: &impl ConnectionTrait, vehicle_id: i32, finding_ids: &[i32], service_record_id: i32) -> DomainResult<()>`
- Produces: `NewServiceRecord.recall_finding_ids: Option<Vec<i32>>`.
- Produces: `research::check_recalls` returns `RecallCheck { newly_found: usize, recalls: Vec<RecallWithStatus>, message: Option<String> }`.
- Produces: MCP tool `set_recall_status { vehicle_id, finding_id, status }`.

Status derivation, first match wins: a `service_recall_links` row or stored finding status `completed` → `done`; stored status `dismissed` → `not_applicable`; a `planned` work item with this `research_finding_id` → `planned`; otherwise `open`. `set_status` maps `done` → stored `completed`, `not_applicable` → `dismissed`, `open` → `new`. `campaign_number` is parsed from the `nhtsaId=` query value in `source_url`. Only findings with `category = "recall"` are recalls; any other finding id passed to these fns is `NotFound`. Completing a recall-sourced work item through a service (Task 6) links that recall to the service.

Split `research::check_recalls` so the NHTSA fetch is injectable: `pub(crate) async fn record_recalls(db, vehicle_id, found: &[RecallInfo]) -> DomainResult<RecallCheck>` does the persistence and status listing and is what the tests call.

- [ ] **Step 1: Write the failing unit tests** in `services/recall.rs`:

```rust
#[tokio::test]
async fn a_recall_moves_from_open_to_planned_to_done() {
    let found = [recall_info("24V110000", "Suction Jet Pump")];
    let check = research::record_recalls(&db, v, &found).await.unwrap();
    assert_eq!((check.newly_found, check.recalls[0].status.as_str()), (1, "open"));
    assert_eq!(check.recalls[0].campaign_number.as_deref(), Some("24V110000"));

    let item = plan_work_for_recall(&db, v, check.recalls[0].finding_id).await;
    let r = &list(&db, v).await.unwrap()[0];
    assert_eq!((r.status.as_str(), r.planned_work_item_id), ("planned", Some(item.id)));

    let svc = record_service_with_recalls(&db, v, "2025-03-25", vec![r.finding_id]).await;
    let r = &list(&db, v).await.unwrap()[0];
    assert_eq!(r.status, "done");
    assert_eq!(r.done_service.as_ref().unwrap().service_date, "2025-03-25");
    assert_eq!(r.done_service.as_ref().unwrap().id, svc.record.id);
}

#[tokio::test]
async fn checking_again_reports_known_recalls_with_their_status_and_zero_new() {
    // record_recalls twice with the same campaign after marking it done.
    assert_eq!(second.newly_found, 0);
    assert_eq!(second.recalls.len(), 1);
    assert_eq!(second.recalls[0].status, "done");
}

#[tokio::test]
async fn a_recall_can_be_marked_done_or_not_applicable_without_a_service() {
    assert_eq!(set_status(&db, v, id, "not_applicable").await.unwrap().status, "not_applicable");
    assert_eq!(set_status(&db, v, id, "done").await.unwrap().status, "done");
    assert_eq!(set_status(&db, v, id, "open").await.unwrap().status, "open");
    assert!(set_status(&db, v, id, "closed").await.is_err());
}

#[tokio::test]
async fn a_recall_from_another_vehicle_or_a_non_recall_finding_is_not_found() {
    // On set_status and on recall_finding_ids in record_service; the service
    // is not created.
}
```

- [ ] **Step 2: Run them.** `cargo test -p glovebox-shared recall` — expected FAIL, module not defined.
- [ ] **Step 3: Implement `recall.rs`, the `record_recalls` split and the `record_service` link.**
- [ ] **Step 4: Write the failing MCP test** in `tests/recalls.rs`: seed one recall finding directly in `c.db`, call `record_service` with `recall_finding_ids`, then assert `set_recall_status` and a `recall::list`-backed read both report `done` with the service date. Do not call `check_recalls` in tests; it makes a live request.
- [ ] **Step 5: Add `set_recall_status`, extend `RecordServiceInput`, and rewrite the `check_recalls` description:** it returns every known recall with its status; NHTSA is queried by make, model and year, so it cannot know whether a recall was performed on this car; link a service to a recall when an invoice names a campaign number.
- [ ] **Step 6: Verify.** `just ci` passes.
- [ ] **Step 7: Commit.** `git commit -m "feat: recall status linked to planned work and services"`

### Task 9: One call answers "what's coming"

**Files:**
- Create: `glovebox-shared/src/services/upcoming.rs`
- Modify: `glovebox-shared/src/services/mod.rs`, `glovebox-mcp/src/schemas.rs`, `glovebox-mcp/src/handler/planning.rs`
- Test: unit tests in `services/upcoming.rs`, `glovebox-mcp/tests/planning.rs`

**Interfaces:**
- Consumes: `reminders::calculate_reminders`, `budget::forecast_from`, `work_item::revisit_status`, `recall::list`, `incident::list`.
- Produces:

```rust
pub struct PlannedWork {
    pub item: work_item::Model,
    pub revisit_status: Option<String>,
}
pub struct Upcoming {
    #[serde(flatten)]
    pub reminders: RemindersResponse,
    pub budget: BudgetForecast,
    pub planned_work: Vec<PlannedWork>,
    pub planned_work_est_cost_cents: i32,
    pub recalls: Vec<RecallWithStatus>,       // status open or planned only
    pub open_incidents: Vec<incident::Model>, // resolved == false, category != "note"
}
pub async fn upcoming(db: &impl ConnectionTrait, vehicle_id: i32) -> DomainResult<Upcoming>
```

- Produces: MCP tool `whats_coming { vehicle_id }`, replacing `check_due_maintenance`.

`planned_work` holds `planned` items only, sorted overdue first, then due soon, then ok, then items with no revisit status. `planned_work_est_cost_cents` sums their `est_cost_cents`, treating missing values as zero.

- [ ] **Step 1: Write the failing unit tests** in `services/upcoming.rs`:

```rust
#[tokio::test]
async fn upcoming_merges_schedule_planned_work_recalls_and_open_incidents() {
    // GTI on the seeded template, purchased 2017-03-05 at 40 miles, one
    // mileage reading of 64000 today. One planned item "Replace N80 valve"
    // with revisit_by_miles 63000 and est_cost_cents 18000, one planned item
    // "Rear wiper" with no revisit and est 2500, one done item, one open
    // recall, one done recall, one unresolved shop_observation, one note.
    let u = upcoming(&db, v).await.unwrap();
    assert!(u.reminders.reminders.iter().any(|r| r.schedule_item.name == "Spark Plugs" && r.status == "overdue"));
    assert_eq!(titles(&u.planned_work), vec!["Replace N80 valve", "Rear wiper"]);
    assert_eq!(u.planned_work[0].revisit_status.as_deref(), Some("overdue"));
    assert_eq!(u.planned_work[1].revisit_status, None);
    assert_eq!(u.planned_work_est_cost_cents, 20500);
    assert_eq!(u.recalls.len(), 1);
    assert_eq!(u.open_incidents.len(), 1);
}

#[tokio::test]
async fn upcoming_works_for_a_bare_vehicle() {
    // No model template, no mileage readings, no purchase date, no records.
    let u = upcoming(&db, v).await.unwrap();
    assert!(u.reminders.reminders.is_empty());
    assert!(u.planned_work.is_empty() && u.recalls.is_empty() && u.open_incidents.is_empty());
    assert_eq!(u.planned_work_est_cost_cents, 0);
}

#[tokio::test]
async fn a_dated_revisit_is_judged_without_any_mileage() {
    // Bare vehicle, one planned item with revisit_by_date in the past.
    assert_eq!(u.planned_work[0].revisit_status.as_deref(), Some("overdue"));
}

#[tokio::test]
async fn upcoming_for_an_unknown_vehicle_is_not_found() {}
```

If `calculate_reminders` errors on a vehicle with no mileage data, fix it there so the second test passes; an empty or estimate-free result is the expected behavior.

- [ ] **Step 2: Run them.** `cargo test -p glovebox-shared upcoming` — expected FAIL, module not defined.
- [ ] **Step 3: Implement `upcoming`.**
- [ ] **Step 4: Write the failing MCP test** in `tests/planning.rs`: `whats_coming` returns the keys `reminders`, `budget`, `planned_work`, `recalls`, `open_incidents`, and `tool_names()` no longer contains `check_due_maintenance`.
- [ ] **Step 5: Replace the tool.** Its description: call this first for any "what does the car need" question; it covers scheduled maintenance, planned work, open recalls and unresolved incidents in one answer; group overdue and due-soon items into one shop trip when advising.
- [ ] **Step 6: Verify.** `just ci` passes.
- [ ] **Step 7: Commit.** `git commit -m "feat: whats_coming merges schedule, plans, recalls and incidents"`

### Task 10: Every record can be created, read, corrected and deleted through tools

**Files:**
- Modify: `glovebox-shared/src/services/{service_record,work_item,incident,part,schedule,model_template}.rs`, `glovebox-mcp/src/schemas.rs`, `glovebox-mcp/src/handler/{vehicles,services,parts,incidents,planning,schedule,reads}.rs`
- Test: unit tests in `services/service_record.rs`, every file under `glovebox-mcp/tests/`

**Interfaces:**
- Produces these MCP tools. Each maps to the named existing service fn unless marked new.

| Tool | Input | Service fn |
|---|---|---|
| `add_vehicle` | `NewVehicle` fields incl. optional `model_template_id` | `vehicle::create` |
| `update_vehicle` | `vehicle_id` + `UpdateVehicle` fields | `vehicle::update` |
| `list_model_templates` | none | `model_template::list` with each template's platform name |
| `list_services` | `vehicle_id`, optional `from`, `to` | `service_record::list` |
| `correct_service` | `vehicle_id`, `service_id` + `UpdateServiceRecord` fields | `service_record::update` |
| `delete_service` | `vehicle_id`, `service_id` | `service_record::delete` |
| `list_parts` | `vehicle_id` | `part::list` |
| `correct_part` | `vehicle_id`, `part_id` + `UpdatePart` fields | `part::update` |
| `delete_part` | `vehicle_id`, `part_id` | `part::delete` |
| `list_incidents` | `vehicle_id`, optional `resolved` | `incident::list` |
| `update_incident` | `vehicle_id`, `incident_id` + `UpdateIncident` fields | `incident::update` |
| `add_incident_followup` | `vehicle_id`, `incident_id`, `occurred_at`, `summary`, `notes` | `incident::create_followup` |
| `delete_incident` | `vehicle_id`, `incident_id` | `incident::delete` |
| `update_planned_work` | `vehicle_id`, `work_item_id` + `UpdateWorkItem` fields incl. `status` | `work_item::update` |
| `delete_planned_work` | `vehicle_id`, `work_item_id` | `work_item::delete` |
| `list_schedule` | `vehicle_id` | `schedule::resolve` |
| `add_schedule_item` | `vehicle_id`, `name`, `interval_miles`, `interval_months`, `est_cost_cents`, `source`, `notes`, `is_factory_recommended` | `schedule::create` scoped to the vehicle |
| `update_schedule_item` | `vehicle_id`, `schedule_item_id` + the same fields and `enabled` | `schedule::update`, vehicle-scoped items only |

- Produces: `service_record::delete` reopens what the service closed, inside one transaction: work items with `completed_service_id` equal to the service return to `planned`; each incident whose only `resolved` link was this service gets `resolved = false`. Recall links and incident links go by `ON DELETE CASCADE`.

Double-option fields in tool inputs use the existing lenient pattern in `schemas.rs`: an explicit JSON `null` clears the field and an absent key leaves it alone. `update_schedule_item` on a platform or template item returns `BadRequest` telling the caller to use `dismiss_schedule_item` or add a vehicle-specific item. The existing `link_service_to_maintenance`, `dismiss_schedule_item`, `log_mileage`, `save_note`, `file_research_finding`, `search_records`, `cost_summary` and `summarize_recent_activity` tools stay.

- [ ] **Step 1: Write the failing unit test** in `services/service_record.rs`:

```rust
#[tokio::test]
async fn deleting_a_service_reopens_what_it_closed() {
    // One service that completed a work item, resolved incident A (its only
    // resolver), resolved incident B (also resolved by an earlier service),
    // and performed one recall. Delete it.
    assert_eq!(work_item_after.status, "planned");
    assert_eq!(work_item_after.completed_service_id, None);
    assert!(!incident_a_after.resolved);
    assert!(incident_b_after.resolved);
    assert_eq!(recall::list(&db, v).await.unwrap()[0].status, "open");
}
```

- [ ] **Step 2: Run it.** Expected FAIL on the work item status.
- [ ] **Step 3: Implement the reopen logic in `service_record::delete`.**
- [ ] **Step 4: Write the failing MCP tests,** one round trip per tool group, each also asserting a wrong-vehicle id returns the same error text as a nonexistent id:
  - `vehicles.rs`: `add_vehicle` with a seeded `model_template_id` from `list_model_templates`, then `list_schedule` is non-empty; `update_vehicle` sets `license_plate` and clears `notes` with `null`.
  - `services.rs`: `correct_service` changes `total_cost_cents` from 48250 to 48520 and replaces the line items; `delete_service` then makes `get_service` an error.
  - `parts.rs`, `incidents.rs`, `planning.rs`: correct then delete; `update_planned_work` with `status: "dropped"` removes the item from `whats_coming`.
  - `schedule.rs`: `add_schedule_item` named "Smog check" with `interval_months: 24` appears in `whats_coming`; `update_schedule_item` sets `est_cost_cents: 5800`; updating a seeded platform item is a tool error mentioning `dismiss_schedule_item`.
  - `reads.rs`: `tool_names()` equals the exact expected set of tools, sorted.
- [ ] **Step 5: Run them.** Expected FAIL, tools not found.
- [ ] **Step 6: Add the tools and their schemas.** Rewrite the server instructions string to describe three loops: file an invoice, capture an advisory, answer "what's coming".
- [ ] **Step 7: Verify.** `just ci` passes.
- [ ] **Step 8: Commit.** `git commit -m "feat(mcp): create, read, correct and delete for every record"`

### Task 11: Claude Desktop install, project prompt, rehearsal and housekeeping

**Files:**
- Create: `docs/claude-desktop/README.md`, `docs/claude-desktop/claude_desktop_config.snippet.json`, `docs/claude-desktop/glovebox-project-prompt.md`, `glovebox-mcp/tests/rehearsal.rs`
- Modify: `CLAUDE.md`, `AGENTS.md`, `BRIEF.md`, `.claude/skills/new-migration/SKILL.md`, `glovebox-mcp/src/lib.rs` (crate docs)

**Interfaces:**
- Consumes: every tool from Tasks 5–10 and `just install` from Task 4.

The config snippet, modeled on `../financier/docs/claude-desktop/claude_desktop_config.snippet.json`:

```json
{
  "mcpServers": {
    "glovebox": {
      "command": "/Users/sloveless/Development/projects/glovebox/target/release/glovebox-mcp",
      "env": { "GLOVEBOX_DB_PATH": "/Users/sloveless/Development/projects/glovebox/data/glovebox.db" }
    }
  }
}
```

The project prompt states, as instructions to Claude in Desktop:

- **Filing an invoice.** Read the PDF. Make one `record_service` call with the dates, who did it, mileage, the printed invoice number as `invoice_ref`, every line with its `work_kind`, and every part with `supplied_by`. Ask who supplied a part when the invoice does not make it clear. In the same call, name the schedule items, planned work, recalls and incidents the work satisfies; call `whats_coming` and `check_recalls` first to get their ids. Match any campaign number on the invoice (NHTSA or manufacturer) against known recalls.
- **Capturing an advisory.** For anything the shop flagged but did not fix, call `log_incident` with category `shop_observation` and `noticed_during_service_id`, then `plan_work` linked to that incident with the shop's timeframe as a revisit-by.
- **Answering "what's coming".** Call `whats_coming` first. Present overdue and due-soon items grouped into one shop trip, with open recalls and planned work that fit the same trip. Research typical cost and parts worth supplying; save durable results with `file_research_finding` and `update_schedule_item` (`est_cost_cents`). Say when a cost is an estimate.
- **Fixing mistakes.** Use the `correct_*` and `delete_*` tools; never record a second service to patch a first.
- Money is integer cents. Dates are `YYYY-MM-DD`.

- [ ] **Step 1: Write the rehearsal test** in `glovebox-mcp/tests/rehearsal.rs`, tools only, no direct DB writes:

```rust
#[tokio::test]
async fn a_new_owner_can_set_up_file_an_invoice_and_ask_whats_coming() {
    // 1. list_model_templates -> pick the GTI template; add_vehicle (2017 VW
    //    Golf GTI, purchase_date 2017-03-05, purchase_mileage 40).
    // 2. plan_work "Replace front pads".
    // 3. record_service: 2026-11-10..2026-11-12, shop, mileage 64000,
    //    invoice_ref "RO-2291", three lines (maintenance, repair, mod), one
    //    self-supplied part from "FCP Euro" and one shop-supplied part,
    //    work_item_ids [pads], schedule_item_ids [spark plugs item].
    // 4. log_incident shop_observation "Water pump weeping" noticed during
    //    that service; plan_work for it with revisit_by_miles 70000.
    // 5. whats_coming.
    assert_eq!(status_of(&coming, "Spark Plugs"), "ok");
    assert_eq!(titles(&coming["planned_work"]), vec!["Replace water pump"]);
    assert_eq!(coming["planned_work"][0]["revisit_status"], "ok");
    assert_eq!(coming["open_incidents"][0]["title"], "Water pump weeping");
    // 6. get_service shows the weeping pump under noticed_incidents, two
    //    parts with suppliers "self" and "shop", and three line kinds.
    // 7. record_service again with invoice_ref "RO-2291" -> already_recorded.
}
```

- [ ] **Step 2: Run it.** `cargo test -p glovebox-mcp --test rehearsal`. Expected PASS if Tasks 5–10 are complete; any failure is a gap to fix in the owning service, with a unit test added there.
- [ ] **Step 3: Write the three `docs/claude-desktop/` files.** The README covers `just install`, merging the snippet into `~/Library/Application Support/Claude/claude_desktop_config.json`, restarting Desktop, creating a project with the prompt, and connecting NotePlan's own MCP server separately.
- [ ] **Step 4: Rewrite `CLAUDE.md` and mirror it to `AGENTS.md`** for the two-crate layout: commands, environment (`GLOVEBOX_DB_PATH` only), architecture, conventions that still apply, the tool list by handler module. Remove every frontend, Playwright, HTTP, router, deep-link and `TEST_PLAN.md` section. Update `BRIEF.md` to the spec's intent. Update the `new-migration` skill for the squashed baseline.
- [ ] **Step 5: Install and smoke-test by hand.** Run `just install`, then pipe one `initialize` request into the release binary with `GLOVEBOX_DB_PATH` pointing at the scratchpad and confirm a JSON-RPC response on stdout.
- [ ] **Step 6: Triage beads,** serialized in one session. Close as obsolete, each with `--reason="Removed by the MCP-first reset (docs/superpowers/specs/2026-11-12-mcp-first-reset-design.md)"`: `1dco`, `w57q`, `5toj`, `f3dv`, `lwq6`, `z8c0`, `3dyj`, `8ru0`, `cgrq`, `sawg`, `870l`, `6vln`, `j316`, `m53`, `x3gz`, `vrgv`, `brm2`. Close `urf1` with a reason citing the `db::connect` foreign-key test. Rewrite `y064` to drop its build half. Leave `2mh3`, `nd5i`, `06ek`, `666f`, `cp69`, `fbss`, `om74`, `leqa`, `toh` open. File one new bead: "Named milestone services (VW 10k–60k checklists as first-class objects)", P3, referencing the spec. Run `bd show` on each id before closing to confirm it matches this description.
- [ ] **Step 7: Verify.** `just ci` passes.
- [ ] **Step 8: Commit.** `git commit -m "docs: Claude Desktop setup, project prompt and two-crate CLAUDE.md"`

After Task 11 the builder's work is done. The owner's first real use (spec stage 3) follows: install into Claude Desktop, add the GTI, file the scanned invoices, ask the target question, and file a bead for each shortfall.
