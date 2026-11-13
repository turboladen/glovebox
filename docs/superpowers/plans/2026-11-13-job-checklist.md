# Job Checklist and First Web Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A job lists what it needs, each need is taken from decision to purchase to installation, appointments group jobs, and the owner works the list on a local web page as well as through Claude Desktop.

**Architecture:** All tables, rules and view models go in `glovebox-shared`. `glovebox-mcp` gains thin tools over them. A new `glovebox-web` crate (axum + askama, server-rendered, no JavaScript) renders the same view models and posts plain forms. Both binaries open one SQLite file chosen by `GLOVEBOX_ENV`.

**Tech Stack:** Rust 2024, SeaORM 2.0 + SQLite (WAL), rmcp 3.9 (stdio), axum 0.8, askama 0.16, tower (tests).

**Spec:** `docs/superpowers/specs/2026-11-13-job-checklist-design.md`. Read it before any task; it is the authority where this plan is silent.

## Global Constraints

- **`data/glovebox.db` holds the owner's real records.** Never open it for writing, reset it or delete it. Task 2 verifies its migration against a *copy* in the scratch directory.
- Never read or edit any Claude Desktop configuration file.
- Migrations are additive and rerun-safe (`IF NOT EXISTS`, pragma-guarded `ALTER`). No existing row is rewritten.
- Every rule in `CLAUDE.md` holds: layering, `DomainResult`, `updated_at` stamping in service update functions, cross-reference guards with a byte-identical `NotFound` for a wrong-vehicle id and a regression test per link, integer cents, `YYYY-MM-DD` dates, batch loads with `is_in()`, `sea_orm::*` glob imports, pedantic clippy.
- MCP inputs: `#[serde(deny_unknown_fields)]`, no `flatten`, `Option<Option<T>>` for clearable fields, schemas with no `$ref`, and the pinned tool list in `glovebox-mcp/tests/reads.rs` updated.
- `glovebox-web` runs no SeaORM query and owns no table. Every number or sentence on a page is produced by a tested function in `glovebox-shared`.
- No JavaScript in `glovebox-web`. Every state shown by color is also shown by a word.
- Comments and docs: full sentences, present behavior only, no `//` block over 10 lines, tests use `//`.
- Never stage `.beads/issues.jsonl`. Run `just ci` before every commit; `just fmt` uses nightly.
- Vocabulary (exact strings): supplier `self` | `shop`; approach `replacement` | `upgrade`; fitment `confirmed` | `unconfirmed` | `does_not_fit`; need stages `undecided`, `to_order`, `shop_supplies`, `ordered`, `received`, `installed`; appointment status `planning` | `done` | `cancelled`; part statuses gain `returned`.
- Ports: 4827 for `live`, 4828 for any other environment.

## Review Focus

1. A part is linked to a second need while still linked to a first. Expected: refused with `BadRequest` naming the other need (test in Task 4).
2. The chosen option is deleted, or the linked part is deleted through `delete_part`. Expected: the need falls back to `undecided` or `to_order`; nothing dangles (Tasks 3 and 4).
3. A service is deleted after it completed an appointment's jobs and installed its parts. Expected: the appointment reads `planning` again and the needs read `received` or `ordered` (Task 7).
4. Money typed on the page: `935`, `$935.00`, `1,234.5`, `-5`, `abc`, empty. Expected: the first three parse, the rest are `Invalid` with the row re-rendered and nothing written (Tasks 9 and 12).
5. The web page and the MCP server write at the same moment. Expected: the second write waits and succeeds, with no "database is locked" error (Task 1).

## File Structure

```
glovebox-shared/src/
  env.rs                       # GLOVEBOX_ENV -> db path, port, name (Task 1)
  money.rs                     # parse_dollars, format_cents (Task 9)
  migration/m20261113_000001_job_checklist.rs
  entities/{appointment,job_need,need_option,todo}.rs
  inputs/{appointment,need,todo}.rs
  services/{need,appointment,todo,checklist}.rs
  seed.rs                      # made-up data for a non-live environment (Task 9)
glovebox-mcp/src/handler/checklist.rs      # appointment, need, option, purchase, todo tools
glovebox-mcp/src/schemas/checklist.rs
glovebox-mcp/tests/checklist.rs
glovebox-web/
  Cargo.toml  src/{main,lib,guard,pages}.rs  templates/*.html  tests/{common/mod,pages,actions}.rs
```

---

### Task 1: Environments, busy timeout and the stale-binary guard

**Files:** Create `glovebox-shared/src/env.rs`. Modify `glovebox-shared/src/{lib.rs,db.rs}`, `glovebox-mcp/src/main.rs`, `justfile`, `docs/claude-desktop/README.md` (add a test-server config snippet file `claude_desktop_config.test.snippet.json`).

