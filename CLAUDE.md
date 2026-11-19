# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What Is This

Car maintenance tracker, used through Claude Desktop and a local web page. A Rust cargo workspace
with four crates: `glovebox-shared` (the domain library: SeaORM 2.0 + SQLite), `glovebox-mcp` (an rmcp stdio
server with domain-verb tools and read-only `glovebox://` resources), `glovebox-cli` (the
`glovebox` command the owner runs by hand) and `glovebox-web` (a local page for working a job
checklist). Claude Desktop launches the `glovebox-mcp` binary itself, so that server needs no
babysitting; the web page runs only while the owner serves it with `just web`. The question the
app answers: given what has been done to the car, its mileage and its age, what maintenance is
coming, when, how much will it cost, and what parts are worth buying and bringing to the shop, and
for each job, whether every part it needs has been decided, ordered and received.
The design is in `docs/adr/0006-claude-desktop-over-mcp-is-the-main-interface.md` and
`docs/adr/0007-a-jobs-checklist-tracks-every-need.md`, and the Desktop
setup and project prompt are in `docs/claude-desktop/`.

## Commands

```bash
just install              # Build target/release/glovebox-mcp and print the Claude Desktop config path
cargo build --workspace
cargo test --workspace    # Unit tests live in glovebox-shared services; MCP tests in glovebox-mcp/tests
cargo test -p glovebox-shared test_name           # One service-level test
cargo test -p glovebox-mcp --test rehearsal       # One MCP integration test file

just ci                   # Everything CI runs: nightly fmt-check, layering, typos, clippy, docs, build, test
                          # (often over 2 minutes: give the Bash call a 600000 ms timeout)
just fmt                  # cargo +nightly fmt — rustfmt.toml uses nightly-only options (it also rewraps `///` text, so run it after editing docs); plain `cargo fmt` passes locally but CI's format job will fail
just check-layering       # Layering gate only (scripts/check-layering.sh)
just backup               # Copy the current environment's database to data/backups/ now (glovebox backup)
just reset-env test       # Delete the named non-live environment's database files (glovebox reset --env test)
just seed test            # Fill the named non-live environment with made-up demo data (glovebox seed --env test)
just web                  # Serve the live page on 127.0.0.1:4827 and open it; `just web test` serves data/glovebox-test.db on 4828
# Visual checks: `just seed <name>` then `just web <name>`; Playwright screenshots must go under .playwright-mcp/ (git-ignored), not the scratchpad

cargo clippy --workspace --all-targets --locked -- -D warnings   # Rust lints, tests included (the CI gate; pedantic is set in Cargo.toml)

