# ADR-0011: A sold car is archived, not deleted

- **Status:** Accepted
- **Date:** 2026-11-17

## Context

`delete_vehicle` removes only a car with no history, so a mistaken `add_vehicle` can be undone. A
sold car has history the owner keeps: its services, parts, costs and incidents are a record of
what was spent and done. It has to leave the garage without losing any of that. The `vehicles`
table has carried `archived_at` and the sale columns (`sold_date`, `sold_price_cents`,
`sold_mileage`) since the initial schema, and the web home page leaves out a vehicle with
`archived_at` set.

## Decision

**`archive_vehicle` stamps `archived_at` and records the sale fields it is given, in one
transaction.** Archiving a car already archived keeps its first stamp and records the sale.
`unarchive_vehicle` clears `archived_at` and keeps the sale fields.

**Open work is settled first.** Archiving is refused while the car has an open appointment, a
planned job or an open to-do, and the refusal says how many of each. No garage list shows an
archived car, so that work would otherwise stay open where nothing lists it. For the same reason an
archived car takes no new planned job, appointment or to-do (`vehicle::require_in_garage`);
every other write, such as a late invoice or a correction, still works.

**Garage lists leave an archived car out.** `list_vehicles` hides it unless `include_archived` is
true, and the web home page hides it. Both read `vehicle::list` with `include_archived` false.

**Everything else still reads it by id.** Every tool that takes a `vehicle_id` works for an
archived car, so its history, costs and services can still be looked up. The
`glovebox://vehicles` resource lists every vehicle with its `archived` flag, because it is the
full record set rather than the garage.

## Alternatives considered

- **Refusing writes to an archived car.** A sold car still gets the occasional late invoice or
  correction, and refusing them would force an unarchive and archive around each one.
- **Hiding archived cars from every read.** The records stay so that they can be read, so a lookup
  by id has to keep working.
- **A separate sale record.** The columns already exist on the vehicle, and a car is sold once.
- **Cancelling or dropping the open work as part of archiving.** That decides for the owner what
  became of each job, which the rest of the app never does.

## Consequences

- Claude Desktop does not offer an archived car in the garage, so it does not plan work for it
  unless the owner asks about it by name.
- `whats_coming` for an archived car still answers. The project prompt tells Claude not to plan
  work for one.
