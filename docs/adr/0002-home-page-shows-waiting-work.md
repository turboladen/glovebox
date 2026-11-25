# ADR-0002: The home page shows what is waiting

- **Status:** Accepted
- **Date:** 2026-11-14

## Context

The web home page listed open appointments, to-dos, and planned jobs that have needs. An owner
with planned jobs and a maintenance schedule, but no needs and no appointments, saw an empty
page. What the owner wants from the page first is the cue to book the shop: which maintenance
and repairs are outstanding.

The `whats_coming` tool already answers that question from the shared library (due maintenance,
planned jobs, open recalls, open incidents).

## Decision

**The home page lists, per vehicle, everything that is not yet on an appointment**, above the
appointments. `waiting::waiting` builds four lists:

- schedule items that are `overdue` or `due_soon`;
- every `planned` job on no appointment, whether or not it has needs;
- recalls that are `open` or `planned`;
- unresolved incidents, notes excluded.

**Overlap follows two rules.** A maintenance item, recall or incident that a planned job covers
names that job, and the job is still listed among the jobs. When that job is on an appointment,
the item is left out, because the appointment already lists the job. A planned job is only ever
on an open appointment, so "has an appointment" is the whole test.

**The page and the tool share their filters.** `upcoming::planned_work`, `open_recalls` and
`open_incidents` feed both `upcoming` and `waiting`, so the two cannot disagree about what is
open.

**The lists only show.** They hold no forms. Planning a job, booking an appointment and closing
an incident stay with Claude.

**The forecast and the latest records follow the waiting lists (2026-11-24).** After the four
lists, one panel per vehicle prints the 12-month forecast (the total, the scheduled maintenance
and the planned work) and the three latest services, incidents and odometer readings, with a
link to the vehicle's page for the rest. The total is printed as "At least" while an item has no
estimate, with the count of such items from `BudgetForecast::unestimated_note`. The panel sits
after the waiting lists, because it is reference and asks nothing of the owner, and before the
appointments, so that the appointments and the to-dos, which hold the page's forms, stay
together at the end. A vehicle with nothing to forecast and no records shows no panel.

## Alternatives

- **Let the owner tick items and create an appointment on the page.** A due maintenance item
  would first have to become a planned job, which is a second write path beside the tools. The
  owner asked for the cue only.
- **Render `upcoming` itself.** It does not say which items a job already covers.
- **Put the forecast in the vehicle's heading.** A figure above the waiting lists would lead the
  page with money, and the cue to book the shop would come second.
- **Hide a covered item from its own list.** The owner would then not see that an overdue item
  is overdue once a job exists for it, although nothing is booked yet.

## Consequences

- `checklist::home` reads each vehicle's reminders and planned jobs once and builds the waiting
  lists and the forecast from them, so the forecast costs one more query per vehicle and the
  latest records three. The home page costs about two dozen queries per vehicle that is not
  archived.
- Maintenance is judged against today's mileage, and the sheet prints that mileage and says when
  it is an estimate.
- A list that leaves items out says how many ("plus 2 on an appointment"), so an empty list
  does not read as "nothing is due".
- `get_checklist` with no appointment still lists only jobs that have needs, because that view
  is about deciding, ordering and receiving parts.
- An incident whose job was dropped shows as not planned until the incident is resolved.
