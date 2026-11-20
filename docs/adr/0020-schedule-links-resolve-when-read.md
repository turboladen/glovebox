# ADR-0020: Schedule links resolve when read

- **Status:** Accepted
- **Date:** 2026-11-19

## Context

A vehicle's schedule is layered: its platform's items, then its model template's, then its own,
and an item shadows a lower one of the same name (`schedule::name_key`). The row a vehicle tracks
under a name therefore changes: `update_schedule_item` or `dismiss_schedule_item` on an inherited
item gives the vehicle its own copy, `add_schedule_item` under an inherited name does the same,
and `update_vehicle` can move the vehicle to another model template.

A service link (`service_schedule_links`) and a job (`work_items.schedule_item_id`) each store a
row id. A reader that matches those ids against the rows the vehicle tracks misses every link
stored before the row changed, unless each write that changes the row also rewrites the links. A
write that forgets leaves history that silently stops counting.

## Decision

**A stored id names a row, and the item it counts toward is the one the vehicle tracks under the
same name.** `schedule::effective_ids(db, vehicle_id, ids)` maps each id to the item
`resolve_for_listing` shows for the vehicle under that row's name, dismissed items included, or
to itself when the vehicle has none. The vehicle's own row maps to itself. No write moves a link
when the tracked row changes.

The readers that judge what counts go through it:

- `schedule::links_of_services` and `schedule::links_to_items`, so service reads, reminders and
  the `released_schedule_item_ids` of a correction name the tracked item.
- `work_item::list_with_status`, the planned-work list behind `list_planned_work`, `whats_coming`
  and the home page, so a planned job carries the same id as the reminder it covers.
- `budget::forecast_from`, which asks whether a planned job covers an item.
- `schedule::drop_links_jobs_bring`, so a stored link to the inherited row still follows a job on
  the vehicle's own copy (ADR-0017). `add_schedule_item`, `update_schedule_item` and a change of
  the vehicle's model template end with it for the vehicle's services, because each can make a
  link and a job of one service count toward the same item.

`schedule::tracked_ids` still maps an id to the tracked item when a write stores it, so new links
and jobs name the tracked row. The mapping at read covers what a later change of row leaves
behind.

**Other job reads show the id as stored.** Job rows are loaded in many places, and outside the
planned-work list a reply that carries a job prints `schedule_item_id` unmapped. After a vehicle
gets its own copy, that id is the inherited row's, which `list_schedule` shows as the copy's
`overrides.id`, and the tools that take a schedule item id accept either one. A row of a model
template the vehicle has left is outside its schedule, so a write that names it is refused as not
found while its links keep counting.

The schema is unchanged and no migration runs. On the owner's records every stored link and
every job names the row its vehicle tracks.

## Alternatives considered

- **Store a name key and the vehicle in place of the row id.** It needs the link table rebuilt and
  a data migration, gives up the foreign keys' cascade, makes renaming an item a re-keying step,
  and fixes what a schedule item's identity is before named milestone services are designed.
- **Keep moving links on every write that changes the tracked row, and add the template change.**
  Each new way to change the row needs its own move, and a missed one loses history silently.
- **Map the id in every job read.** About nine modules load job rows. The id as stored stays a
  valid id of the vehicle's schedule, so the wider change buys only a tidier reply.

## Consequences

- No write moves a link, and reminders layer the schedule through `schedule::resolve`.
- History and plans follow a vehicle to another model template wherever the new template has an
  item of the same name. An item with no counterpart keeps its links, which count toward nothing
  until an item of that name exists again.
- A vehicle-owned item that is renamed keeps its links, because they name its row.
- The name is the only tie between an inherited row and a vehicle's copy of it when links are
  read. A migration that renames a platform or template item therefore moves the links and jobs
  that name the row, or the copies that override it, in the same step.
- Reads that list links do one extra pass through the vehicle's schedule.
- A new reader that compares a stored schedule item id with a tracked item must map it first.