**Interfaces — Produces:**
```rust
// glovebox_shared::env
pub struct Environment { pub name: String, pub db_path: String, pub web_port: u16 }
impl Environment { pub fn is_live(&self) -> bool }
/// `lookup` returns an environment variable's value; production passes `|k| std::env::var(k).ok()`.
pub fn resolve(lookup: impl Fn(&str) -> Option<String>) -> DomainResult<Environment>
```
Rules: `GLOVEBOX_ENV` absent or `live` → name `live`, `data/glovebox.db`, port 4827. Any other name matching `[a-z0-9-]{1,32}` → `data/glovebox-<name>.db`, port 4828; anything else is `Invalid { field: "GLOVEBOX_ENV" }`. `GLOVEBOX_DB_PATH` overrides the path only. `GLOVEBOX_WEB_PORT` overrides the port.

- [ ] **Step 1: Write failing tests in `env.rs`**: `default_is_live_database_and_port`, `named_env_uses_suffixed_file_and_other_port` (`test` → `data/glovebox-test.db`, 4828), `explicit_db_path_wins_and_keeps_env_name`, `bad_env_name_is_invalid` (`"../x"`, `"Test"`, `""`).
- [ ] **Step 2: Run** `cargo test -p glovebox-shared env::` — expect compile failure, then FAIL.
- [ ] **Step 3: Implement `resolve`.** Use it in `glovebox-mcp/src/main.rs` in place of the inline `GLOVEBOX_DB_PATH` read, and log the environment name.
- [ ] **Step 4: Busy timeout.** In `db::connect_options` add `.busy_timeout(Duration::from_secs(5))`. Test `two_connections_to_one_file_do_not_fail_with_locked`: open two `connect()`s on one temp file, begin a write transaction on the first, spawn a write on the second, commit the first after 200 ms, assert the second returns `Ok`.
- [ ] **Step 5: Stale-binary guard.** Test `connect_refuses_a_database_with_an_unknown_migration`: connect a temp file, insert a row `('m29990101_000001_future', <now>)` into `seaql_migrations`, drop the connection, call `connect` again, assert `Err` whose message contains `rebuild`. If `Migrator::up` already errors, map that error to `DomainError::Internal("… was migrated by a newer glovebox; rebuild with `just install`")`; if it does not, compare applied names against `Migrator::migrations()` before calling `up`.
- [ ] **Step 6: justfile.** `reset-db env="test"` deletes that environment's three files and exits non-zero with a message when `env` is `live` unless `confirm="yes"` is also passed. Keep `install` unchanged.
- [ ] **Step 7:** `just ci`, commit `feat: select the database by GLOVEBOX_ENV and wait on a busy database`.

### Task 2: Schema and entities

**Files:** Create `migration/m20261113_000001_job_checklist.rs`, `entities/{appointment,job_need,need_option,todo}.rs`. Modify `migration/mod.rs`, `entities/{mod,work_item,part,service_record}.rs`.

**Schema (final):**
```
appointments: id, vehicle_id FK vehicles CASCADE, purpose TEXT NOT NULL, shop_name TEXT, shop_id INT FK shops SET NULL,
              planned_date TEXT, notes TEXT, cancelled_on TEXT, closed_on TEXT, created_at, updated_at
job_needs:    id, work_item_id FK work_items CASCADE, name TEXT NOT NULL, quantity INT NOT NULL DEFAULT 1,
              supplied_by TEXT, approach TEXT, chosen_option_id INT FK need_options SET NULL,
              decision_reason TEXT, decided_on TEXT, part_id INT FK parts SET NULL, created_at, updated_at
              UNIQUE INDEX on part_id WHERE part_id IS NOT NULL
need_options: id, need_id FK job_needs CASCADE, description TEXT NOT NULL, manufacturer TEXT, part_number TEXT,
              seller TEXT, price_cents INT, url TEXT, fitment TEXT NOT NULL DEFAULT 'unconfirmed',
              fitment_note TEXT, rejected_reason TEXT, rejected_on TEXT, created_at, updated_at
todos:        id, vehicle_id FK vehicles CASCADE, appointment_id INT FK appointments SET NULL, title TEXT NOT NULL,
              notes TEXT, done_on TEXT, created_at, updated_at
ALTER work_items       ADD appointment_id INT REFERENCES appointments(id) ON DELETE SET NULL
ALTER service_records  ADD appointment_id INT REFERENCES appointments(id) ON DELETE SET NULL
ALTER parts            ADD received_on TEXT, returned_on TEXT, return_reason TEXT, order_ref TEXT,
                           returned_from_need_id INT REFERENCES job_needs(id) ON DELETE SET NULL
```
New entity fields go at the end of each existing entity struct, in the order above.

- [ ] **Step 1: Failing test** in `db.rs` tests: `job_checklist_tables_exist_and_cascade` — on `test_db()`, create vehicle → work item → need → option; delete the work item; assert zero needs and options. Second test `migration_is_rerun_safe`: call the migration's `up` twice on one connection and assert `Ok`.
- [ ] **Step 2:** Run, expect FAIL (no such table).
- [ ] **Step 3: Write the migration** with `IF NOT EXISTS` tables and each `ALTER` guarded by a `pragma_table_info` check (follow the `new-migration` skill templates). Add entities with relations.
- [ ] **Step 4: Verify against real data, on a copy.** `cp data/glovebox.db "$SCRATCH/copy.db"` (also the `-wal` and `-shm` files if present), then run a small test binary or `GLOVEBOX_DB_PATH="$SCRATCH/copy.db" cargo run -p glovebox-mcp < /dev/null` so the migration applies to the copy, and confirm with `sqlite3 "$SCRATCH/copy.db" "select count(*) from parts; select count(*) from service_records; pragma foreign_key_check;"` that counts equal the original's (read the original with `sqlite3 -readonly`) and the check prints nothing. Record the output in the task report. Delete the copy.
- [ ] **Step 5:** `just ci`, commit `feat: schema for appointments, needs, options and to-dos`.