# Merging PRs: gate on parsed `gh pr checks` output showing zero pending/failed —
# `--watch` exits 0 when it races a run that hasn't registered yet
```

## Environment

`GLOVEBOX_ENV` names the environment (`live` by default). `live` uses `data/glovebox.db`, and any
other name such as `test` uses `data/glovebox-<name>.db` (`glovebox_shared::env::resolve`).
`GLOVEBOX_DB_PATH` overrides the path only, and `GLOVEBOX_WEB_PORT` overrides the web port (4827 for
`live`, 4828 otherwise). `RUST_LOG` sets the tracing filter (default `info`). The Claude Desktop
config snippets set an absolute `GLOVEBOX_DB_PATH`. No command deletes the live database;
`glovebox reset` and `glovebox seed` refuse `live`. `glovebox-web` reads the same variables as the
MCP server, and its `--open` flag opens the browser.

## Architecture

### Workspace layout

- **`glovebox-shared/`** (lib crate `glovebox_shared`) — the transport-agnostic domain:
  `src/{db.rs,env.rs,dates.rs,vocab.rs,seed.rs,entities,migration,services,inputs,error.rs,test_support.rs}`. All SQL, validation
  and business logic live here. It never depends on axum or rmcp.
- **`glovebox-mcp/`** (lib crate `glovebox_mcp` plus the `glovebox-mcp` binary) — the thin MCP
  surface. Every tool body is argument struct → shared-service call → serialize, with one
  `DomainError → MCP` mapping helper. `src/main.rs` serves `GloveboxMcp` over stdio.
- **`glovebox-cli/`** (lib crate `glovebox_cli` plus the `glovebox` binary) — `backup`,
  `reset --env <name>` and `seed --env <name>`, with `run` callable in process from tests. The
  last two refuse `live` and build the file name themselves.
- **`glovebox-web/`** (lib crate `glovebox_web` plus the `glovebox-web` binary) — an axum and askama
  server that renders the checklist. `src/pages.rs` holds the four page handlers (home, appointment, job and work sheet), `src/actions.rs`
  the form posts, `src/view.rs` the view structs, `src/guard.rs` the request guard and the content
  security policy, and `templates/` the askama templates.
- `scripts/check-layering.sh` (run by CI and `just ci`) keeps axum and rmcp out of `glovebox-shared`
  and SeaORM out of `glovebox-web`, and refuses a transaction in `glovebox-shared/src/services`
  opened any way but `services::begin_write`.

### Domain library (`glovebox-shared/src/`)

**`db.rs`**: `db::connect(path)` is the only place a SQLite connection is opened. It creates the
parent directory, uses WAL mode with foreign keys enforced and a 5-second busy timeout (the MCP
server and the web page open the same file at once), and runs `Migrator::up()`. It refuses to open
a database that records a migration this binary does not contain, with a message to rebuild. Each
binary opens through `db::connect_as` with its `db::Program`, so the message names that program's
rebuild step. The check runs only at start, so the upgrade order in `docs/claude-desktop/README.md`
matters.

**`backup.rs`**: `db::connect` copies a database that already holds data to `backups/` beside it
(`VACUUM INTO`, so the copy is consistent while another process writes) before any pending
migration runs. Otherwise it copies once per calendar day, at open and from an hourly task
(`backup::spawn_daily`) so a server that runs for weeks is covered, and only when the database
holds a vehicle. `prune` keeps the newest 14, the newest of every month and the largest file, so
copies of an emptied database cannot push out the last good one. A backup that fails stops the
open only when a migration is pending. To restore: quit Claude Desktop, move the current
`data/glovebox.db`, `-wal` and `-shm` files somewhere outside `data/backups/`, and copy a backup
to `data/glovebox.db`. The backups share a disk with the database, so they do not protect against
losing the directory or the machine.

**The database holds the owner's real records and there is no recipe that deletes it.** Never
delete, reset or overwrite `data/glovebox.db`, and never run a command against it to see what the
command does. Tests use `test_db()` or a temporary directory.

**Entities** (`entities/`): Hand-written `DeriveEntityModel` structs. Parent entities declare
`has_many` relations; junction tables have `via()` impls.

**Inputs** (`inputs/`): Plain domain input structs (`New*`/`Update*`) consumed by service
functions.

**Services** (`services/`): One module per domain (`vehicle`, `service_record`, `part`, `incident`,
`work_item`, `appointment`, `need`, `todo`, `schedule`, `recall`, `research`, ...) of free functions taking `db: &impl
ConnectionTrait` first and returning `DomainResult<T>`. Composite reads: `upcoming` (the
`whats_coming` answer: reminders, budget, planned work, recalls, open incidents), `reminders`,
`budget`, `costs`, `activity`, `search`, `work_sheet` (an appointment's sheet for the mechanic), and `checklist` (the checklist for an appointment, an
unscheduled job or one job, the past decisions across vehicles, and the home page's rows; the MCP
tools and the web page both render it), and `waiting` (what is not yet on an appointment: due
maintenance, planned jobs, open recalls and open incidents, for the home page). `whats_coming` lists each open appointment.
`seed::demo` fills an empty database with made-up data that exercises every need stage.
`vin_decode` and `nhtsa` make the only network calls.

**Errors** (`error.rs`): `DomainError::{NotFound, Invalid{field,message}, BadRequest, Db, Internal}`
with the `DomainResult<T>` alias.

**Migrations** (`migration/`): one squashed schema migration (`m20261112_000001_initial_schema`),
one seed migration (`m20261112_000002_seed_vw_mqb`: the VW MQB platform, the Golf GTI and
Alltrack templates and their schedule items), the job checklist migration
(`m20261113_000001_job_checklist`: appointments, needs, options and to-dos, additive only),
`m20261113_000002_option_choice_history` (an option's `chosen_reason` and `chosen_on`),
`m20261114_000001_typed_dates`, which refuses to finish while a date column holds a value that is
not a real `YYYY-MM-DD`, because such a row could not be read,
`m20261114_000002_utc_timestamps`, which rewrites every stamp as RFC 3339 and removes the
`datetime('now')` column defaults by editing the stored table definitions, and
`m20261114_000003_work_item_link_foreign_keys`, which gives a job's `schedule_item_id`,
`research_finding_id` and `incident_id` an `ON DELETE SET NULL` foreign key the same way, after
clearing any id that names a missing row, and `m20261115_000001_vocabulary_values`, which changes
nothing and refuses to finish while a vocabulary column holds a value outside its list, and
`m20261117_000001_work_item_quotes`, which gives a job `quoted_by`, `quoted_on` and
`quote_service_id` (an `ON DELETE SET NULL` key added with the column), and
`m20261118_000001_part_contents`, which adds a kit's items (`part_contents`, cascading with the
part), and `m20261118_000002_parts_search`, which adds the FTS5 indexes `fts_parts` and
`fts_part_contents` and fills them from the stored rows. They run on startup.

**Test harness** (`test_support.rs`): `test_db()` is an in-memory SQLite database with migrations
applied. It is compiled under `#[cfg(any(test, feature = "test-support"))]`; `glovebox-mcp` uses it
through a dev-dependency with the `test-support` feature.
`race_pools()` and `race(&pools, |i, db| ...)` run an operation twice on each of three pools; a
race test must fail when `services::begin_write` is swapped for a plain `BEGIN`.

