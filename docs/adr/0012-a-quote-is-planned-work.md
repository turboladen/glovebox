# ADR-0012: A quote for work not done is planned work

- **Status:** Accepted
- **Date:** 2026-11-17

## Context

Shop invoices often carry quotes for work the owner did not have done on that visit, such as a
dealer's estimate for a mirror repair printed beside the oil change. Importing the owner's real
records stored one as a zero-cost inspection line plus service notes, where nothing can find it
as a quote, price it in the budget, or turn it into a job. A quote has the same shape as planned
work: a job with an estimate, from a source, on a date.

## Decision

**A quote is a planned job.** `plan_work` files it with the quoted amount in `est_cost_cents`,
and three columns on `work_items` say where it came from: `quoted_by` (who gave it, such as the
shop), `quoted_on` (the day) and `quote_service_id` (the service whose visit produced it). A quote
that came with an invoice links it, and the service then carries the shop and the day; the two
text fields are for a quote with no invoice, or one given by someone the service does not name.
They are never copied from the service, so correcting the service cannot leave them stale.

**The service names its quotes.** A service read lists the jobs it quoted in
`quoted_work_items`, apart from the jobs it completed. Deleting the service keeps each quote,
clears its `quote_service_id` in the same transaction (the `ON DELETE SET NULL` key does the same
for any other writer), and lists it in `quotes_unlinked`.

**The project prompt tells Claude Desktop** to file a quote this way and never as a zero-cost
service line.

## Alternatives considered

- **A project-prompt instruction alone** (`plan_work` with the source in the notes). That changes
  no code, but the source of a quote stays free text, and the invoice cannot list its quotes.
- **A separate quote table.** It would duplicate the job's estimate and need its own tools. It
  pays off only for quotes with several lines or competing quotes for one job, which nothing asks
  for yet.

## Consequences

- A quote counts in the planned-work budget through its estimate until it is done or dropped, so
  an old quote the owner never acts on should be dropped.
- `search_records` matches a job's title and notes, not `quoted_by`; a search for the shop finds
  the quoting service, which lists the quote.
