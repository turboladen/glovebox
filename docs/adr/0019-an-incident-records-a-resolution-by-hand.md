# ADR-0019: An incident records a resolution by hand

- **Status:** Accepted
- **Date:** 2026-11-19

## Context

An incident reads resolved when its `resolved` flag is set. Two kinds of write set it: a service
resolves the incident (an invoice names it, or a job filed from it is completed), or the owner
marks it resolved with `update_incident` and `resolved: true`. The flag does not say which.

`incident::reopen_resolved` runs when a correction or a delete undoes a service's work with
`did_not_happen`. It treated "no other service resolved the incident" as proof that the service
being undone had resolved it. An incident the owner had resolved by hand, with a job filed from it
completed by a service afterwards, was therefore reopened, and the refusal that asks for
`completed_work` named it. ADR-0018 listed this as an accepted limit.

## Decision

**An incident carries `resolved_by_hand`, set when the owner's word is what resolved it.** The
column is true only on a resolved incident (a CHECK enforces this). One private enum in
`services/incident.rs`, `Resolution` (`Open`, `ByHand`, `ByService`), is the only value behind the
two columns, and one function writes both.

**When the marker is written.**

- `update_incident` with `resolved: true` writes `ByHand` when that call is what changes the
  incident from unresolved to resolved. The check runs after the call's link changes, so
  `resolved: true` sent together with a `resolved_by_service_ids` that resolves the incident, or
  sent on an incident that is already resolved (a read sent back), writes no marker. When the same
  call removes every resolver, no service stands behind the flag, so `resolved: true` marks it.
- A service that resolves an incident (a `resolved` link, `mark_resolved` for a completed job)
  writes `ByService`, and only on an unresolved incident. On a resolved incident it changes
  nothing, so a marker survives a job completed later.
- `create_incident` has no `resolved` argument. A new incident is open, and it reads resolved only
  through a service named in `resolved_by_service_ids`, which writes `ByService`. No incident is
  created marked.
- Every write of unresolved clears the marker: `resolved: false`, a call that clears every
  resolver, and `reopen_resolved`.

**What the marker protects.** `incident::resolved_by_service` leaves out an incident with the
marker, so the work a correction or a delete would undo never lists it, the `completed_work`
refusal does not name it, and `reopen_resolved` never receives it. `reopen_resolved` also skips such
an incident itself. After `did_not_happen` the job returns to planned and the incident stays
resolved and marked. The owner reopens it with `resolved: false`.

**How existing records read after the upgrade.** The migration
`m20261120_000003_incident_resolved_by_hand` marks each resolved incident that no service resolves:
no stored `resolved` link and no completed job filed from it. A resolved incident with a resolving
service stays unmarked and behaves as before, so an incident resolved by hand before a job was
completed, in records older than this migration, can still be reopened by `did_not_happen`.

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
- A hand marker survives a job completed later, an undo of that job's service, and a link named in
  `resolved_by_service_ids`. It ends only when the owner sends `resolved: false` or clears every
  resolver.
- Setting `resolved: true` on an incident a service resolved leaves it unmarked, so undoing that
  service reopens it.