### MCP surface (`glovebox-mcp/`)

**Transport:** stdio only. **Stdout carries only protocol frames**, so every diagnostic goes to
stderr (`tracing_subscriber` is installed with a stderr writer before anything else runs).
`tests/stdio.rs` spawns the binary and fails if stdout carries anything but JSON-RPC frames. Never
add a `println!` to this crate or to code it calls.

**Tools** (`src/handler/*.rs`, one `#[tool_router]` impl per area, combined in `handler/mod.rs`),
named as things a person would say:

- `vehicles.rs`: `list_vehicles`, `get_vehicle`, `add_vehicle`, `update_vehicle`, `delete_vehicle`,
  `archive_vehicle`, `unarchive_vehicle`, `log_mileage`, `list_mileage`, `delete_mileage`,
  `list_model_templates`
- `services.rs`: `record_service`, `link_service_to_maintenance`, `list_services`,
  `correct_service`, `delete_service`
- `parts.rs`: `record_part`, `list_parts`, `correct_part`, `delete_part`
- `incidents.rs`: `log_incident`, `save_note`, `list_incidents`, `update_incident`,
  `add_incident_followup`, `delete_incident`
- `planning.rs`: `plan_work`, `list_planned_work`, `whats_coming`, `update_planned_work`,
  `delete_planned_work`
- `checklist.rs`: `plan_appointment`, `update_appointment`, `list_appointments`,
  `delete_appointment`, `get_checklist`, `get_work_sheet`, `past_decisions`, `add_need`, `update_need`, `delete_need`,
  `add_need_option`, `update_need_option`, `delete_need_option`, `mark_need_ordered`,
  `link_part_to_need`, `mark_need_received`, `return_part`, `add_todo`, `update_todo`,
  `delete_todo`
- `schedule.rs`: `list_schedule`, `add_schedule_item`, `update_schedule_item`,
  `dismiss_schedule_item`, `restore_schedule_item`
- `recalls.rs`: `check_recalls`, `list_recalls`, `set_recall_status`, `file_research_finding`
- `reads.rs`: `summarize_recent_activity`, `search_records`, `cost_summary`, `get_service`,
  `get_incident`

Inputs are schemars-derived structs in `src/schemas.rs`, `src/schemas/corrections.rs` and
`src/schemas/checklist.rs`; their doc
comments become the LLM-visible field descriptions. `Parameters<T>` defers deserialize errors
so malformed arguments come back as actionable tool errors, not bare JSON-RPC `-32602`s, and
reads them through `serde_path_to_error`, so the error names the argument at fault. The
wrapper is named `Parameters` so the `#[tool]` macro derives the input schema from it; do not add
an `input_schema` attribute. The wrapper's `JsonSchema` impl inlines every subschema, because Claude
Desktop does not resolve `$ref` (it hides the nested fields from the model and sends the object as
a JSON string); the tools-list test fails if a reference survives. A `Parameters` type must not
refer to itself. A `correct_*` or
`update_*` input uses `Option<Option<T>>`: an absent key leaves the field alone, and an explicit
`null` clears it. An argument that edits a list by item, such as `correct_part`'s `add_contents`
and `remove_contents`, is a plain `Option<Vec<T>>`, and `null` there changes nothing. Every input struct, nested ones included, carries
`#[serde(deny_unknown_fields)]`, so an argument the tool does not define is a tool error that names
it. Serde does not support it together with `#[serde(flatten)]`, so input structs never flatten.

