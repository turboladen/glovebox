# ADR-0025: A vehicle has a printable history

- **Status:** Accepted
- **Date:** 2026-11-24

## Context

A buyer asks what has been done to a car, and an owner wants that record to stay with the car.
The records hold the answer, but no page states it in one place. The home page shows only open
work, a service's page shows one invoice, and a vehicle's page shows the latest 20 records mixed
with incidents and odometer readings. Claude Desktop can write a summary, but its wording and
its choice of records vary between conversations, and a buyer has no reason to trust a summary
over a list.

## Decision

**Each vehicle has a history page at `/vehicles/{vid}/history`**, linked from the vehicle's page.
`history::for_vehicle` builds it from `service_record::list` and `part::list`.

**The page lists every service, oldest first.** A history is read from the start, like the
stamped pages of a service book, and a buyer checks that the intervals follow one another.
Services of one day keep the order they were recorded in. No service is left out and none is
grouped, so the page hides nothing a reader would have to ask about.

**Each service prints its date, description, odometer, shop, total and lines.** A service the
owner performed prints "Self-performed" in place of a shop. A line prints its description and
its kind, and never its cost or who paid for it. The total is the invoice's grand total, and a
service with no total prints none. A buyer judges what was done and roughly what it cost; the
split between the owner and an insurer, the invoice reference and the owner's notes are private
and stay on the service's page.

**The parts on the car follow the services.** The list holds each part whose status is
`installed`, in the order they went on, with its manufacturer and part number and the date and
odometer of the install. A part that is only purchased, was replaced or was returned is not on
the car and is left out. A kit prints as one part without its contents.

**The head names the car and the span of the records.** It prints the year, make, model and
trim, the VIN, the day the page was printed, the number of services, the first and last service
dates, and the lowest and highest odometer readings the services record. The owner's name for
the car prints only when the vehicle records no year, make, model or trim.

**The page leaves out everything that is not finished work**: incidents, recalls, planned jobs,
quotes, appointments, odometer readings logged between services, and the cost summary.

**On paper the page is plain, as the work sheet is** (ADR 0009). The links to the garage and the
vehicle's page, and the hint to use the browser's print command, sit in the on-screen strip.
Each service's date links to its page and each shop's name to the shop's page, and on paper
those links print as plain text.

## Alternatives

- **Newest first.** That order suits a feed the owner checks for the latest entry. A printed
  history is read once, from the beginning.
- **A print stylesheet on the vehicle's page.** That page mixes incidents and readings into its
  list and stops at 20 records, so it would need a second, hidden layout.
- **Line costs and the owner's share.** They answer the owner's questions about spending, which
  `cost_summary` and the service's page already answer, and they tell a buyer who paid, which is
  not the buyer's concern.
- **Only Claude Desktop writes the history.** The format would vary between conversations, and
  the owner would have to ask for it again after every service.

## Consequences

- One more page and template. The order and the choice of parts live in one tested shared
  function, so a tool that returns the history later reads the same function.
- A buyer sees every service's total. An owner who wants to hide a price has no switch for it.
- A service recorded with the wrong date or odometer shows on the page as it is stored, and the
  span in the head follows it. It is corrected through Claude Desktop.
- Recalls performed are not marked as such, beyond a line whose kind is `recall`.
