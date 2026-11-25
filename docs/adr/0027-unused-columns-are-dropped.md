# ADR-0027: Unused columns are dropped

- **Status:** Accepted
- **Date:** 2026-11-24

## Context

Six columns hold nothing any code reads:

- `service_records.shop_name` and `appointments.shop_name` are NULL in every row, and a CHECK on
  each table keeps them so (ADR-0021). The full-text index over services still lists the column.
- `research_reports.summary` and `raw_data` are written by the recall check, and `summary` by the
  first filed finding as well. `notes` has no writer. Nothing reads any of the three. `raw_data`
  is a JSON copy of the findings the same call stores as `research_findings` rows.
- `vehicle_attributes.superseded_by` has no writer and no reader.

Every entity lists the columns, so every fixture, insert and serialized record carries them, and a
reader of the schema cannot tell them from columns that matter. A service's `shop_id` also has no
index, and a shop's history reads services by it.

SQLite's `ALTER TABLE … DROP COLUMN` refuses a column that a CHECK, a foreign key, an index or a
trigger names. `shop_name` is named by two checks on `service_records`, one on `appointments` and
the three search triggers, and `superseded_by` by its own foreign key. The documented way around
that is to rebuild the table: create a copy, move the rows and drop the original. `service_records`
and `appointments` are parents of nine tables between them, connections run with foreign keys on,
and `DROP TABLE` runs each key's action, so a rebuild that mishandles the foreign-key pragma
deletes every invoice line or takes every job off its appointment (ADR-0003 describes the same
risk for `work_items`).

## Decision

**A column no code reads is dropped, with everything that names it**
(`m20261124_000001_drop_unused_columns`). The same migration adds `idx_service_records_shop`.

**A constraint that names the column leaves the stored table definition first, and the column is
then dropped with `ALTER TABLE … DROP COLUMN`. No table is rebuilt.** In one transaction that
holds the write lock, the migration:

1. Drops `fts_service_records` and its three triggers.
2. Edits the stored definitions through `schema::edit_definitions`, which compares
   `integrity_check` and `foreign_key_check` of the whole database before and after: the two
   `shop_name IS NULL` checks and the `superseded_by` foreign key are removed, and
   `service_records_self_performed_has_no_shop` reads `performed_by <> 'self' OR shop_id IS NULL`.
   SQLite's documentation names this edit for removing a constraint.
3. Refuses, with nothing changed, when a definition still names a dropped column anywhere but its
   declaration, and then drops each column.
4. Creates `fts_service_records` and its triggers over `description` and `notes` with
   `schema::create_fts` and fills the index with `rebuild`.

`DROP COLUMN` rewrites the rows where they are. Every row keeps its id, the tables are never
dropped, so no foreign key action runs, and every index, trigger, CHECK and key the migration does
not name stays as stored. The remaining columns keep their order, and each entity lists them in
that order.

A shop is found by a search through `fts_shops`, so search needs no change.

## Alternatives

- **Rebuild each table.** It works for any change and does not depend on how a definition is
  spelled. It is rejected for the reason ADR-0003 gives: the parent tables have cascading children,
  and the rebuild must also restate every column, index and trigger by hand, where a mistake
  changes a table the entities read by position.
- **Leave the columns.** It costs no migration, but each new fixture, test and reader of the
  schema keeps paying for columns that mean nothing.
- **Keep `raw_data` as an audit copy of what NHTSA sent.** It holds what the code built from the
  reply, not the reply, and the findings table holds the same fields.

## Consequences

- The values the dropped columns held are gone. On the owner's database as of the 2026-11-24
  backup, an upgrade loses one report's 31-character summary and its 5,192-character JSON copy of
  five findings, whose five `research_findings` rows remain. Every other dropped column held only
  NULL. `db::connect` copies the database to `backups/` before the migration runs, and that copy
  keeps the dropped values.
- `down` changes nothing, because the values cannot be restored.
- A Claude Desktop server that is still running when another program applies the migration fails
  every read and write of services and appointments, because its queries name `shop_name`. The
  owner quits Claude Desktop, runs `just install` and starts it again, before or right after any
  other glovebox program has opened the database.
- A program built before this migration refuses to open a migrated database and says to rebuild,
  and its inserts would name columns that are gone. Every program is rebuilt before the database
  is opened by a new one, in the order `docs/claude-desktop/README.md` gives under "Upgrading".
- The edit matches the spelling `schema::add_checks` and the initial schema store. A database
  whose definition is spelled another way fails the migration with nothing changed.
- Every migration must still run again over the finished schema, as after a crash, and two
  earlier ones name `shop_name`. `schema::add_checks` tests stored rows only against a check
  the table lacks, and `m20261121_000002_shops_from_names` does nothing when the columns are
  gone. A migration that drops a column gives each earlier migration that names it the same
  kind of guard.
- ADR-0021's rule that `shop_name` stays as an always-NULL column ends here. Reads still print a
  shop's name under the `shop_name` key, taken from the directory.
- A later column that nothing reads is dropped the same way: plain `DROP COLUMN` when nothing
  names it, and the definition edit first when a constraint does.