**`record_service` is transactional.** One call writes the service, its lines, its parts and every
link (schedule items, work items, recalls, incidents) in one transaction. One bad id fails the call
and commits nothing. Re-recording an existing `invoice_ref` for the vehicle returns the stored
record with `already_recorded: true` and a `message` and writes nothing (both keys appear only
on that path). Completing planned work happens by naming
`work_item_ids` in `record_service`, or in `correct_service` for a service already recorded.
Every service read carries `lines_total_cents`, and a `note` when the total differs from it (the
kinds are `maintenance`, `mod`, `repair`, `inspection`, `recall` and `fee`; payers are `self`,
`insurance`, `third_party` and `manufacturer`). A line may carry its own `paid_by`
(`service_record::effective_payer` gives the line's own value, else the service's), and
`cost_summary` judges out-of-pocket and covered amounts per line. Every line in a service read also
carries `effective_paid_by` (its own `paid_by`, else the service's; the stored `paid_by` stays
null when inherited). `total_cost_cents` is the invoice's grand total across all lines, including
lines someone else paid for, and a covered line's `cost_cents` is the value the invoice shows for
that work (zero or omitted when it shows none); the owner's share is the total minus the covered
lines. `cost_summary`'s `covered_work` lists the services with covered work, each with
`covered_by_others_cents` and `out_of_pocket_cents` (the covered figures sum to the summary's
`covered_cents`). `miles_covered` (the highest odometer reading minus the earliest service or
reading, dated by `history_starts_on`) is the base of the per-1,000-miles figure, which stays null
until the records cover 1,000 miles (`costs::MIN_MILES_FOR_COST_PER_1000`).

