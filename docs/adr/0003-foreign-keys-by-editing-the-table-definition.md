# ADR-0003: Add a foreign key by editing the stored table definition

- **Status:** Accepted
- **Date:** 2026-11-14

## Context

A job names the schedule item, research finding or incident it comes from in
`work_items.schedule_item_id`, `research_finding_id` and `incident_id`, and the home page links a
due item, recall or incident to its planned job through these ids. Each column needs a foreign
key: a plain integer column keeps the id of a row after that row is deleted, and the home page
then shows a link to the wrong job.

SQLite cannot add a foreign key to an existing column with `ALTER TABLE`. It offers two ways to
change a table's constraints: rebuild the table, or edit the `CREATE TABLE` text it stores in
`sqlite_schema` under `PRAGMA writable_schema`.

`work_items` is expensive to rebuild safely:

- Connections run with foreign keys on, and `DROP TABLE` runs an implicit `DELETE`. Dropping
  `work_items` would cascade into `job_needs` and delete every need. Avoiding that takes
  `PRAGMA foreign_keys = OFF`, which SQLite ignores inside a transaction, so it must be set on one
  pooled connection outside the transaction, and a failure returns that connection to the pool
  with enforcement off.
- A rebuild drops the table's three search triggers and two indexes, which must be created again,
  and must reproduce the physical column order the entity reads, including `appointment_id`,
  which a later migration appended.

A foreign key exists only in the stored definition. Adding one changes no stored row.

## Decision

**A migration that adds a foreign key to an existing column edits the stored table definition,
and does not rebuild the table**, when the table has dependents that a rebuild would put at risk
(cascading children, triggers, a search index).

`m20261114_000003_work_item_link_foreign_keys` is the model. Such a migration:

1. Runs in one transaction that takes the write lock first.
2. Makes the stored rows satisfy the new key before adding it, because SQLite does not check
   them. A dangling id is set to NULL, the row is kept and its `updated_at` is stamped, and the
   count is logged as a warning.
3. Edits `sqlite_schema` with `PRAGMA writable_schema` on, turns the pragma off whether or not
   the edit worked, and raises `PRAGMA schema_version` so every connection reloads the schema.
   `migration::schema::edit_definitions` does this step and the comparison in step 4.
4. Judges the edit by what SQLite reads back (`pragma_foreign_key_list`), and by
   `PRAGMA integrity_check` and `PRAGMA foreign_key_check` on the edited table compared with their
   results before the edit. Any difference returns an error, which rolls the transaction back.
5. Skips the edit when the keys are already present, so it is safe to run again.

## Alternatives

- **Rebuild the table** (the twelve-step procedure in SQLite's `ALTER TABLE` documentation). It is
  the procedure SQLite documents for adding a constraint, and it does not depend on how the stored
  definition is spelled. It is rejected for this table because a mistake in the foreign-key
  pragma handling deletes the owner's needs, and because it replaces the triggers and indexes
  that the edit leaves alone. It stays the right choice for a change that alters stored rows,
  such as dropping or retyping a column.
- **Keep the columns plain and clear them in each delete function.** Every function that deletes
  a schedule item, finding or incident has to remember the cleanup, and a cascade from a vehicle,
  platform or report delete reaches none of those functions.

## Consequences

- SQLite's documentation names the definition edit for removing a constraint and for changing a
  default, and does not name adding a foreign key. The edit is safe on the same ground (no stored
  row changes), but the migration carries the proof itself in steps 2 and 4.
- The edit matches one spelling of the column (`"incident_id" integer,`). A database whose
  definition is spelled another way fails the migration with nothing changed, and needs a
  migration written for its spelling.
- The foreign key clears a job's column without setting the job's `updated_at`. Code that needs
  the stamp to move must update the job itself.
- A corrupt definition would make the table unreadable, so the migration is run against a copy of
  the real database before it merges, and `db::connect` takes a backup before any pending
  migration runs.
