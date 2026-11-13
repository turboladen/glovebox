# Agent Instructions

This project uses **bd** (beads) for issue tracking. Run `bd onboard` to get started.

## What Is This

Car maintenance tracker, used only through Claude Desktop. A Rust cargo workspace with two crates:
`glovebox-shared` (the domain library: SeaORM 2.0 + SQLite) and `glovebox-mcp` (an rmcp stdio
server with domain-verb tools and read-only `glovebox://` resources). Claude Desktop launches the
`glovebox-mcp` binary itself, so there is no server to keep running and no web UI. The question
the app answers: given what has been done to the car, its mileage and its age, what maintenance is
coming, when, how much will it cost, and what parts are worth buying and bringing to the shop.
The design is in `docs/superpowers/specs/2026-11-12-mcp-first-reset-design.md`, and the Desktop
setup and project prompt are in `docs/claude-desktop/`.

## Commands

```bash
just install              # Build target/release/glovebox-mcp and print the Claude Desktop config path
cargo build --workspace
cargo test --workspace    # Unit tests live in glovebox-shared services; MCP tests in glovebox-mcp/tests
cargo test -p glovebox-shared test_name           # One service-level test
cargo test -p glovebox-mcp --test rehearsal       # One MCP integration test file

just ci                   # Everything CI runs: nightly fmt-check, layering, build, test, clippy
just fmt                  # cargo +nightly fmt — rustfmt.toml uses nightly-only options; plain `cargo fmt` passes locally but CI's format job will fail
just check-layering       # Layering gate only (scripts/check-layering.sh)
just reset-db             # Delete data/glovebox.db; the next start recreates it

cargo clippy --workspace -- -D clippy::pedantic   # Rust lints (pedantic is the CI gate)

# Merging PRs: gate on parsed `gh pr checks` output showing zero pending/failed —
# `--watch` exits 0 when it races a run that hasn't registered yet
```

## Environment

`GLOVEBOX_DB_PATH` (default `data/glovebox.db`) is the only variable the server reads. `RUST_LOG`
sets the tracing filter (default `info`). The Claude Desktop config snippet sets an absolute
`GLOVEBOX_DB_PATH`.

## Architecture

### Workspace layout

- **`glovebox-shared/`** (lib crate `glovebox_shared`) — the transport-agnostic domain:
  `src/{db.rs,entities,migration,services,inputs,error.rs,test_support.rs}`. All SQL, validation
  and business logic live here. It never depends on axum or rmcp.
- **`glovebox-mcp/`** (lib crate `glovebox_mcp` plus the `glovebox-mcp` binary) — the thin MCP
  surface. Every tool body is argument struct → shared-service call → serialize, with one
  `DomainError → MCP` mapping helper. `src/main.rs` serves `GloveboxMcp` over stdio.
- `scripts/check-layering.sh` (run by CI and `just ci`) keeps axum and rmcp out of `glovebox-shared`.

### Domain library (`glovebox-shared/src/`)

**`db.rs`**: `db::connect(path)` is the only place a SQLite connection is opened. It creates the
parent directory, uses WAL mode with foreign keys enforced, and runs `Migrator::up()`.

**Entities** (`entities/`): Hand-written `DeriveEntityModel` structs. Parent entities declare
`has_many` relations; junction tables have `via()` impls.

**Inputs** (`inputs/`): Plain domain input structs (`New*`/`Update*`) consumed by service
functions.

**Services** (`services/`): One module per domain (`vehicle`, `service_record`, `part`, `incident`,
`work_item`, `schedule`, `recall`, `research`, ...) of free functions taking `db: &impl
ConnectionTrait` first and returning `DomainResult<T>`. Composite reads: `upcoming` (the
`whats_coming` answer: reminders, budget, planned work, recalls, open incidents), `reminders`,
`budget`, `costs`, `activity`, `search`. `vin_decode` and `nhtsa` make the only network calls.

**Errors** (`error.rs`): `DomainError::{NotFound, Invalid{field,message}, BadRequest, Db, Internal}`
with the `DomainResult<T>` alias.

**Migrations** (`migration/`): one squashed schema migration (`m20261112_000001_initial_schema`)
and one seed migration (`m20261112_000002_seed_vw_mqb`: the VW MQB platform, the Golf GTI and
Alltrack templates and their schedule items). They run on startup.

**Test harness** (`test_support.rs`): `test_db()` is an in-memory SQLite database with migrations
applied. It is compiled under `#[cfg(any(test, feature = "test-support"))]`; `glovebox-mcp` uses it
through a dev-dependency with the `test-support` feature.

