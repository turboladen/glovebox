# ADR-0029: A milestone service is derived

- **Status:** Accepted
- **Date:** 2026-11-25

## Context

The owner, the dealer and the mechanic speak of "the 60k service". Glovebox tracks each schedule
item on its own interval, so the reminders list oil at 69,900, air filter at 72,000 and tire
rotation overdue at 57,000, and nothing names the trip the shop will quote. A shop's milestone
package also charges for every item the manufacturer lists at that mark, including one done a
month earlier on another visit.

CLAUDE.md asks that named milestones stay possible, and that nothing collapses schedule items
into one list or hard-codes the grouping.

## Decision

**The milestone is computed from the reminders on every read and never stored.**
`milestone::from_reminders` takes the reply `reminders::calculate_reminders_on` gives and the
day of the read.

**The mark is the first multiple of 10,000 miles above the estimated mileage.** A car at 60,060
reads `70k service`, and a car at exactly 60,000 reads `70k service` too, because its 60k is
already here. `reached_around` is the day the car's average daily mileage reaches the mark, and
none when the car has no rate.

**An item is in the milestone when its next due point falls by the mark**: it is overdue or due
soon now, its due mileage is at or below the mark, or its due date is on or before
`reached_around`. A car with no mileage rate still lists what is due by date now. The date
rule brings in the items with no mileage interval, such as brake fluid and wiper blades, which a
shop does on the same visit.

**The list is split by `is_factory_recommended`**, so the owner can ask for "the factory 70k
items plus the alignment". An item with no flag counts as not factory.

**`done_recently` lists the factory items the manufacturer's package at the mark includes that a
recent service did.** An item belongs to the package when its mileage interval divides the mark.
It is listed when it is not due by the mark and a service is on record for it, which means that
service did it within one interval before the mark. The owner reads this list to the shop so the
package does not charge for those items twice.

**`whats_coming` returns it as `milestone`, and the home page prints it** in a panel after the
four waiting lists and before the forecast, judged from the same reminders. The work sheet does
not print it: the sheet lists one appointment's jobs, and the milestone is a view of the schedule.

## Alternatives

- **Storing milestones as rows that a service completes.** A car drifts off the 10,000-mile grid:
  oil done at 61,000 is next due at 71,000, and a stored "70k service" would either disagree with
  the reminders or have to be rewritten by every service. Which items a service did is already
  stored in the schedule links, so a stored milestone would be a second copy of them that could
  go stale.
- **Seeding VW's milestone checklists.** The seed holds only items with real intervals, not the
  manufacturer's inspection checklist ("inspect brake pads", "check tire pressure"), and copying
  that list into a migration would invent intervals for items that have none. The milestone means
  "the tracked items due by 70k", and the project prompt tells Claude to say so.
- **Marking by miles only.** Brake fluid and wipers, which count only time, would never appear,
  and the shop would still do them on the visit.
- **Counting an item done recently when its last service falls within some fixed window.** A
  window is one more number to choose, and the interval already says what "recently" means for
  that item.

## Consequences

- The milestone moves as the estimate moves. A car that drives faster than its readings say
  reaches its mark sooner than `reached_around`.
- A car that drives little reaches its mark years out, so every item counted by time falls due
  before it and is listed, including one done recently. The milestone says what is due by the
  time the car gets there, not what one visit at the mark would cover.
- The milestone lists an item whether or not a job is planned for it or that job is on an
  appointment, since it shows what the shop's package covers. The waiting lists and
  `planned_work` say what is already planned.
- An item whose interval does not divide the mark (a 20,000-mile air filter at 70k) is never in
  `done_recently`, even when the shop's 70k package happens to include it.
- A page that shows a past milestone in retrospect, which items a visit did and which it left
  out, can be built from the same reminders and schedule links without a migration.
