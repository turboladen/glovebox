# ADR-0016: A state whose fields depend on each other is one enum

- **Status:** Accepted
- **Date:** 2026-11-19

## Context

Several tables hold groups of columns that describe one fact. Each column can be set on its own,
so the service code keeps them consistent by hand: it refuses some combinations, clears one field
when another changes and reads a status from several nullable columns. One CHECK constraint
exists, the one that gives a schedule item exactly one owner. The rest are guarded only where a
service function remembers to guard them, and some writes are not guarded at all:

- **A part's life cycle.** `parts.status` is `purchased`, `installed`, `replaced` or `returned`,
  and beside it sit three install columns, two replace columns, `received_on` and three return
  columns. `validate_status_change`, `validate_install_details`, `validate_coming_off`,
  `clear_install`, `clear_fitting` and the return-field clearing in `part::apply_update` keep them
  in step. Nothing stops a `purchased` part from getting install details through `record_part` or
  `correct_part`, or any part from getting replace details. `part::install` also leaves an earlier
  replacement's columns in place.
- **A need's decision.** `chosen_option_id` and `decided_on` are set together, and
  `decision_reason` is valid only with a chosen option or while the shop supplies the part.
  `holds_reason` encodes this, and `need::apply_update` adds a resent-reason exemption, because a
  stored row can hold a reason it could not take now. The `ON DELETE SET NULL` key on
  `chosen_option_id` clears the choice without its date or reason, so `delete_option` clears the
  choice itself first.
- **An option's history.** `rejected_reason` and `rejected_on` are written together, but
  `is_rejected` accepts either one. `chosen_reason` needs a `chosen_on`.
- **An appointment's ending.** `closed_on` and `cancelled_on` are never both set, because
  `put_away` refuses a second ending. `Ending::of` still decides what a row with both means, and
  `shows_as_open` reads the raw `closed_on` to tell a hand-closed appointment from one whose jobs
  are all done.
- **Single-row rules.** A job's `completed_service_id` is set only while the job is done. A
  service's `end_date` is not before its `service_date`, and a self-performed service names no
  shop, by `shop_name` or by `shop_id` (only the first is checked). A mileage reading with a
  `service_record_id` has source `service`, but a manual reading can claim that source. A
  vehicle-level schedule item is the only kind that overrides another, but `schedule::create`
  takes `overrides_item_id` from its caller. `research_findings.status` is free text, and a value
  outside `new`, `completed` and `dismissed` reads as open.

A read of the live database on 2026-11-19 found no row that breaks any of these rules: 103 parts,
17 needs, 24 options and 2 appointments, with every rule above checked. The hand-written guards
have held so far. The gaps are the writes they do not cover.

## Decision

**Entities stay flat.** SeaORM maps one field to one column, so the columns stay as they are. A
group of columns that describes one fact gets a Rust enum with data in its variants, in the
service that owns the table. The service builds the enum once from the row and writes it back
through one function that sets every column of the group, and no other code writes those
columns. A combination the enum cannot hold cannot be written. `appointment::Ending` is the start
of this, and it grows the dates into its variants.

**Inputs stay commands.** A change that moves a state names its target and carries what the
target needs, as `StatusChange` does for an appointment. A patch field whose legality depends on
another field becomes a variant of that command. A column that depends on nothing keeps its
`Option<Option<T>>` field.

**Output stays flat.** A summary serializes the same keys it does now, filled from the enum. The
MCP replies, the tool descriptions, the Claude Desktop project prompt and the web templates do
not change when an area converts.

**Each converted area adds CHECK constraints to its table.** One migration per area refuses to
finish while a row breaks a rule, as `m20261115_000001_vocabulary_values` does, and then adds the
constraints by editing the table's stored definition (ADR-0003). The enum is the guard every
service write goes through. The CHECK catches raw SQL, a migration and a binary built before the
conversion.

