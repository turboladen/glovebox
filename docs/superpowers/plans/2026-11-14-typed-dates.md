# Typed Dates and Timestamps Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `String` dates and timestamps in `glovebox-shared` with `NaiveDate`, `DayOrTime` and `DateTime<Utc>`, and move the stored stamps to RFC 3339 with no column defaults.

**Architecture:** Two pull requests. PR 1 (tasks 1 to 5) types the dates and event times and adds a migration that changes no valid row. PR 2 (tasks 6 to 8) types the stamps and adds the migration that rewrites them and removes the defaults by editing the schema text.

**Tech Stack:** Rust, SeaORM 2.0 over SQLite (sqlx), chrono, rmcp, axum and askama.

**Spec:** `docs/superpowers/specs/2026-11-14-typed-dates-design.md`

## Global Constraints

- Never open, copy over or run anything against `data/glovebox.db`. Verification uses a copy of a file from `data/backups/` in the scratch directory.
- New migrations go after `m20261113_000002_option_choice_history` and are safe to run twice.
- `glovebox-web` never names SeaORM; `glovebox-shared` never names axum or rmcp (`just check-layering`).
- A bad date is refused with `"{field}: must be a date like 2027-05-01"`, or for an event time `"{field}: must be a date like 2027-05-01 or a date and time like 2027-05-01 14:30:00"`.
- Tool input fields and their doc comments do not change. Date output stays `YYYY-MM-DD`.
- `just fmt` before each commit; `just ci` is the gate. `.beads/issues.jsonl` is never staged on this branch.
- Comments are full sentences, state what holds now, and no `//` block passes 10 lines.

## Review Focus

- A stored date that is well-shaped but not a real day (`2026-02-30`): the migration refuses and names it.
- A bare-day event time and a timed one on the same day: every list orders them as it does today.
- `service_date` range filters at both ends: `from` and `to` are inclusive.
- An insert path that never passes through `before_save` (`insert_many`, raw SQL): both stamps are set.
- A database that already holds RFC 3339 stamps, or a mix: the second migration run changes nothing.

---

### Task 1: `DayOrTime` and the parsers

**Files:** Create `glovebox-shared/src/dates.rs`; modify `glovebox-shared/src/lib.rs`, `glovebox-shared/src/services/mod.rs`.

**Interfaces (produces):**
- `pub enum DayOrTime { Day(NaiveDate), Time(NaiveDateTime) }` with `FromStr`, `Display`, `Serialize`, `Deserialize`, `Ord`, `DeriveValueType` (`value_type = "String"`), and `pub fn day(self) -> NaiveDate`.
- `pub fn parse_date(field: &str, value: &str) -> DomainResult<NaiveDate>`
- `pub fn parse_day_or_time(field: &str, value: &str) -> DomainResult<DayOrTime>`
- `pub fn local_now() -> DayOrTime`

- [ ] Tests first, in `dates.rs`: both forms round-trip through `to_string`; a `T` form prints with a space; `2026-1-5`, a leading space, `2026-02-30` and `2026-01-05 25:00:00` are refused by the parsers with the sentences above; `Day(d) < Time(d 00:00:00)`; JSON is the printed string.
- [ ] Run `cargo test -p glovebox-shared dates` and see them fail to compile.
- [ ] Implement, run, commit.

### Task 2: Migration `m20261114_000001_typed_dates`

**Files:** Create `glovebox-shared/src/migration/m20261114_000001_typed_dates.rs`; modify `migration/mod.rs`.

- [ ] Tests first (in the migration file, on `Database::connect("sqlite::memory:")` migrated up to the previous migration, then rows inserted by raw SQL): a `T` event time is rewritten with a space; a valid populated database is otherwise unchanged; `service_records.service_date = '2026-02-30'` makes `up` return an error naming `service_records.service_date`; a second `up` changes nothing.
- [ ] Implement: one table of `(table, column)` for dates and one for event times; the guard is `c IS NOT NULL AND date(c) IS NOT c` for dates.
- [ ] Run, commit.

### Task 3: Entities, inputs and services

**Files:** every file in `glovebox-shared/src/entities` and `inputs` that holds a date; `services/*.rs`; `seed.rs`; `test_support.rs`.

**Interfaces (produces):** `test_support::day(&str) -> NaiveDate`; `test_support::days_ago(i64) -> NaiveDate`; `need::mark_received(.., on: Option<NaiveDate>)`, `need::link_part(.., received_on: Option<NaiveDate>)`, `todo::complete(.., on: Option<NaiveDate>)`, `service_record::list_between(.., from: Option<NaiveDate>, to: Option<NaiveDate>)`.

- [ ] Change the entity and input field types, then follow the compiler. Existing tests are the proof; their date literals become `day("…")` or `"…".parse().unwrap()`.
- [ ] Delete `validate_date`, `validate_optional_date`, `validate_datetime`, `today_label`, `local_now_stamp`, `costs::day_of`, `reminders::parse_date` and `activity::sort_key`'s string padding as each loses its last caller.
- [ ] `cargo test -p glovebox-shared` passes with the same test count as `main` plus the new ones. Commit.

### Task 4: MCP and web boundaries

**Files:** `glovebox-mcp/src/schemas.rs`, `schemas/checklist.rs`, `schemas/corrections.rs`, `handler/*.rs`; `glovebox-web/src/actions.rs`, `pages.rs`, `view.rs`.

- [ ] `into_domain` returns `DomainResult<…>` where the input holds a date, and each handler maps the error with `domain_error`. `malformed_dates_are_refused_naming_the_field` passes unchanged.
- [ ] The web order form parses `purchase_date` with `parse_date` and renders the same field error.
- [ ] `cargo test --workspace` passes with no test edited in `glovebox-mcp/tests` except literals passed straight to shared functions. Commit.

### Task 5: Docs, populated copy, PR 1

- [ ] Update the "DB datetimes" bullet and the service notes in `CLAUDE.md` and `AGENTS.md`.
- [ ] Run the populated-copy check from the spec; record the counts in the PR description.
- [ ] `just ci`, open the PR, review, merge on green.

### Task 6: Migration `m20261114_000002_utc_timestamps`

**Files:** Create `glovebox-shared/src/migration/m20261114_000002_utc_timestamps.rs`; modify `migration/mod.rs`.

- [ ] Tests first: a database with old-shape stamps comes out with every stamp matching `????-??-??T??:??:??+00:00`; no table's SQL contains `datetime(`; `PRAGMA integrity_check` is `ok`; an insert without stamps is refused; a stamp of another shape makes `up` fail naming the column and changes nothing; a second `up` changes nothing.
- [ ] Implement in one transaction as the spec lists. Run, commit.

### Task 7: Stamp types and insert stamping

**Files:** entities with stamps; `services/*.rs`; `seed.rs`; `db.rs` tests; `migration/m20261112_000002_seed_vw_mqb.rs` is not edited.

**Interfaces (produces):** `services::now() -> DateTime<Utc>`; each stamped entity's `before_save` fills unset stamps on insert.

- [ ] Test first: one test per entity module asserting a row created through its service has both stamps set and equal to whole seconds.
- [ ] Change the types, replace `now_stamp()`, set stamps by hand at `insert_many` and `col_expr` sites. Workspace tests pass. Commit.

### Task 8: Docs, populated copy, PR 2

- [ ] Update the `new-migration` skill, `CLAUDE.md` and `AGENTS.md`: no default on a stamp column, a migration's inserts set both stamps, the schema-edit procedure, the next migration name.
- [ ] Populated-copy check, `just ci`, PR, review, merge, close the bead on `main`.