### MCP surface (`glovebox-mcp/`)

**Transport:** stdio only. **Stdout carries only protocol frames**, so every diagnostic goes to
stderr (`tracing_subscriber` is installed with a stderr writer before anything else runs).
`tests/stdio.rs` spawns the binary and fails if stdout carries anything but JSON-RPC frames. Never
add a `println!` to this crate or to code it calls.

**Tools** (`src/handler/*.rs`, one `#[tool_router]` impl per area, combined in `handler/mod.rs`),
named as things a person would say:

- `vehicles.rs`: `list_vehicles`, `get_vehicle`, `add_vehicle`, `update_vehicle`, `delete_vehicle`,
  `log_mileage`, `list_mileage`, `delete_mileage`, `list_model_templates`
- `services.rs`: `record_service`, `link_service_to_maintenance`, `list_services`,
  `correct_service`, `delete_service`
- `parts.rs`: `record_part`, `list_parts`, `correct_part`, `delete_part`
- `incidents.rs`: `log_incident`, `save_note`, `list_incidents`, `update_incident`,
  `add_incident_followup`, `delete_incident`
- `planning.rs`: `plan_work`, `list_planned_work`, `whats_coming`, `update_planned_work`,
  `delete_planned_work`
- `schedule.rs`: `list_schedule`, `add_schedule_item`, `update_schedule_item`,
  `dismiss_schedule_item`, `restore_schedule_item`
- `recalls.rs`: `check_recalls`, `list_recalls`, `set_recall_status`, `file_research_finding`
- `reads.rs`: `summarize_recent_activity`, `search_records`, `cost_summary`, `get_service`,
  `get_incident`

Inputs are schemars-derived structs in `src/schemas.rs` and `src/schemas/corrections.rs`; their doc
comments become the LLM-visible field descriptions. `Parameters<T>` defers deserialize errors
so malformed arguments come back as actionable tool errors, not bare JSON-RPC `-32602`s. The
wrapper is named `Parameters` so the `#[tool]` macro derives the input schema from it; do not add
an `input_schema` attribute. The wrapper's `JsonSchema` impl inlines every subschema, because Claude
Desktop does not resolve `$ref` (it hides the nested fields from the model and sends the object as
a JSON string); the tools-list test fails if a reference survives. A `Parameters` type must not
refer to itself. A `correct_*` or
`update_*` input uses `Option<Option<T>>`: an absent key leaves the field alone, and an explicit
`null` clears it. Every input struct, nested ones included, carries
`#[serde(deny_unknown_fields)]`, so an argument the tool does not define is a tool error that names
it. Serde does not support it together with `#[serde(flatten)]`, so input structs never flatten.

**`record_service` is transactional.** One call writes the service, its lines, its parts and every
link (schedule items, work items, recalls, incidents) in one transaction. One bad id fails the call
and commits nothing. Re-recording an existing `invoice_ref` for the vehicle returns the stored
record with `already_recorded: true` and writes nothing. Completing planned work happens by naming
`work_item_ids` in `record_service`.

**Resources**: `glovebox://vehicles`, `glovebox://vehicles/{id}` and `…/{id}/activity`.
`list_resources` enumerates concrete URIs from the database and `read_resource` parses them.

**Error mapping**: one helper (`domain_error`) used by every tool. `NotFound`, `Invalid` and
`BadRequest` become tool-level error results whose message reaches the LLM; `Db` and `Internal`
become opaque JSON-RPC internal errors with the detail kept in tracing.

**Tests** (`glovebox-mcp/tests/`): `common/mod.rs` is the harness (an in-process client over an
in-memory duplex stream and an in-memory database). One file per handler area (`vehicles`,
`services`, `parts`, `incidents`, `planning`, `schedule`, `recalls`, `reads`), plus `stdio.rs`
(the binary's stdout) and `rehearsal.rs` (a new owner's whole first session through tool calls
only). Add coverage for a new tool to its area's file.

## Conventions

### Layering (enforced by `scripts/check-layering.sh` in CI)

- Domain inputs, validation and SQL live in `glovebox-shared::{inputs,services}`. MCP handlers only
  map tool arguments to domain inputs and rely on `domain_error` for failures. No SeaORM queries,
  `ActiveModel`s or business logic in `glovebox-mcp/src/handler/`.
- `glovebox-shared` never imports axum or rmcp.
- Service functions take `db: &impl ConnectionTrait` as the first parameter, never a server type.
- **Error model**: services return `DomainResult<T>`. `Invalid` renders as `"{field}: {message}"`;
  `BadRequest` renders its message verbatim; `Db` and `Internal` are opaque to the client.