**Job checklist.** An appointment groups jobs (`work_items`) and to-dos for one shop trip, a job
lists needs, and a need lists researched options. A need's stage (`undecided`, `to_order`,
`shop_supplies`, `ordered`, `received`, `installed`) is derived by `need::stage` and never stored;
the first matching row wins, so a linked part decides the stage, and a part that is installed or
replaced reads `installed`. An appointment reads `done` when
it has jobs and every one is done or dropped; only `cancelled` and a hand-closed date are stored,
closing is refused while a job is still planned, and cancelling unassigns them. A closed or cancelled
appointment therefore never holds a planned job: when a job on one returns to `planned` (its
service is deleted, or its status is set back), a hand-closed appointment opens again and a
cancelled one lets the job go to the unscheduled pool (`appointment::settle_reopened_jobs`).
The `delete_service`, `correct_service` and `update_planned_work` replies name the appointment that
reopened and the jobs that left a cancelled one. A quote for work not done is a planned job with
`quote_service_id` naming the invoice it came with, which lists it in `quoted_work_items`;
`delete_service` keeps such a quote and names it in `quotes_unlinked`
(`docs/adr/0012-a-quote-is-planned-work.md`). Re-recording the service does not close the appointment again.
Beside the status, `appointment::phase` reads an open appointment as `planning` (no date),
`booked` (before its date) or `in_shop` (from its date on). Every summary carries it and the web
pills show it, but no rule keys on it. The readers that build summaries take `today` for it
(`docs/adr/0015-an-appointment-shows-its-phase.md`).
Deleting a service, or correcting its `part_ids` as work that did not happen, also unlinks from its
need every shop-supplied part the service stops installing that goes back to purchased; a part a later service replaced
keeps its status and its need. `correct_part` setting an installed or replaced part to
`purchased` clears its install and replace details and fills an empty `received_on` from the
install date, so its need reads received; it refuses a shop-supplied part, install or
replacement details sent in the same call, and a cleared `received_on`. `delete_service` refuses a service that recorded work as done
(jobs, installed parts, resolved incidents, recalls) until `completed_work` says whether that
work did not happen, which undoes it, or was done another way, which keeps it and drops only the
invoice. `correct_service` asks the same when its `work_item_ids`, `part_ids` or
`recall_finding_ids` leave out a completed job, a fitted part or a performed recall, and a job
added to `work_item_ids` must be planned. `docs/adr/0005-deleting-a-service-asks-what-happened.md`
records why. A need is
open when `need::counts_as_open` says so (an open stage on a planned job); every open-needs count
uses it. Rejecting or ruling out a chosen option keeps the need's reason and date on the option as
`chosen_reason` and `chosen_on` until the option is chosen again, and `past_decisions` lists both
kinds under `rejected`, so it still reports them. Choosing the option a need
already has keeps `decided_on`. A need with no chosen option holds a `decision_reason` only while
the shop supplies it: `need::update` refuses a new one otherwise (sending back the reason it already holds is no change) and clears the stored one when the
supplier moves away from `shop`. Clearing a choice the need does not have keeps its reason. The web
decision form sends back the reason it showed, so with no option chosen an unedited reason counts
as none sent, and the form goes through `need::update_with_choice` like `update_need`.
`mark_ordered` claims the need with a conditional write as the first statement of its transaction,
so two orders at once create one part, and its default cost
is the option's unit price times the need's quantity (`need::order_cost_cents`). `update_need` and
`update_appointment` apply an edit together with a choice or a status change in one transaction
(`need::update_with_choice`, `appointment::update_with_status`), so a refused choice or status
leaves the other fields unchanged. An option that is `does_not_fit` or rejected cannot be chosen,
and no tool chooses a supplier or an option for the owner. `return_part` is the only way a part
becomes `returned`, and its to-do goes on the job's appointment only while that appointment is
open (no list shows the to-dos of a closed or cancelled one), and `add_todo` and `update_todo`
refuse to put a to-do on a closed or cancelled appointment (`appointment::require_assignable`).
Open to-dos follow the same rule as planned jobs: closing is refused while one is open,
cancelling takes them off, and un-ticking a to-do reopens a hand-closed appointment or takes the
to-do off a cancelled one (`appointment::settle_reopened_todo`). Naming an appointment in `record_service` installs and completes nothing; the
result's `unfitted_needs` lists the linked parts the invoice did not install. `need_id` is accepted
on the inline parts of `record_service` only, and `get_checklist` takes at most one of
`appointment_id` and `work_item_id`.
A part bought as a kit lists its items in `contents`. `record_part`, `mark_need_ordered` and the
inline parts of `record_service` take them, and `correct_part`
edits them by item (`add_contents` appends, `remove_contents` names ids; an id not of the part is
`NotFound` and writes nothing). Every reply that returns a part returns a `part::PartView`, the
part with its contents (`part::views` loads them for many parts in one query), and the job and
appointment pages list a kit's items in the need's row.
`docs/adr/0014-a-kit-lists-its-contents.md` records why.

