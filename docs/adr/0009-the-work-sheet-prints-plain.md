# ADR-0009: The work sheet prints plain

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

When the owner drops the car at a shop, the mechanic needs a short list of the work: each job,
the parts the owner is bringing, and the parts the shop should supply. The appointment page cannot
serve as that list. It is organized by the owner's next steps (decide, order, arriving), and it
carries forms, prices and research the mechanic does not need. A part that is still undecided, or
chosen but not bought, would put something vague or wrong in front of the mechanic.

## Decision

**Each appointment has a work sheet page at `/vehicles/{vid}/appointments/{id}/print`**, linked
from the appointment page. `work_sheet::for_appointment` builds it. The page lists every planned
job with a box to tick. Under each job, "I'm supplying" lists the owner's parts with the
manufacturer and part number, and "Please supply" lists the shop's parts by name. Either list
shows a quantity above one. A kit the owner brings lists its contents under it, and ADR 0014
records why. A job's notes print under its title, and a part's notes print under its line,
whichever list it is on. A note that is blank prints nothing.

**On paper the sheet is black on white.** It departs from ADR 0008's ledger look: it has no
panels, pills or tints, and the site header is hidden. Every other page prints as it shows. A job does not split across pages unless it
is longer than one. The browser's own print command prints it, because the page runs no script.

**Needs that are not settled show only on screen, in a "Before you print" box.** That covers
undecided needs and chosen parts not yet ordered. Each has a link to decide it on the job page and
an "Ask the shop" button. The button sets the need's supplier to the shop, so the records and the
sheet agree. The sheet does not keep a print-only override that would leave the checklist saying
something else.

**An ordered part that has not arrived prints as the owner's.** The owner expects it before the
visit.

`get_work_sheet` returns the same sheet to Claude Desktop.

## Alternatives

- **A print stylesheet on the appointment page.** That page is grouped by next step, not by job and
  supplier, so it would need a second, hidden layout, with the warnings mixed in among its forms.
- **Only Claude Desktop writes the sheet.** The owner starts from the appointment page, and the
  format would vary between conversations. The MCP tool is kept as a second way in, over the same
  shared function.

## Consequences

- One more page and template to keep in step with the checklist. The placement rules live in one
  tested shared function, which both the page and the tool read.
- "Ask the shop" is a real decision. The checklist shows it like any other supplier choice.
