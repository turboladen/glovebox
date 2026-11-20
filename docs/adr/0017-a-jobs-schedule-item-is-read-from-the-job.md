# ADR-0017: A job's schedule item is read from the job

- **Status:** Accepted
- **Date:** 2026-11-19

## Context

A service gives a schedule item its reminder baseline in two ways. The invoice can name the item
(`schedule_item_ids` on `record_service`, `correct_service` and `link_service_to_maintenance`),
which stores a row in `service_schedule_links`. Or the service can complete a planned job that
was filed against the item (`work_items.schedule_item_id`).

A stored link holds a service id and a schedule item id and nothing else. A link written for a
completed job would be the same row as a link the invoice named, so nothing could tell them apart
when `correct_service` takes the job off the service. Removing the link would drop an item the
invoice named, and keeping it would leave the reminder counting from work the owner said did not
happen.

## Decision

**A job's schedule item is read from the job and never stored as a link.** A service satisfies
the schedule items its stored links name and the schedule item of each job it completes.
`schedule::links_of_services` and `schedule::links_to_items` return that union, and every reader
goes through one of them: a service's `schedule_item_ids`, the service list, and the reminder
baseline.

No stored link names an item that a job of the same service brings.
`schedule::drop_links_jobs_bring` removes such a link, and each write that could create one ends
with it: storing links (`schedule::insert_service_links`), completing jobs
(`work_item::complete_with_service`) and changing a completed job's schedule item
(`work_item::update`). It compares the items the two count toward (ADR-0020), so a link to an
inherited row repeats a job on the vehicle's own copy. A service read lists a job's item beside the named ones, so a caller that sends the list back
stores nothing for it.

## Alternatives considered

- **A column on the link that names the job.** The link's primary key is the service and the
  item, so one row cannot say that the invoice named the item and a job also brought it. A key
  that could say both needs the table rebuilt.
- **Storing the link and removing it when the job is taken off.** The stored row cannot say
  whether the invoice also named the item, and a list sent back from a read would turn a job's
  item into a named one.
- **Asking the owner to correct `schedule_item_ids` in the same call.** The reminder stays wrong
  whenever the second list is forgotten, and the reply gives no sign of it.

## Consequences

- Taking a job off a service takes its schedule item's baseline with it, after either answer to
  `completed_work`, and deleting the service does the same. The other items the invoice named
  stay. This replaces the last consequence of ADR-0005.
- An item named in `schedule_item_ids` beside a job for the same item follows the job: it leaves
  the service when a correction takes the job off, when the job's status is set back to planned
  or to dropped, and when the job is given another schedule item. A correction that takes a job
  off ignores that job's item in the `schedule_item_ids` it sends, because the list may be a
  read sent back. When the invoice still covered the item, a later call names it.
- Deleting a completed job is the one exception. It removes the job's record and says nothing
  about the work, so the service keeps the item as a stored link. `done_another_way` differs
  because it says another event did the work.
- A job's schedule item counts for as long as the service completes the job. A
  `schedule_item_ids` list replaces the items the invoice names and cannot remove a job's item;
  the job is taken off through `work_item_ids`.
- `m20261120_000001_job_schedule_links` removes the stored links that repeat a completed job's
  item. Each service satisfies the same schedule items before and after it runs.
- An incident has the same shape, and ADR-0018 records its rule, which differs: an incident the
  invoice named beside a job stays resolved when the job is taken off.