**Schedule names** compare through `schedule::name_key` (ASCII case and surrounding spaces do not
matter), so a vehicle's "Cabin air filter" shadows the inherited "Cabin Air Filter". A new item that shadows an inherited one takes that
item's spelling of the name and, for every field the caller leaves out, its value
(`schedule::fill_from_inherited`, shared with the copy `update_schedule_item` makes), and
`list_schedule` shows it with `overrides` (the replaced item's id and `inherited_from`). A vehicle
created with a `model_template_id` takes the template's year, make, model, trim, body style, engine,
transmission and drivetrain for each one left empty, with the precedence caller, then template,
then VIN decode; `update_vehicle` setting a template fills only the empty ones. Each reminder
carries a `baseline` (`last_service`, `purchase` or `none`), the forecast names the schedule items
and planned work it left unpriced, `bundle_suggestions` is one entry listing every overdue or
due-soon item when there are two or more, and a stored recall title is the NHTSA component in title
case.
Platforms and model templates are written only by migrations; `model_template::list` and
`list_with_platform` are the only template services
(`docs/adr/0013-templates-come-from-migrations.md`). Shops have their table, `shop_id` and
`shop::require_if_named`, but no write path yet.

**Resources**: `glovebox://vehicles`, `glovebox://vehicles/{id}` and `…/{id}/activity`.
`list_resources` enumerates concrete URIs from the database and `read_resource` parses them.

**Error mapping**: one helper (`domain_error`) used by every tool. `NotFound`, `Invalid` and
`BadRequest` become tool-level error results whose message reaches the LLM; `Db` and `Internal`
become opaque JSON-RPC internal errors with the detail kept in tracing.

**Tests** (`glovebox-mcp/tests/`): `common/mod.rs` is the harness (an in-process client over an
in-memory duplex stream and an in-memory database). One file per handler area (`vehicles`,
`services`, `parts`, `incidents`, `planning`, `checklist`, `schedule`, `recalls`, `reads`), plus
`stdio.rs` (the binary's stdout) and `rehearsal.rs` (a new owner's whole first session through
tool calls only). Add coverage for a new tool to its area's file.

### Web page (`glovebox-web/`)

A page is a view of the shared library, and a future page follows the same rules:

- **No queries.** The crate holds `glovebox_shared::db::DatabaseConnection` and calls shared
  services. It never names SeaORM, and the layering gate fails if a manifest, source or test file
  does. The one piece of page state outside the database is `AppState::deleted_todo`, the undo
  slot for the last to-do deleted on the page. It holds only a row this server deleted, and it
  does not make the page a store of records, so a future page keeps its records in the database.
  Its accepted limits: one slot serves every vehicle and tab, a double-clicked Delete or Undo can
  answer with an error page although the action worked, a failed undo keeps its row without
  showing the banner again, and a vehicle deleted during an undo gives an opaque error and writes
  nothing.
- **No script.** Every action is an HTML form post followed by a redirect back to the page, and a
  failed write renders the page again with the domain error beside the row. That page is served
  at the form's address, so every link on a page names the page's own path. The content security
  policy allows no scripts and no external resources (the page's typeface is the one file it
  loads, from this server), so a script would not run.
- **Every count and sentence about the records comes from a tested shared function** (stage,
  step sentences, open counts, option notes, warnings, errors). `src/view.rs` picks labels and
  nouns for those values, such as "× 2" or "Price for 2", and decides which forms a row shows;
  templates print what `view.rs` built.
- **Local only.** The server binds `127.0.0.1` on `GLOVEBOX_WEB_PORT` (4827 for `live`, 4828
  otherwise) and refuses a request whose Host or Origin is not local. Every page shows the
  environment's name in the header, `live` included, and the database file's name when
  `GLOVEBOX_DB_PATH` chose the file. An option's link is shown only for an `http` or `https`
  address.
- **Tests** drive the router in process over an in-memory database with a fixed clock
  (`AppState::today`, `common::today()` = 2026-11-13); date fixtures by `common::today()`, not
  `services::today()`. Askama escapes only `{{ }}` values, so assert literal template text as
  written (`I'm`) and rendered values escaped (`&#39;bogus&#39;`).
  Need-row and linked-part tests live in `tests/actions.rs`; its `need_summary` helper reads only
  the row's `<summary>`, so markup a test checks there must sit inside it.
- **The home page leads with what is waiting.** Per vehicle it lists the maintenance that is
  overdue or due soon, every planned job on no appointment, the open recalls and the open
  incidents (`waiting::waiting`), then the appointments and to-dos. Those four lists hold no
  forms. An item a planned job covers links to the job, and an item whose job is on an
  appointment is left out. `docs/adr/0002-home-page-shows-waiting-work.md` records why.
- **The work sheet** (`/vehicles/{vid}/appointments/{id}/print`, `work_sheet::for_appointment`)
  lists each planned job with the owner's parts (received or ordered, with the name, manufacturer
  and part number the linked part records, never a researched option's, and a kit's contents
  beneath it, each with a tick box) and the shop's parts by the need's name.
  Undecided and to-order needs are left off and shown in an on-screen "Before you print" box,
  whose "Ask the shop" posts the existing supplier form without an approach, which leaves the
  stored approach alone. `docs/adr/0009-the-work-sheet-prints-plain.md`
  records why.
- **Marking a need ordered has no undo**, because it creates a purchase record. A to-do tick, a
  received date and a deleted to-do show a banner with an undo form. The undo of a delete puts
  the row back through `todo::restore` and lasts until the next delete or a restart of the server.
  It is the one post that does not return to its page: a to-do whose appointment has closed comes
  back on no appointment, so its undo redirects to the home page, which lists it. MCP deletes get
  no undo.