### Task 3: Needs and options

**Files:** Create `inputs/need.rs`, `services/need.rs`. Modify `inputs/mod.rs`, `services/mod.rs`.

**Interfaces — Produces:**
```rust
pub const VALID_SUPPLIERS: [&str; 2] = ["self", "shop"];
pub const VALID_APPROACHES: [&str; 2] = ["replacement", "upgrade"];
pub const VALID_FITMENTS: [&str; 3] = ["confirmed", "unconfirmed", "does_not_fit"];
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)] #[serde(rename_all = "snake_case")]
pub enum Stage { Undecided, ToOrder, ShopSupplies, Ordered, Received, Installed }
/// Pure. `part` is the linked part, if any.
pub fn stage(need: &job_need::Model, part: Option<&part::Model>) -> Stage

pub async fn add(db, vehicle_id: i32, work_item_id: i32, input: NewNeed) -> DomainResult<job_need::Model>
pub async fn update(db, vehicle_id: i32, need_id: i32, input: UpdateNeed) -> DomainResult<job_need::Model>
pub async fn delete(db, vehicle_id: i32, need_id: i32) -> DomainResult<()>
pub async fn require(db, vehicle_id: i32, need_id: i32) -> DomainResult<job_need::Model> // "Need {id} not found"
pub async fn add_option(db, vehicle_id, need_id, input: NewOption) -> DomainResult<need_option::Model>
pub async fn update_option(db, vehicle_id, option_id, input: UpdateOption) -> DomainResult<need_option::Model>
pub async fn delete_option(db, vehicle_id, option_id) -> DomainResult<()>   // "Option {id} not found"
/// Sets or clears the chosen option and the reason in one write; stamps `decided_on` with `services::today()`.
pub async fn decide(db, vehicle_id, need_id, option_id: Option<i32>, reason: Option<String>) -> DomainResult<job_need::Model>
```
`NewNeed { name, quantity: Option<i32>, supplied_by: Option<String>, approach: Option<String> }`. `UpdateNeed` uses double options for `supplied_by`, `approach`, `decision_reason`. `NewOption { description, manufacturer, part_number, seller, price_cents, url, fitment: Option<String>, fitment_note }`; `UpdateOption` adds `rejected_reason: Option<Option<String>>` (setting it stamps `rejected_on`, clearing it clears both).

Rules: `quantity >= 1`; vocabulary checked with `Invalid`; `decide` refuses (`BadRequest`) an option from another need, a `does_not_fit` option ("… does not fit; change its fitment first") and a rejected option; rejecting or marking `does_not_fit` the currently chosen option clears the choice in the same write.

- [ ] **Step 1: Failing tests** (names are the spec): `stage_table_first_match_wins` (table-driven over all six rows, including linked-part-with-no-option → `Ordered` and shop need with installed part → `Installed`); `add_rejects_zero_quantity_and_unknown_supplier`; `decide_refuses_option_that_does_not_fit`; `decide_refuses_rejected_option`; `decide_refuses_option_of_another_need`; `rejecting_the_chosen_option_clears_the_choice`; `deleting_the_chosen_option_clears_the_choice`; `decide_stamps_decided_on`; `wrong_vehicle_need_reads_as_missing` and `wrong_vehicle_option_reads_as_missing` (assert the error string equals the nonexistent-id string).
- [ ] **Step 2:** Run `cargo test -p glovebox-shared need::` — FAIL.
- [ ] **Step 3: Implement.** Ownership is resolved need → work item → vehicle in one joined query.
- [ ] **Step 4:** Run — PASS. `just ci`, commit `feat: needs and researched options on a job`.

### Task 4: Buying, receiving and returning

**Files:** Modify `services/{need,part,costs}.rs`, `inputs/need.rs`. Add `returned` wherever part status is validated or matched (search `"purchased"` and `"replaced"`).

