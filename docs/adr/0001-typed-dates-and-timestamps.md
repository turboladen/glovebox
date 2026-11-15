# ADR-0001: Typed dates and timestamps

- **Status:** Accepted
- **Date:** 2026-11-14

## Context

Every date and timestamp in `glovebox-shared` was a `String` over SQLite TEXT. Dates were
compared, sorted and sliced as text, and validators existed mainly to keep that text sortable.
Three columns (`incidents.occurred_at`, `incident_followups.occurred_at`,
`mileage_log.recorded_at`) held either a bare date or a local date and time, so code that read
them split and padded strings to compare them.

The database holds the owner's real records. SQLite migrations are not transactional here, and
connections run with foreign keys on, so `DROP TABLE` runs the children's `ON DELETE CASCADE`.

Two facts about sqlx constrain the choice. It reads a calendar date strictly (`%F`), so one
malformed stored date makes its whole row unreadable. It writes `DateTime<Utc>` as RFC 3339, with
fractional seconds when the value has them, which is a different text from what
`datetime('now')` writes.

## Decision

**Each value has a type that says what it is.**

- A calendar date is `chrono::NaiveDate`, stored as `YYYY-MM-DD`.
- An event time is `dates::DayOrTime`: a day, or a date and time on the owner's own clock. It is
  stored and serialized as `YYYY-MM-DD` or `YYYY-MM-DD HH:MM:SS`, and a bare day orders before
  any time on that day, as the stored text does.
- A bookkeeping stamp (`created_at`, `updated_at`, `vehicles.archived_at`,
  `research_reports.generated_at`) is `chrono::DateTime<Utc>`, stored as RFC 3339 with whole
  seconds (`2026-11-14T12:00:00+00:00`).

**Text is parsed once, at the edge.** `dates::parse_date` and `dates::parse_day_or_time` are the
only readers of a date the owner types. The MCP `into_domain` conversions and the web forms call
them, so shared inputs and services never hold a date as a string. Tool input stays text with the
same descriptions, and a malformed date is a tool error that starts with the field's name.

**A migration refuses a value it cannot read.** `m20261114_000001_typed_dates` stops, naming the
table, column and count, while any date column holds a value that is not a real `YYYY-MM-DD`. A
refused start is better than a row that cannot be read later.

**No stamp column has a default.** Code sets both stamps on every insert, because a row written
by `datetime('now')` would sort before every RFC 3339 row.

**The defaults are removed by editing the schema text, not by rebuilding tables.** SQLite
documents this procedure for a change that does not alter stored rows, and names changing a
column default as one: inside a transaction, set `PRAGMA writable_schema`, edit
`sqlite_schema.sql`, bump `PRAGMA schema_version`, clear the pragma and run
`PRAGMA integrity_check`. The stored clause is exactly ` DEFAULT (datetime('now'))` in every
table, so the edit is one `replace()`, and a second run finds nothing to replace.

## Alternatives considered

- **Keep the stored stamp text behind a custom type.** No row is rewritten and the defaults stay
  correct, but the stored format stays one that only this project's type understands.
- **RFC 3339 stamps with the defaults left in place and never used.** Any later raw insert that
  leans on a default writes the other format and sorts wrong.
- **Rebuild the 19 tables to drop the defaults.** It restates every table, index and search
  trigger, needs foreign keys off on a pinned connection, and a mistake cascades deletes from
  `vehicles` down.
- **Leave the event-time columns as strings, or make them dates.** The first keeps the string
  helpers alive; the second drops a time the owner recorded.

## Consequences

- Date logic is arithmetic on dates. The string slicing, padding and re-parsing helpers are gone.
- Date output is unchanged (`YYYY-MM-DD`). Stamps print as `2026-11-14T12:00:00Z`.
- A malformed date is refused before any lookup, so a call with a bad date and an unknown id
  reports the date.
- A migration that inserts rows sets both stamps itself.
- A column's declared type stays `text`. SQLite does not care, and changing it needs the rebuild.
- A data-moving migration is checked against a populated copy of a backup: row counts and a hash
  of every untouched column must match before and after.