The look lives in one inline stylesheet in `templates/base.html`, with the typeface served by
`src/assets.rs`, and `docs/adr/0008-the-web-page-reads-as-a-ledger.md` states every visual rule:
panels on a tinted ground, one sticky header line per job, gray labels and dark values, five pill
kinds each with a tint and a mark (`view::PillKind`), boxed outlines on form fields only, one
role per control style, and `YYYY-MM-DD` dates.

## Conventions

### Layering (enforced by `scripts/check-layering.sh` in CI)

- Domain inputs, validation and SQL live in `glovebox-shared::{inputs,services}`. MCP handlers only
  map tool arguments to domain inputs and rely on `domain_error` for failures. No SeaORM queries,
  `ActiveModel`s or business logic in `glovebox-mcp/src/handler/`.
- `glovebox-shared` never imports axum or rmcp, and `glovebox-web` never names SeaORM.
- Service functions take the database as the first parameter, never a server type. A service
  that checks and then writes opens its own transaction and takes `db: &C` with
  `C: ConnectionTrait + TransactionTrait`; a call made inside a caller's transaction becomes a
  savepoint. `db: &impl ConnectionTrait` is for a read, a single statement that needs no check
  (`todo::delete`), or a step only ever run inside a caller's transaction, which is
  `pub(crate)` when nothing outside the crate calls it.
  `glovebox-mcp/tests` call shared services directly as fixtures (`schedule_svc::create`), so
  search that directory before narrowing a `pub` service function to `pub(crate)`.
- **Error model**: services return `DomainResult<T>`. `Invalid` renders as `"{field}: {message}"`;
  `BadRequest` renders its message verbatim; `Db` and `Internal` are opaque to the client.
  A service's message reads correctly on both the web page and in a tool result, so it carries no
  argument syntax such as "send null". The MCP tool and field descriptions carry the wire syntax.
- **A transaction that writes opens with `services::begin_write`.** It issues `BEGIN IMMEDIATE`,
  so the transaction holds SQLite's write lock before its first read, and the reads and checks
  that follow see the committed state. A transaction from plain `begin` holds a snapshot from its
  first read, and when the other program commits before its first write, SQLite fails it with a
  busy-snapshot error at once. A no-op `UPDATE` as the first statement does not fix that on the
  ten tables with an FTS5 trigger, which fail with "database is locked" without waiting out the
  busy timeout. A race test opens several pools on one file (`test_support::race_pools`) and
  fails when the transaction opens with plain `begin`.
- **Stamps are set by code, never by the database.** No stamp column has a default. An entity
  fills `created_at` and `updated_at` on insert (`stamp_on_insert!` in `entities/mod.rs`), and
  every update function sets `active.updated_at = Set(services::now())`, because SeaORM
  `ActiveModel::update()` does not set it. A bulk insert (`insert_many`) or raw SQL bypasses the
  entity and sets both stamps itself.
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

- **`git rm --cached` on a tracked file deletes it from every other checkout** when that checkout
  pulls the change. Restore with `git show <commit-before>:<path>` if the files are still wanted.
- **Issue tracking**: Use `bd` (beads), never markdown TODOs.
- **Design decisions**: a design or architecture decision is recorded as an ADR in `docs/adr/`
  (`NNNN-short-name.md`: status, date, context, decision, alternatives, consequences). Brainstorming
  specs and task plans under `docs/superpowers/` are working files that git ignores, so new
  text points at the ADR and never at one of them.
- **Currency**: Stored as integer cents (`i32`). Tool inputs and outputs are cents. Use integer
  division for formatting, not `as f64 / 100.0`.
- **Dates**: a calendar date is `chrono::NaiveDate` and an event time (`incidents.occurred_at`,
  `incident_followups.occurred_at`, `mileage_log.recorded_at`) is `dates::DayOrTime`, a day or a
  local date and time. Both are stored as SQLite TEXT (`YYYY-MM-DD`, `YYYY-MM-DD HH:MM:SS`). Text
  the owner types is read only by `dates::parse_date` and `dates::parse_day_or_time`, which the
  MCP `into_domain` conversions and the web forms call, so shared inputs and services never see a
  date as a string. A bookkeeping stamp (`created_at`,
  `updated_at`, `archived_at`, `generated_at`) is `DateTime<Utc>`, stored as RFC 3339 with whole
  seconds (`2026-11-14T12:00:00+00:00`) and printed in tool output as `2026-11-14T12:00:00Z`.
  `docs/adr/0001-typed-dates-and-timestamps.md` records why.