**Rules that span rows or tables stay in services.** A closed appointment holding no planned job
(`settle_reopened_jobs`, `settle_reopened_todo`), `require_assignable`, a chosen option belonging
to its need and being choosable, and an archived vehicle refusing open work all read other rows.
Neither a constraint on one row nor an enum built from one row can state them.

The areas convert in this order, one pull request each:

1. **A part's life cycle.** The state is purchased, installed (with its service, date and odometer,
   each optional, because `delete_service` keeps a part fitted another way and the `ON DELETE SET
   NULL` key on `installed_service_id` clears the service), replaced (with the install details and
   the replace date and odometer) or returned (with its date, its reason and the need while that
   need exists). `received_on` stays a column of its own, because an installed or returned part
   keeps the date it arrived and a need's stage reads it. The command replaces the status, install
   and replace fields of `correct_part`. It retires the validators and clearing functions named
   above, and it refuses a `received_on` before `purchase_date`, which no write checks now. Its
   CHECK constraints: install columns only while installed or replaced, replace columns only while
   replaced, `returned_on` and `return_reason` exactly while returned, `returned_from_need_id` only
   while returned (its `ON DELETE SET NULL` key clears it when the need is deleted), and
   `received_on` not before `purchase_date`.
2. **A need's decision.** The state is undecided, shop-supplied (with an optional reason) or
   chosen (with the option, the date and an optional reason). An option's rejection and its kept
   choice each become one optional pair. This retires `holds_reason` and the date branches in
   `decide`, and narrows the resent-reason exemption to the call that moves a need off the shop. The `chosen_option_id` key changes to `ON DELETE RESTRICT`,
   so the choice, its date and its reason leave together. Its CHECK constraints: choice and date set
   together, a reason only with a choice or a shop supplier, the rejected pair together, and a
   kept reason only with its date.
3. **An appointment's ending.** The state is open, closed on a date or cancelled on a date.
   `shows_as_open` reads the variant. The CHECK: the two dates are never both set.
4. **The single-row rules.** These need no enum, only CHECK constraints: a job's completing
   service, a service's dates and shop, a mileage reading's source, a schedule item's override,
   and `research_findings.status` as a vocabulary column (ADR-0004).

These have no rule between their columns and stay as they are:

- **The quote fields on a job.** They are independent by design (ADR-0012).
- **A vehicle's sale details.** They outlast unarchiving.
- **An incident's `resolved` flag.** The owner can override what its links imply.
- **A to-do's `done_on`.** It is one optional date already.
- **A schedule item's `enabled`.** No other column depends on it.
- **The `*_currency` columns.** They carry defaults, so a currency with no amount is normal.

## Alternatives considered

- **Serialize the enum as a tagged object.** Each reply would carry only the fields its state
  has, but every MCP test that reads a key, every tool description that names one and the project
  prompt would change with each area.
- **Store the enum in the entity.** SeaORM maps a field to one column. An enum with data in its
  variants spans several, so every read and write would need hand-written conversion in the
  entity, which is the service's job here.
- **CHECK constraints only.** They stop a bad row at the write, but the services would still
  patch fields one by one and report the failure as an opaque database error.
- **Enums only.** Cheaper, with no edits to stored table definitions, but raw SQL and a migration
  could still write a row the enum cannot read. ADR-0004 turned CHECK constraints down for
  vocabulary columns because the enums were their only writers. Here the columns have several
  writers, and the definition edit happens once per area.

## Consequences

- An area's conversion changes no reply and no column, so an older binary reads every row. A
  CHECK refuses an older binary's write that breaks a rule, and the database is the better place
  for that error to surface.
- Building a summary from an enum adds a conversion in each direction, and a test of each area
  checks that the flat keys round-trip.
- A state that cannot be read from a row is a bug the conversion surfaces at load, so the load
  function returns `DomainError::Internal` and names the row.
- A new table whose columns depend on each other starts with its enum and its CHECK constraints.