**Interfaces — Produces:**
```rust
pub struct OrderDetails { pub seller: Option<String>, pub purchase_date: Option<String>,
                         pub cost_cents: Option<i32>, pub order_ref: Option<String> }
/// Creates the part from the chosen option (fields in `details` win) with `supplied_by = "self"` and links it.
pub async fn mark_ordered<C: ConnectionTrait + TransactionTrait>(db: &C, vehicle_id, need_id, details: OrderDetails) -> DomainResult<NeedPurchase>
pub async fn link_part<C: ..>(db, vehicle_id, need_id, part_id, received_on: Option<String>) -> DomainResult<NeedPurchase>
pub async fn unlink_part(db, vehicle_id, need_id) -> DomainResult<job_need::Model>
pub async fn mark_received(db, vehicle_id, need_id, on: Option<String>) -> DomainResult<NeedPurchase> // default today
pub async fn clear_received(db, vehicle_id, need_id) -> DomainResult<NeedPurchase>
pub struct ReturnInput { pub reason: String, pub option_was_wrong: bool }
pub async fn return_part<C: ..>(db, vehicle_id, need_id, input: ReturnInput) -> DomainResult<ReturnOutcome>
#[derive(Serialize)] pub struct NeedPurchase { pub need: job_need::Model, pub part: part::Model, pub stage: Stage }
#[derive(Serialize)] pub struct ReturnOutcome { pub need: job_need::Model, pub returned_part: part::Model, pub todo: todo::Model, pub stage: Stage }
```
Rules: `mark_ordered` needs a chosen option and no linked part, and refuses a `shop` need; a need with no supplier becomes `self`. `link_part` needs a same-vehicle part that is not `returned` and not linked to another need (`BadRequest` "Part {id} is already linked to need {n}"), and sets the need's supplier from the part when unset. `return_part` needs stage `Ordered` or `Received`; in one transaction it sets the part `returned` with `returned_on`, `return_reason` and `returned_from_need_id`, unlinks it, when `option_was_wrong` rejects the chosen option with the same reason, and inserts the to-do `Return <part name> to <seller>` (or `Return <part name>` with no seller) on the job's appointment, else on the vehicle. `costs::summary` leaves `returned` parts out of every parts figure. `part::delete` on a linked part leaves the need unlinked (the FK does it; test it).

This task needs `todo::Model` and a minimal insert. Create `entities` use only; the to-do row is inserted directly inside `return_part`'s transaction through a `pub(crate) fn todo::insert(txn, vehicle_id, appointment_id, title)` that Task 5 builds on. Create `services/todo.rs` with just that function here.

- [ ] **Step 1: Failing tests:** `mark_ordered_copies_the_option_and_lets_details_win`; `mark_ordered_without_a_choice_is_refused`; `mark_ordered_on_a_shop_need_is_refused`; `link_part_refuses_a_part_linked_elsewhere`; `link_part_refuses_a_returned_part`; `link_part_sets_received_when_a_date_is_given`; `received_then_cleared_round_trips`; `return_part_marks_returned_unlinks_and_adds_a_todo` (assert part status, `return_reason`, need `part_id` is `None`, stage `ToOrder`, to-do title `Return Rear rotors to FCP Euro`); `return_with_wrong_option_rejects_it_and_goes_undecided`; `return_before_ordering_is_refused`; `deleting_a_linked_part_leaves_the_need_to_order`; `cost_summary_leaves_out_returned_parts`; wrong-vehicle tests for need and part ids.
- [ ] **Step 2:** Run — FAIL. **Step 3:** Implement. **Step 4:** PASS, `just ci`, commit `feat: order, receive and return the part a need calls for`.

### Task 5: Appointments, to-dos and assigning jobs

**Files:** Create `inputs/{appointment,todo}.rs`, `services/appointment.rs`; extend `services/todo.rs`, `services/work_item.rs`, `inputs/work_item.rs`.

**Interfaces — Produces:**
```rust
// appointment
pub async fn create(db, vehicle_id, input: NewAppointment) -> DomainResult<appointment::Model>
pub async fn update(db, vehicle_id, id, input: UpdateAppointment) -> DomainResult<appointment::Model>
pub async fn require(db, vehicle_id, id) -> DomainResult<appointment::Model>          // "Appointment {id} not found"
pub async fn list(db, vehicle_id, include_closed: bool) -> DomainResult<Vec<AppointmentSummary>>
/// Sets `closed_on` (or `cancelled_on` when `cancel`) and unassigns every job still `planned`. Returns their ids.
pub async fn close<C: ..>(db, vehicle_id, id, cancel: bool) -> DomainResult<ClosedAppointment>
pub async fn reopen(db, vehicle_id, id) -> DomainResult<appointment::Model>
pub async fn delete<C: ..>(db, vehicle_id, id) -> DomainResult<()>                      // jobs survive, unassigned
/// Pure. `job_statuses` are the statuses of the jobs on the appointment.
pub fn status(a: &appointment::Model, job_statuses: &[&str]) -> &'static str
#[derive(Serialize)] pub struct AppointmentSummary { #[serde(flatten)] pub appointment: appointment::Model,
    pub status: &'static str, pub booked: bool, pub jobs: usize, pub needs_open: usize, pub todos_open: usize }
// todo
pub async fn add(db, vehicle_id, input: NewTodo) -> DomainResult<todo::Model>
pub async fn update(db, vehicle_id, id, input: UpdateTodo) -> DomainResult<todo::Model>
pub async fn complete(db, vehicle_id, id, on: Option<String>) -> DomainResult<todo::Model>
pub async fn reopen(db, vehicle_id, id) -> DomainResult<todo::Model>
pub async fn delete(db, vehicle_id, id) -> DomainResult<()>
pub async fn list(db, vehicle_id, appointment_id: Option<Option<i32>>, include_done: bool) -> DomainResult<Vec<todo::Model>>
```
`NewAppointment { purpose, shop_name, shop_id, planned_date, notes }` (purpose non-blank; shop resolved exactly as `service_record::create` resolves `shop_name`/`shop_id` — reuse that function). `NewWorkItem` gains `appointment_id: Option<i32>` and `UpdateWorkItem` gains `appointment_id: Option<Option<i32>>`, guarded to the same vehicle and refused for a cancelled or hand-closed appointment.