- **`updated_at` stamping lives in shared service update functions**, not handlers: every update
  function sets `active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string())`
  because SeaORM `ActiveModel::update()` does not set it.
- New domain logic gets service-level unit tests in `glovebox-shared` (via `test_support::test_db()`)
  and MCP coverage in `glovebox-mcp/tests`.
- **Cross-reference guards**: services guard every foreign id they accept (schedule items, work
  items, incidents, recall findings, services, parts). MCP calls services directly with no
  pre-checks. A wrong-parent id must produce the byte-identical `NotFound` that a nonexistent id
  produces, so there is no ownership oracle, and each link has a wrong-parent regression test.
- **Future feature to leave room for**: named milestone services (VW's 10k to 60k checklists as
  first-class objects) are wanted. Maintenance is tracked per schedule item on its own interval, and
  Claude groups overdue items into "your 60k service" when answering. Nothing should make named
  milestones harder to add, such as collapsing schedule items into a single list or hard-coding the
  grouping.

### General

- **Issue tracking**: Use `bd` (beads), never markdown TODOs.
- **Currency**: Stored as integer cents (`i32`). Tool inputs and outputs are cents. Use integer
  division for formatting, not `as f64 / 100.0`.
- **DB datetimes**: `String` at the SeaORM boundary (SQLite TEXT). Dates are `YYYY-MM-DD`.
- **Migrations**: new migrations go after `m20261112_000002_seed_vw_mqb`; the `new-migration` skill
  has the templates. Use `Expr::cust()` for expressions inside builder queries; raw
  `execute_unprepared` only where no builder form exists (FTS5 DDL, cross-table copies).
- **Migrations are not transactional on SQLite** (sea-orm-migration only wraps Postgres), so write
  them rerun-safe: `IF NOT EXISTS` DDL, `INSERT OR IGNORE` or `NOT EXISTS`-gated DML,
  pragma-guarded ALTERs.
- **SQLite cannot `DROP COLUMN` named in an FK clause**; use the table-rebuild pattern. For FTS5,
  copy the external-content trigger trio in the initial schema migration; aggregating over `bm25()`
  arms needs a `MATERIALIZED` CTE because query flattening breaks `bm25`.
- **Verify data-moving migrations against a populated copy of `data/glovebox.db`**, not just
  `test_db()`.
- **`tokio::fs::File` buffers**: always `.flush().await` before the handle drops, or a pending write
  can be silently lost under load.
- **Entity field order**: must match physical DB column order. ALTER TABLE appends columns to the
  end, so new fields go after `created_at`/`updated_at` in the entity struct.
- **N+1 queries**: List functions that load related data must batch with `is_in()` queries, not
  query per record in a loop.
- **Imports**: Use `sea_orm::*` glob imports. Explicit imports create unused-import churn.

## Clippy

Crate-level `#![allow]`s in `glovebox-shared/src/lib.rs` suppress intentional pedantic lints:
- `clippy::option_option`: the double-option convention for update inputs
- `clippy::struct_field_names`: entity fields map to DB column names
- `clippy::wildcard_imports`: `sea_orm::*` is idiomatic
- `clippy::missing_errors_doc`, `clippy::missing_panics_doc`, `clippy::must_use_candidate` and
  `clippy::implicit_hasher`: these target public-API surface, and the rationale is in a comment in
  `lib.rs`

`glovebox-mcp/src/schemas.rs` allows `clippy::option_option` on the corrections module for the same
reason. Run `cargo clippy --workspace -- -D clippy::pedantic` to verify zero warnings. Add
per-function `#[allow]` for unavoidable cases.

## Non-Interactive Shell Commands

**ALWAYS use non-interactive flags** with file operations to avoid hanging on confirmation prompts.

Shell commands like `cp`, `mv`, and `rm` may be aliased to include `-i` (interactive) mode on some systems, causing the agent to hang indefinitely waiting for y/n input.

**Use these forms instead:**
```bash
# Force overwrite without prompting
cp -f source dest           # NOT: cp source dest
mv -f source dest           # NOT: mv source dest
rm -f file                  # NOT: rm file

# For recursive operations
rm -rf directory            # NOT: rm -r directory
cp -rf source dest          # NOT: cp -r source dest
```

**Other commands that may prompt:**
- `scp` - use `-o BatchMode=yes` for non-interactive
- `ssh` - use `-o BatchMode=yes` to fail instead of prompting
- `apt-get` - use `-y` flag
- `brew` - use `HOMEBREW_NO_AUTO_UPDATE=1` env var

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
