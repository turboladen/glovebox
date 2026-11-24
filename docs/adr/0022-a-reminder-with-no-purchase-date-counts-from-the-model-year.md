# ADR-0022: A reminder with no purchase date counts from the model year

- **Status:** Accepted
- **Date:** 2026-11-24

## Context

A reminder's due point is an interval added to a baseline. The baseline is the latest service
linked to the schedule item, or the vehicle's purchase date and mileage when no service is
linked. A vehicle may have neither a linked service nor a purchase date. Counting a time-based
item from today in that case moves its due date forward every day, so the item reads `ok` for as
long as the purchase date stays empty, and the budget forecast places it a full interval out.

The mileage half already has an answer for the same gap: with no purchase mileage it counts from
zero, so a mileage-based item on a car with 60,000 miles reads `overdue` with `baseline: none`.

Odometer readings feed every estimate. A reading below zero or dated after today is a typing
mistake, and a reading dated ahead becomes the newest one, so the estimate starts from it and
ignores the real readings logged after it.

## Decision

**With no linked service and no purchase date, a time-based item counts from January 1 of the
vehicle's model year.** A vehicle with no model year either has no due date, and the item is
judged on mileage alone. This is the date half of what the mileage half does with zero.

**`baseline` reads `purchase` only when the purchase record holds what the item counts from**:
the purchase date for an item with a month interval and the purchase mileage for one with a mile
interval. Otherwise it reads `none`, which tells the client that part of the due point is a
stand-in and that it should ask when the work was last done.

**A reading below zero or dated after today is refused** by `log_mileage`, and a service or a
vehicle's purchase record refuses a mileage below zero. A service dated ahead can still log a
reading dated ahead, so the mileage estimate ignores every reading dated after today.

**Of a day's readings the latest is the one with the latest time**, where a reading with no time
of day counts as the end of its day (`DayOrTime::or_end_of_day`), as a service's reading does. Of
two at the same moment the higher is the later.

## Alternatives

- **An `unknown` due status.** It says exactly what is known, but `DueStatus` is shared by
  reminders, planned work's revisit, the bundle suggestion, the budget and the web pills, and
  each would need a rule for it. The mileage half would also need the same status to stay
  consistent, which changes how every car with no purchase mileage reads today.
- **No due date at all without a purchase date.** A month-only item then reads `ok` with nothing
  to show why, which is the misleading reading this decision removes.
- **Refusing a service dated after today.** A typed date is most often wrong by a year, but the
  owner may file an invoice for a car still at the shop with tomorrow's end date. Ignoring the
  future reading in the estimate removes the harm without a new refusal.
- **Taking the highest reading of a day as the latest.** A mistyped high reading would then
  outrank the correction logged a minute later.

## Consequences

- A vehicle with a model year and no purchase date shows each time-based item that has no linked
  service as overdue, on the web home page and in the forecast as well as in `whats_coming`. The
  mileage half already reads this way. Entering the purchase date or recording the service that
  did the work corrects it.
- An item with a mile interval on a vehicle that has only a purchase date reads `baseline: none`
  where it read `purchase`. Its status does not change.
- Stored records: the one vehicle in the 2026-11-18 backup has a purchase date, so its reminders
  read as before. That backup holds no reading below zero, no reading dated after
  today and no service with a mileage below zero, so no stored row breaks a new refusal.
- January 1 of the model year is an approximation. A car of a model year is often sold some
  months earlier, so the due date can be late by that much.
