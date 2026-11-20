# ADR-0019: An incident records a resolution by hand

- **Status:** Accepted
- **Date:** 2026-11-19

## Context

An incident reads resolved when its `resolved` flag is set. Two kinds of write set it: a service
resolves the incident (an invoice names it, or a job filed from it is completed), or the owner
says so with `update_incident` and `resolved: true`. The flag alone does not say which.

A correction or a delete that undoes a service's work with `did_not_happen` reopens the incidents
that service resolved. With the flag alone, an incident that no other service resolves looks like
one the undone service resolved, even when the owner had resolved it by hand before a job filed
from it was completed.

## Decision

**An incident carries `resolved_by_hand`, set when the owner's word is what resolved it.** The
column is true only on a resolved incident (a CHECK enforces this). One private enum in
`services/incident.rs`, `Resolution` (`Open`, `ByHand`, `ByService`), is the only value behind the
two columns, and one function writes both.

**When the marker is written.**

- `update_incident` with `resolved: true` on an open incident writes `ByHand`, unless the same
  call names a service in `resolved_by_service_ids`, which writes `ByService`. A completed job
  that still names a service does not change this: the owner reopened the incident after that
  job, so closing it again is the owner's word.
- `resolved: true` on a resolved incident writes `ByHand` only when no service resolves it, so a
  read sent back marks nothing.
- **A resolved incident that no service resolves is marked.** A write that takes the last
  resolving service away and leaves the incident resolved marks it: work taken off a service as
  `done_another_way`, and an edit of a completed job (set back to planned or dropped, or given
  another incident).
- A service that resolves an incident (a `resolved` link, `mark_resolved` for a completed job)
  writes `ByService`, and only on an unresolved incident. On a resolved incident it changes
  nothing, so a marker survives a job completed later.
- `create_incident` has no `resolved` argument. A new incident is open, and it reads resolved only
  through a service named in `resolved_by_service_ids`, which writes `ByService`. No incident is
  created marked.
- Every write of unresolved clears the marker: `resolved: false`, and a service's work undone as
  `did_not_happen` on an unmarked incident. An emptied `resolved_by_service_ids` marks an
  unmarked incident unresolved and leaves a marked one resolved.

**What the marker protects.** `incident::resolved_by_service` leaves out an incident with the
marker, so the work a correction or a delete would undo never lists it and the `completed_work`
refusal does not name it. `incident::release_resolved` changes only unmarked incidents. After
`did_not_happen` the job returns to planned and the incident stays resolved and marked. The owner
reopens it with `resolved: false`.

**How existing records read after the upgrade.** The migration
`m20261120_000003_incident_resolved_by_hand` marks each resolved incident that no service resolves:
no stored `resolved` link and no completed job filed from it. A resolved incident with a resolving
service stays unmarked, so an incident resolved by hand before a job was completed, in records
older than this migration, can still be reopened by `did_not_happen`. The marking matches the
rule every write keeps, so running the migration again changes nothing.

## Alternatives considered

- **A `resolved_on` date compared with the job's completion.** It would tell whether the flag was
  set before or after the job, but it needs a second column with a rule to keep it in step, and the
  comparison fails for an incident resolved, reopened and resolved again.
- **Marking nothing in existing rows.** The column would start empty, so every incident the owner
  resolved by hand before the upgrade could still be reopened by an undo. Marking the ones no
  service resolves protects them when a job filed from one is completed later, and only the owner
  can have resolved them.

## Consequences

- `update_incident` and every incident read carry `resolved_by_hand`.
- A hand marker survives a job completed later, an undo of that job's service, a link named in
  `resolved_by_service_ids` and an emptied list. It ends only when the owner sends
  `resolved: false`.
- Setting `resolved: true` on an incident a service resolves leaves it unmarked, so undoing that
  service reopens it.
- A service whose only recorded work is an invoice-named incident that carries the marker is
  deleted without a `completed_work` question, and the incident stays resolved.
- ADR-0016 lists the incident's resolution among the converted areas.
