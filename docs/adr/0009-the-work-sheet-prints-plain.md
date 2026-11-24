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

**The sheet names the car and the complaint**, because the mechanic reads it with the owner
gone and the owner's nickname for the car identifies nothing in a shop's lot. Under the heading
(the vehicle's name, the shop and the date) it prints the year, make, model and trim on one line,
then the VIN, the license plate and the odometer, each only when the vehicle has it. The odometer
is the newest logged reading with the day it was read. The sheet never prints the projected
mileage the reminders use, because a mechanic would copy it onto the invoice as a reading, and a
vehicle with no logged reading prints no odometer. A job filed from an incident prints the
incident's title and description under the job's title, labeled "Reported symptom", so the
mechanic reads what the owner noticed and not only the job the owner guessed at. Only an incident
of the same vehicle prints, and only while it is unresolved and its category is not `note`: a
resolved incident is not a current complaint, and a note records a fact, not a symptom. The
incident's notes never print. The tool and field descriptions tell Claude Desktop that the title
and description of an incident a job is filed from are text the shop reads.

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
- The symptom is read from incidents recorded before the sheet printed one. In the backup of
  2026-11-18, five planned jobs name an incident. All five are on an appointment and all five
  incidents are unresolved (three `shop_observation`, one `noise`, one `performance`), so each
  prints its incident's title and description on the next sheet. Two done jobs and one dropped
  job also name an incident and print nothing, because the sheet lists only planned jobs.
