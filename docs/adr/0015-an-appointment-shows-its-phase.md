# ADR-0015: An appointment shows its phase beside its status

- **Status:** Accepted
- **Date:** 2026-11-18

## Context

An appointment's status is `planning`, `done` or `cancelled`, derived from its closed and
cancelled dates and its jobs. The status has no value for a trip whose date has come, so an
appointment with the car at the shop and one with no date yet both read `planning`. A planned
date sets only the `booked` flag, which means "has a date". The same
vocabulary is also `update_appointment`'s `status` argument, so a new value there would be one a
caller could send as a change that means nothing.

## Decision

**A derived phase sits beside the status.** `appointment::phase` reads the status, the planned
date and today's date: an open appointment is `planning` with no date, `booked` before its date
and `in_shop` from its date on, and `done` and `cancelled` carry over. Every
`AppointmentSummary` carries it, so `list_appointments` and `get_checklist` return it, and the web
pills show it: "Booked" and "In the shop" as `wait` pills.

**No rule keys on the phase.** Closing, reopening, what a list shows as open and what takes new
work all still read the status or the stored dates. The phase only reports.

**Today is a parameter.** The readers that build summaries take `today`, and so does
`update_with_status`, so the web page's fixed test clock decides the phase. The work sheet and the
open-work check before archiving a car read the clock themselves, because they never show a
phase.

## Alternatives considered

- **New values in `AppointmentStatus`.** That would split the input vocabulary from the output
  one, or let a caller set `booked` by hand.
- **A web-only label from the `booked` flag.** Claude Desktop would still read `planning` and
  tell the owner the trip had not happened.

## Consequences

- An appointment whose date passed without the trip reads `in_shop` until it is re-dated,
  cancelled or filed. The project prompt tells Claude Desktop to ask about one whose date is more
  than a few days past.
- The phase can change overnight with no write, so a cached read can be a day stale.