`status` rule: `cancelled_on` set → `cancelled`; else `closed_on` set → `done`; else at least one job and every job `done` or `dropped` → `done`; else `planning`. A need is "open" for `needs_open` when its stage is `Undecided`, `ToOrder` or `Ordered`.

- [ ] **Step 1: Failing tests:** `status_table` (no jobs → planning; all done → done; one planned → planning; cancelled wins; hand-closed → done); `create_rejects_blank_purpose`; `assigning_a_job_from_another_vehicle_reads_as_missing`; `assigning_to_a_cancelled_appointment_is_refused`; `close_unassigns_planned_jobs_and_keeps_their_needs`; `delete_keeps_jobs`; `summary_counts_open_needs_and_todos`; `todo_complete_and_reopen_round_trip`; `todo_on_another_vehicles_appointment_reads_as_missing`; wrong-vehicle tests for appointment and to-do ids.
- [ ] **Step 2–4:** FAIL, implement (batch-load jobs, needs, parts, to-dos with `is_in`), PASS, `just ci`, commit `feat: appointments group jobs and to-dos`.

### Task 6: The checklist read, past decisions and `whats_coming`

**Files:** Create `services/checklist.rs`. Modify `services/upcoming.rs`.

**Interfaces — Produces:**
```rust
#[derive(Serialize)] pub struct Checklist {
    pub appointment: Option<AppointmentSummary>,   // None for the unscheduled view
    pub next_steps: NextSteps, pub jobs: Vec<JobView>, pub todos: Vec<todo::Model>, pub warnings: Vec<String> }
#[derive(Serialize)] pub struct NextSteps { pub decide: Vec<StepRow>, pub order: Vec<StepRow>,
    pub arriving: Vec<StepRow>, pub todos: Vec<todo::Model>, pub done: Vec<StepRow> }
#[derive(Serialize)] pub struct StepRow { pub need_id: i32, pub job_id: i32, pub job_title: String,
    pub name: String, pub summary: String, pub stage: Stage }
#[derive(Serialize)] pub struct JobView { pub job: work_item::Model, pub needs: Vec<NeedView> }
#[derive(Serialize)] pub struct NeedView { pub need: job_need::Model, pub stage: Stage,
    pub chosen: Option<need_option::Model>, pub other_options: Vec<need_option::Model>,
    pub part: Option<part::Model>, pub purchase_summary: Option<String>, pub notes: Vec<String> }
pub async fn for_appointment(db, vehicle_id, appointment_id) -> DomainResult<Checklist>
pub async fn for_unscheduled(db, vehicle_id) -> DomainResult<Checklist>   // planned jobs with no appointment that have needs
pub async fn for_job(db, vehicle_id, work_item_id) -> DomainResult<JobView>
#[derive(Serialize)] pub struct PastDecision { pub vehicle_id: i32, pub vehicle_name: String, pub job_title: String,
    pub need: job_need::Model, pub chosen: Option<need_option::Model>, pub rejected: Vec<need_option::Model>,
    pub returned_parts: Vec<part::Model> }
pub async fn past_decisions(db, query: &str) -> DomainResult<Vec<PastDecision>>
```
`summary` sentences (exact forms, tested): decide → `Rear pads: no part chosen` or `Rear pads: who supplies it is not decided`; order → `Rear rotors: order OE 5Q0615601E from FCP Euro`; arriving → `Front pads: ordered from FCP Euro on 2026-11-14`. `purchase_summary` → `FCP Euro · 2026-11-06 · $84.99 · order 12345` using only the parts present. `NeedView.notes` and `Checklist.warnings` carry: `Fitment of the chosen option is not confirmed.`; `The linked part's number (X) differs from the chosen option's (Y).`; `A part is linked and no decision is recorded.` Next-step groups are ordered by job, then need id; `done` holds `ShopSupplies`, `Received` and `Installed`.

`past_decisions` matches `query` case-insensitively against need names and option descriptions, manufacturers and part numbers, across all vehicles, newest `decided_on` first; a blank query is `Invalid`. `returned_parts` are the parts whose `returned_from_need_id` is the need.

`Upcoming` gains `appointments: Vec<AppointmentSummary>` (open ones only).

