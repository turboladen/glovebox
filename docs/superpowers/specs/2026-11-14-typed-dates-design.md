# Typed dates and timestamps

## Intent

Every date and timestamp in `glovebox-shared` is a `String` over SQLite TEXT, so dates are
compared, sorted and sliced as text and validators exist to keep that text sortable. This change
gives each value a type that says what it is: a calendar date, a UTC instant, or an event time
the owner may know only to the day.

## Goals

- A calendar date is `chrono::NaiveDate` in entities, inputs, service parameters and outputs.
- A bookkeeping stamp (`created_at`, `updated_at`, `vehicles.archived_at`,
  `research_reports.generated_at`) is `chrono::DateTime<Utc>`, stored as RFC 3339 with whole
  seconds (`2026-11-14T12:00:00+00:00`).
- An event time (`incidents.occurred_at`, `incident_followups.occurred_at`,
  `mileage_log.recorded_at`) is `DayOrTime`: a day, or a local date and time.
- No stamp column has a default. Code sets both stamps on every insert.
- The owner's records come through unchanged apart from the stamp rewrite.

## Non-goals

- Replacing string whitelists with enums (a separate issue).
- Injecting a clock into shared services.
- Changing a column's declared type from `text`.

## Anti-goals

- A migration that can leave a row unreadable, or that deletes or reorders records.
- A tool that refuses a date it accepts today, or reports a bad date with a different message.
- A table rebuild. With foreign keys on, `DROP TABLE` runs the children's `ON DELETE CASCADE`.

## Constraints

- sqlx reads `NaiveDate` strictly (`%F`). A malformed stored date makes its row unreadable, so
  the migration refuses to finish when it finds one, and names the table, column and count.
- sqlx writes `DateTime<Utc>` with fractional seconds when the value has them, so
  `services::now()` truncates to whole seconds and every stored stamp has one width.
- SQLite migrations are not transactional here. Each migration is safe to run again, and the
  timestamp migration runs inside its own transaction.
- Tool input stays a string with the same descriptions. A bad date is a tool error that starts
  with the field's name and carries the same sentence as today.
- Tool output: dates print as `YYYY-MM-DD` as before. Stamps print as `2026-11-14T12:00:00Z`.

## Approaches

**Stamp storage.** Three were considered: keep the stored text behind a custom type; RFC 3339
with the defaults left in place but never used; RFC 3339 with the defaults removed. The owner
chose the third, which matches healthie's ADR-0003.

**Removing the defaults.** Rebuilding 19 tables restates every table, index and search trigger
and needs foreign keys off on a pinned connection. SQLite documents a lighter procedure for a
change that does not alter stored rows, and names changing a column default as one: inside a
transaction, set `PRAGMA writable_schema`, edit `sqlite_schema.sql`, bump
`PRAGMA schema_version`, clear the pragma and run `PRAGMA integrity_check`. The stored clause is
exactly ` DEFAULT (datetime('now'))` in every table, so the edit is one `replace()`, and a
second run finds nothing to replace. A spike confirmed it through sqlx on a WAL file with a
second connection open. This design uses the schema edit.

**Event times.** Leaving them as strings keeps the string helpers alive; making them dates
drops a time the owner recorded. `DayOrTime` keeps both forms and stores the same text.

## Design

### Types

- `glovebox_shared::dates::DayOrTime` is an enum, `Day(NaiveDate)` or `Time(NaiveDateTime)`.
  It parses `YYYY-MM-DD`, `YYYY-MM-DD HH:MM:SS` and `YYYY-MM-DDTHH:MM:SS`, prints the first two,
  serializes as that string, and is stored through SeaORM's string-backed `DeriveValueType`.
  `day()` gives its calendar date. It orders as the stored text does: a bare day sorts before
  any time on that day.
- `services::parse_date(field, &str) -> DomainResult<NaiveDate>` and
  `services::parse_day_or_time(field, &str) -> DomainResult<DayOrTime>` are the only parsers of
  owner input. They refuse anything not zero-padded, with the sentences in use today.
- `services::now() -> DateTime<Utc>` (whole seconds), `services::today() -> NaiveDate` and
  `services::local_now() -> DayOrTime` are the clock.

### Layers

- Entities, shared inputs and service parameters carry the types. Outputs carry them where the
  value is a date; `MonthlyCost.month` and `HomeAppointment.when` stay strings.
- MCP schema structs keep `String` date fields. `into_domain` returns `DomainResult` and calls
  the shared parsers. The web order form does the same.
- An entity with stamps fills `created_at` and `updated_at` in `before_save` on insert when the
  caller left them unset. Update functions keep stamping `updated_at` themselves.

### Migrations

1. `m20261114_000001_typed_dates` writes a space for the `T` in the three event-time columns,
   then refuses to finish if any date column holds a value that is not a real `YYYY-MM-DD`, or
   any event-time column a value that is neither form.
2. `m20261114_000002_utc_timestamps`, in one transaction: refuse a stamp that is neither the
   old shape nor the new; rewrite the old shape with `replace(c, ' ', 'T') || '+00:00'`; remove
   the defaults by the schema edit; check integrity and foreign keys.

### Delivery

Two pull requests. The first carries the date and event-time types and migration 1, and changes
no valid stored value. The second carries the stamps and migration 2.

## Verification

Each migration is run against a populated copy of a backup in a scratch directory: row counts
and a checksum of every non-stamp column must match before and after, the stamps must all have
the new shape, and search must still answer. `data/glovebox.db` is opened only by the owner.

## Left to the builder

The exact set of output structs that take `NaiveDate`, the test helpers, and how
`activity`'s mixed-granularity ordering is expressed on `DayOrTime`.
