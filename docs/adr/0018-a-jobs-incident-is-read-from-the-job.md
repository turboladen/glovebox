# ADR-0018: A job's incident is read from the job

- **Status:** Accepted
- **Date:** 2026-11-19

## Context

A service resolves an incident in two ways. The invoice can name the incident
(`resolved_incident_ids` on `record_service`, or `resolved_by_service_ids` on `update_incident`),
which stores a `resolved` row in `incident_service_links`. Or the service can complete a planned
job that was filed from the incident (`work_items.incident_id`).

A stored link holds an incident id, a service id and a role. A link written for a completed job
would be the same row as a link the invoice named, so nothing could tell them apart when
`correct_service` takes the job off the service. ADR-0017 met the same shape for schedule items.

## Decision

**A job's incident is read from the job and never stored as a link.** A service resolved the
incidents its stored `resolved` links name and the incident of each job it completes.
`incident::resolvers_of_services` and `incident::resolvers_of_incidents` return that union, and
every reader goes through one of them: a service's `resolved_incidents`, an incident's
`resolved_by`, the work a correction or a delete would undo, and the check for another service
that also resolved the incident. Completing a job marks its incident resolved
(`incident::mark_resolved`) and stores nothing.

**A stored link means the invoice named the incident, and it stays when the job leaves.** A
service can hold both: the stored link beside a job filed from the same incident. Taking the job
off then changes nothing about the incident, because the invoice still says the service fixed
it.

This is where incidents differ from ADR-0017, in which an item named beside its job follows the
job. A schedule item named beside its job is usually a read sent back, and the reminder is
derived, so dropping the link loses nothing the owner cannot see. An incident's `resolved` flag
is stored, and reopening an incident the invoice names is a wrong record.

## Alternatives considered

- **The rule of ADR-0017: drop a stored link that repeats a job's incident.** One rule for both
  link kinds, but taking the job off would reopen an incident the invoice resolved.
- **A column on the link that names the job.** The link's primary key is the incident, the
  service and the role, so one row cannot say that the invoice named the incident and a job also
  brought it.
- **Storing the link and removing it when the job is taken off.** The stored row cannot say
  whether the invoice also named the incident.

## Consequences

- `correct_service` taking a job off releases the job's incident only when no other job of the
  service came from it and the invoice did not name it. After `did_not_happen` the incident
  reads unresolved unless another service resolved it. After `done_another_way` it stays
  resolved and the service stops being one of its resolvers. No link is written or removed.
- `update_incident` replaces the stored links of the services no job brings. A service that a
  job brings stays in `resolved_by` whatever the list says and keeps the link it has: a stored
  one stays, and a list sent back from a read stores none. An empty list leaves the incident
  resolved while a job still resolves it.
- Deleting a completed job removes the job's record and says nothing about the work, so the
  service keeps the incident as a stored link (`incident::keep_resolver`), as it keeps the
  schedule item. No link is stored while another job of the service came from the incident.
- Setting a completed job back to planned or dropped, or giving it another incident, with
  `update_planned_work` takes the service out of the incident's resolvers and leaves the
  `resolved` flag alone. Only a correction of the service says whether the work happened, and a
  later correction or delete of that service does not reopen the incident, because the service
  is not one of its resolvers. Giving a completed job an incident marks that incident resolved.
- A service that a job brings is ignored in the list `update_incident` replaces, so a list that
  names only such services leaves the `resolved` flag alone. The owner sets the flag with
  `resolved`. `update_planned_work` names the incident and the service in
  `released_incident_id` and `released_from_service_id` when a job edit takes the service out of
  the incident's resolvers.
- Once a job brings a service, no call stores that the invoice also named the incident:
  `update_incident` takes the service as a read sent back, and `correct_service` has no incident
  list. This matters for the links the migration removes. An incident list on `correct_service`
  is tracked as its own issue.
- `m20261120_000002_job_incident_links` removes the stored `resolved` links that repeat a
  completed job's incident. Such a row cannot say whether the invoice named the incident, so it
  is taken as the job's: each incident reads resolved by the same services before and after, and
  taking such a job off reopens its incident.
- A job completed before this decision whose service held no `resolved` link to the job's
  incident (the owner removed it, or the job was given the incident afterwards) reads as a
  resolver of that incident from the upgrade on, whatever the incident's `resolved` flag says.
  The flag is not changed. Clearing the job's incident takes the service off again.
- One limit is accepted. `did_not_happen` reopens an incident that the owner marked resolved by
  hand before the job was completed, when no other service resolved it. Nothing stored says the
  flag was set before the job, and the refusal names the incident before the answer is given.