- [ ] **Step 1: Failing tests:** `next_steps_group_by_stage_in_order` (build the spec's 60k example: rear pads undecided, rear rotors and front pads to order, front rotors received → assert group membership and the three summary strings exactly); `shop_supplied_need_is_done_not_open`; `unconfirmed_fitment_and_mismatched_part_number_warn`; `linked_part_without_decision_is_noted`; `unscheduled_lists_only_planned_jobs_with_needs`; `past_decisions_finds_by_need_name_and_part_number_across_vehicles`; `past_decisions_includes_rejected_options_and_returned_parts`; `blank_query_is_invalid`; `whats_coming_lists_open_appointments_with_counts`.
- [ ] **Step 2–4:** FAIL, implement, PASS, `just ci`, commit `feat: the checklist view, past decisions and appointments in whats_coming`.

### Task 7: Filing an invoice against an appointment

**Files:** Modify `inputs/{service_record,part}.rs`, `services/service_record.rs`.

**Interfaces:** `NewServiceRecord` and `UpdateServiceRecord` gain `appointment_id` (`Option<i32>` / `Option<Option<i32>>`). `NewPart` gains `need_id: Option<i32>` (honored only for inline `parts` of a service: the created part is linked to that need, which must be on this vehicle and unlinked). `ServiceRecordWithLinks` gains `appointment_id` (already in the flattened record) and
```rust
#[serde(skip_serializing_if = "Vec::is_empty")] pub unfitted_needs: Vec<UnfittedNeed>,
#[derive(Serialize)] pub struct UnfittedNeed { pub need_id: i32, pub name: String, pub job_title: String, pub part_id: i32 }
```
`unfitted_needs` lists, on create and update results only, every need on a job this service completed whose linked part is not installed.

- [ ] **Step 1: Failing tests:** `naming_an_appointment_installs_and_completes_nothing_by_itself`; `named_parts_install_and_their_needs_read_installed`; `inline_shop_part_fills_its_need`; `unfitted_needs_lists_linked_parts_not_named`; `appointment_reads_done_when_its_last_job_completes`; `deleting_the_service_reopens_the_appointment_and_needs_fall_back` (Review Focus 3: status `planning`, a received part's need reads `Received`); `appointment_from_another_vehicle_reads_as_missing`; `inline_part_need_from_another_vehicle_reads_as_missing`; `one_bad_need_id_commits_nothing`.
- [ ] **Step 2–4:** FAIL, implement inside the existing transaction, PASS, `just ci`, commit `feat: file an invoice against an appointment`.

### Task 8: MCP tools and the project prompt

**Files:** Create `glovebox-mcp/src/handler/checklist.rs`, `src/schemas/checklist.rs`, `tests/checklist.rs`. Modify `handler/{mod,planning,services,parts}.rs`, `schemas.rs`, `schemas/corrections.rs`, `tests/{reads,rehearsal,common/mod}.rs`, `docs/claude-desktop/{README.md,glovebox-project-prompt.md}`, `CLAUDE.md`, `AGENTS.md` (keep the two bodies identical).

**Tools (names fixed here so docs and the pinned list agree):**

| Tool | Calls |
| --- | --- |
| `plan_appointment`, `update_appointment`, `list_appointments`, `delete_appointment` | `appointment::{create, update (plus close/cancel/reopen through a `status` field), list, delete}` |
| `get_checklist` (`appointment_id`, or `work_item_id`, or neither for the unscheduled view) | `checklist::*` |
| `add_need`, `update_need` (includes `chosen_option_id` + `decision_reason` → `need::decide`), `delete_need` | `need::*` |
| `add_need_option`, `update_need_option`, `delete_need_option` | `need::*_option` |
| `mark_need_ordered`, `link_part_to_need` (`part_id: null` unlinks), `mark_need_received` (`received: false` clears), `return_part` | Task 4 |
| `add_todo`, `update_todo` (`done: true/false`), `delete_todo` | `todo::*` |
| `past_decisions` | `checklist::past_decisions` |

Existing tools: `plan_work` and `update_planned_work` accept `appointment_id`; `record_service` and `correct_service` accept `appointment_id`, and `record_service`'s inline parts accept `need_id`; `record_part`/`correct_part` expose `order_ref` and accept status `returned`. Each description says when to use the tool and what to call next, in the style of the existing ones.

**Project prompt, new section "Planning a job":** add needs as research finds them; record every option considered with a fitment verdict and the evidence for it; record why an option is rejected; never choose the supplier or the option for the owner — ask, then record the answer and the reason in the owner's words; call `past_decisions` before researching a part the owner may have chosen before; when filing an invoice for an appointment pass `appointment_id` and ask about every entry in `unfitted_needs`. README: `just web`, environments, and the test-server snippet.

- [ ] **Step 1: Failing tests** in `tests/checklist.rs`: `sixty_k_story` (through tool calls only: plan appointment, plan two jobs onto it, add needs, add three rear-rotor options with one `does_not_fit`, try to choose it → tool error naming fitment, choose another with a reason, `mark_need_ordered`, `mark_need_received`, `return_part`, `get_checklist` shows the to-do and the need back under `order`); `existing_part_links_without_reentry`; `past_decisions_returns_reason_and_rejected`; `record_service_with_appointment_reports_unfitted_needs`; `unknown_field_is_a_tool_error_naming_it` (one per new input struct, table-driven); `wrong_vehicle_ids_read_as_missing`. Update the pinned tool list and the no-`$ref` test passes unchanged.
- [ ] **Step 2–4:** FAIL, implement, PASS. Extend `rehearsal.rs` with one appointment in the new owner's first session. `just ci`, commit `feat: MCP tools for appointments, needs, options, purchases and to-dos`.

### Task 9: Seed data and money helpers

**Files:** Create `glovebox-shared/src/{seed.rs,money.rs}`; modify `lib.rs` and `services/checklist.rs`. This task adds library functions only. Task 10 gives the seed its command.

**Interfaces — Produces:**
```rust
// money
pub fn parse_dollars(text: &str) -> DomainResult<i32>   // "935", "$935.00", "1,234.5" -> cents; negative, blank, junk -> Invalid{field:"amount"}
pub fn format_cents(cents: i32) -> String               // 8499 -> "$84.99", 93500 -> "$935.00", integer math only
// seed
/// Fills an empty non-live database with made-up data. Errors if any vehicle exists.
pub async fn demo<C: ConnectionTrait + TransactionTrait>(db: &C) -> DomainResult<SeedSummary>
```
The seed creates one made-up vehicle on the GTI template, one appointment "60k service + loose ends" with three jobs, needs covering all six stages, one need with three options (one `does_not_fit`, one rejected), one returned part with its to-do, two more to-dos (one done), and one unscheduled job with a need. No real VIN, plate or name.

- [ ] **Step 1: Failing tests:** `parse_dollars_table` (Review Focus 4 values), `format_cents_table`, `demo_seed_covers_every_stage` (collect stages from `checklist::for_appointment` and assert all six appear), `demo_seed_refuses_a_database_with_a_vehicle`.
- [ ] **Step 2–4:** FAIL, implement, PASS. Replace the ad hoc cents formatting in `checklist.rs` with `format_cents`. `just ci`, commit `feat: demo seed and money parsing for the web page`.

### Task 10: The web crate skeleton and home page

**Files:** Create `glovebox-web/{Cargo.toml, src/main.rs, src/lib.rs, src/guard.rs, src/pages.rs, templates/{base,home,not_found}.html, tests/common/mod.rs, tests/pages.rs}`. Modify root `Cargo.toml` (member; workspace deps `axum = "0.8"`, `askama = "0.16"`, `tower = "0.5"`), `justfile`, `scripts/check-layering.sh`, `.github/workflows/*` only if a job lists crates by name.

Model the crate on `/Users/sloveless/Development/projects/financier/crates/advisor-web` (read its `src/lib.rs`, `src/guard.rs`, `src/main.rs`, `tests/common/mod.rs` and its `CLAUDE.md` first; copy the patterns, not the styles).

**Interfaces — Produces:**
```rust
#[derive(Clone)] pub struct AppState { pub db: DatabaseConnection, pub env_name: String, pub today: fn() -> chrono::NaiveDate }
pub fn router(state: AppState) -> axum::Router
```
- `main.rs`: `glovebox-web` serves; `glovebox-web seed` runs `seed::demo` and refuses when `Environment::is_live()`. Binds `127.0.0.1:<port>` only. Tracing to stderr.
- `guard.rs`: middleware returning 403 unless `Host` is `127.0.0.1:<port>` or `localhost:<port>`, and for non-GET requests unless `Origin` (when present) is one of those; adds `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'`.
- `GET /`: per vehicle, open appointments (purpose, date or "not booked", counts as plain statements such as `3 needs open · 2 to-dos open`), the "Not scheduled yet" jobs from `checklist::for_unscheduled`, and vehicle to-dos with no appointment. The data comes from one new function `checklist::home(db) -> DomainResult<Vec<VehicleHome>>` in `glovebox-shared`, with its own test there.
- `base.html` for this task is plain and unstyled beyond layout; Task 11 owns the look. The header shows the environment name whenever it is not `live`.
- `justfile`: `web env="live"` builds release `glovebox-web`, opens `http://127.0.0.1:<port>/` and runs it with `GLOVEBOX_ENV={{env}}`; `seed env="test"` runs the seed subcommand and fails for `live`. `ci-backend` already covers the workspace.
- `check-layering.sh`: add a second check failing on `sea_orm::(Entity|ActiveModel|Query|Condition)|EntityTrait|ActiveModelTrait|\.filter\(.*Column::` under `glovebox-web/src`.

- [ ] **Step 1: Failing tests** (`tests/pages.rs`, `tower::ServiceExt::oneshot` over `test_db()` with a fixed `today`): `home_lists_open_appointments_and_unscheduled_jobs`; `home_shows_env_name_only_when_not_live`; `foreign_host_is_403`; `cross_origin_post_is_403`; `every_response_carries_the_csp`; `unknown_path_is_404_page`; and in `glovebox-shared`, `home_view_groups_by_vehicle`.
- [ ] **Step 2–4:** FAIL, implement, PASS. Run `just seed env=test && just web env=test` once and confirm the page loads at port 4828 (record what was seen). `just ci`, commit `feat: glovebox-web serves a home page from the shared library`.

### Task 11: The look — a static mockup the owner approves

**This task ends at a gate. The controller stops and shows the owner; nothing in Task 12 starts before approval.**

**Files:** Create `docs/design/job-checklist-mockup.html` (one self-contained static file, no script) and screenshots are not committed.

- [ ] **Step 1:** Invoke the `frontend-design` skill. Read the spec's "Look and feel" and the structural mockup at `.superpowers/brainstorm/87370-1794600449/content/appointment-list.html` if it is present.
- [ ] **Step 2:** Build the appointment page as static HTML using the Task 9 demo data: header with environment name, the next-steps list (decide / order / arriving / to-dos / done folded with `<details>`), the by-job blocks with one expanded need showing options, fitment, reason and purchase line, an undo banner, and one row in its failed-write state. Light and dark through `prefers-color-scheme`. Direction: an instrument face — matte near-black and off-white, legibility-first type with tabular numerals, thin rules in place of boxes, a strict grid, one sparing accent; red and green allowed for state and always paired with a word; no Porsche marks, typefaces or imagery.
- [ ] **Step 3:** Check AA contrast for text and pills in both schemes, and that no two states differ by hue alone. Open it in a browser at desktop width and at 400 px wide (it must not overflow).
- [ ] **Step 4:** Commit `docs: static mockup of the appointment page`. Report the file path. **Stop for the owner's approval; apply their changes and re-commit until approved.**

### Task 12: Appointment and job pages with their actions

**Files:** Create `templates/{appointment,job,need_row,step_row,todo_row,banner}.html`, `tests/actions.rs`. Modify `src/{lib,pages}.rs`, `templates/{base,home}.html` (move the approved mockup's stylesheet into `base.html`).

**Routes:**
```
GET  /vehicles/{v}/appointments/{id}          GET /vehicles/{v}/jobs/{id}
POST /vehicles/{v}/needs/{id}/supplier        form: supplied_by ("", self, shop), approach ("", replacement, upgrade), back
POST /vehicles/{v}/needs/{id}/decision        form: option_id ("" clears), reason, back
POST /vehicles/{v}/needs/{id}/ordered         form: seller, purchase_date, cost (dollars text), order_ref, back
POST /vehicles/{v}/needs/{id}/received        POST …/received/undo
POST /vehicles/{v}/needs/{id}/return          form: reason (required), option_was_wrong (checkbox), back
POST /vehicles/{v}/todos                      form: title, appointment_id (optional), back
POST /vehicles/{v}/todos/{id}/done            POST …/done/undo        POST …/delete
```
Behavior: every POST calls one `glovebox-shared` function, then redirects (303) to `back` (accepted only when it starts with `/` and not `//`; otherwise `/`). `received` and todo `done` redirect with `?done=need-<id>` / `?done=todo-<id>`, and the page then shows a banner with an undo form. A failed write re-renders the `back` page with status 422 (`Invalid`, `BadRequest`), 404 (`NotFound`) or 500, and the error message inside the row it concerns; the `ordered` form keeps what was typed. The `ordered` form is prefilled from the chosen option. Options whose fitment is `does_not_fit` or that are rejected are listed and cannot be selected. The page creates no appointment, job, need or option.

- [ ] **Step 1: Failing tests** (`tests/actions.rs`): `appointment_page_shows_next_steps_then_jobs`; `choosing_an_option_moves_the_need_from_decide_to_order`; `ordering_creates_the_part_and_moves_the_need_to_arriving`; `ordering_with_bad_money_rerenders_422_and_writes_nothing` (table over `-5`, `abc`; assert zero parts and the typed seller still in the HTML); `received_shows_an_undo_banner_and_undo_restores_arriving`; `return_requires_a_reason`; `return_adds_the_todo_and_moves_the_need_back`; `does_not_fit_option_has_no_radio_button`; `todo_add_tick_undo_delete`; `back_outside_the_site_falls_back_to_home`; `need_of_another_vehicle_is_404`; `page_contains_no_script_tag`.
- [ ] **Step 2–4:** FAIL, implement, PASS. Then with `just seed env=test && just web env=test`, walk the Done-means list items 2 and 3 in a browser and record what happened. `just ci`, commit `feat: work the checklist on the appointment page`.

### Task 13: Docs and close-out

**Files:** `CLAUDE.md`, `AGENTS.md` (identical bodies), `docs/claude-desktop/README.md`.

- [ ] **Step 1:** Update the "What Is This", Commands, Environment, Workspace layout and MCP sections for three crates, the environments, the new tools and services, the stage table's home (`need::stage`), and a "Web page" section stating the rules a future page must follow (no queries, no script, forms + redirect, every sentence from a tested shared function, the look's rules).
- [ ] **Step 2:** `just ci`. Run the comment-length check from the global instructions over `*.rs`. Commit `docs: describe the web crate, environments and the checklist tools`.
- [ ] **Step 3:** Do **not** rebuild or reinstall anything for the owner's Claude Desktop. The final report tells the owner to run `just install`, restart Desktop and start a new conversation, then `just web`.