- **Vocabularies**: a value from a fixed list is an enum made by `vocabulary!` (`vocab.rs`) in
  the entity module that owns its column, such as `work_item::JobStatus` and
  `service_record::Payer`. The enum is stored, serialized and printed as one text; `Payer`,
  `Performer` and `Supplier` name the stored text `self` as the variant `Owner`. A tool argument
  names the enum, so its schema lists the values, and a web form reads text through
  `vocab::parse`. Explain a variant in a `//` comment or in the argument's description: the
  macro refuses a doc comment on a variant, because one turns the schema's `enum` list into
  `oneOf`. A query filter takes a variant (`Column::Status.eq(JobStatus::Planned)`), never a
  string literal. Adding a value is one line in the macro call, and every program must be
  rebuilt with it, because an older binary cannot read a row that holds the new value. Removing
  or renaming one needs a migration that rewrites the rows, placed after
  `m20261115_000001_vocabulary_values`, whose lists stay as written.
  An enum that no column stores lives in the service that uses it. Four of them are derived
  statuses (`recall::ShownRecallStatus`, `appointment::AppointmentStatus`,
  `appointment::AppointmentPhase` and `reminders::DueStatus`, which reminders and planned work's
  revisit share). The other four are
  arguments: `recall::RecallStatus`, which the recall service maps to and from the stored finding
  status, `service_record::CompletedWork`, `schedule::LinkMode` and `search::SearchScope`. A
  filter on a column never takes one of these.
  `docs/adr/0004-typed-vocabularies.md` records why.
- **Migrations**: new migrations go after `m20261118_000002_parts_search`; the
  `new-migration` skill has the templates. Use `Expr::cust()` for expressions inside builder queries; raw
  `execute_unprepared` only where no builder form exists (FTS5 DDL, cross-table copies).
- **Migrations are not transactional on SQLite** (sea-orm-migration only wraps Postgres), so write
  them rerun-safe: `IF NOT EXISTS` DDL, `INSERT OR IGNORE` or `NOT EXISTS`-gated DML,
  pragma-guarded ALTERs.
- **Migration tests** start from `schema::testing::migrated_to(N)` (the N migrations before the one
  under test), insert rows, then run `Migrator::up`; `count` and `fts_integrity` live there too.
- **A change to a shared builder in `migration/schema.rs`** must leave the SQL of migrations that
  already ran unchanged: compare `sqlite_schema` from a fresh database with a backup's.
- **SQLite cannot `DROP COLUMN` named in an FK clause**; use the table-rebuild pattern. To add a
  foreign key to an existing column, edit the stored table definition as
  `docs/adr/0003-foreign-keys-by-editing-the-table-definition.md` describes. For FTS5,
  build the index and its triggers with `schema::create_fts` (and `drop_fts` in `down`), then fill
  it from stored rows with `INSERT INTO fts_x(fts_x) VALUES ('rebuild')`; aggregating over `bm25()`
  arms needs a `MATERIALIZED` CTE because query flattening breaks `bm25`.
- **Verify data-moving migrations against a populated copy of `data/glovebox.db`**, not just
  `test_db()`. To migrate a copy, run `GLOVEBOX_DB_PATH=<copy> target/release/glovebox-mcp < /dev/null`
  (it migrates at start and exits at end of input; `glovebox backup` does not migrate). Compare
  `sqlite_schema` and `.dump` output from `main`'s build and the branch's on a pre-migration backup.
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
reason. `[workspace.lints.clippy]` in the root `Cargo.toml` denies the pedantic group for every
crate. Run `cargo clippy --workspace --all-targets --locked -- -D warnings` to verify zero
warnings; it lints test code too. Add a per-function `#[expect(..., reason = "...")]` for
unavoidable cases, such as a test that tells one long story. A test checks a list is empty with
`test_support::assert_empty`, which prints the list when it is not, in preference to a bare
`assert!(… .is_empty())`.

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
- Make every `bd` write, then run `bd export -o .beads/issues.jsonl`, before the `chore(beads):`
  commit; `bd dolt push` exports again and the commit misses the last change otherwise.

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
