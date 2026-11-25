# ADR-0026: A warranty ends at its first bound

- **Status:** Accepted
- **Date:** 2026-11-24

## Context

A vehicle and a part each store two warranty bounds: an expiry date (`warranty_expires_on`) and
an expiry odometer reading (`warranty_expires_miles`). Either may be empty.

`whats_coming` reports the vehicle's warranty with one flag, `possibly_covered`, which is true
while either bound still holds. A ten-year-old car that is under its mileage limit reads as
possibly covered, and the project prompt tells Claude to remind the owner to check coverage
before paying. A factory warranty written as "4 years or 50,000 miles" ends at whichever comes
first, so that reminder is wrong for every car past one bound.

A part's two bounds are stored and no read uses them. The owner records them because a seller's
warranty decides whether a failed part is replaced free, and the question "which parts are still
under warranty" has no answer short of reading every part.

## Decision

**A warranty is in force only while none of its bounds has passed.** `warranty::read` takes the two
bounds, a day and an odometer reading and returns `WarrantyBounds`: `in_force`, `date_passed` and
`days_left`, `mileage_passed` and `miles_left`. A fact about a bound the warranty does not have is
null, and a warranty with no bound at all has no reading.

**The expiry date and the expiry reading are still inside the warranty.** The date bound has
passed when the day is after the expiry date, and the mileage bound when the odometer is above
the expiry reading. `days_left` and `miles_left` are zero at the bound and after it.

**The odometer is the vehicle's estimated mileage on the day of the read**
(`reminders::estimate_mileage`), the same figure the reminders use.

**The vehicle's warranty status keeps `possibly_covered`** with its meaning, and carries the
per-bound facts and `in_force` beside it. A client reports `in_force`.

**`list_parts` carries a part's reading as `warranty`** on every part that has a bound, and the
filter `under_warranty: true` keeps the parts a claim could still cover: those that are purchased
or installed and whose warranty is in force. The `warranty` value reads the bounds alone, so a
replaced part can show `in_force: true`. No other reply that returns a part carries the value,
because those replies take no `today`.

## Alternatives

- **Changing what `possibly_covered` means.** One field would then say the right thing, but a
  client written against the stored meaning would change its answer with no change in the field's
  name. A new field with a plain name costs one more key.
- **Signed `days_left` and `miles_left`**, negative after a bound has passed. The sign would say
  how long ago the warranty ended, which no question asked so far needs, and a negative "left"
  reads as a mistake. The `*_passed` flags say which side of the bound the car is on.
- **Putting `warranty` on `PartView`**, so every part reply carries it. Each of those replies
  would need `today` and the vehicle's mileage estimate, including the write replies, for a value
  the caller of `record_part` or `correct_part` does not ask for.
- **Letting `under_warranty` include a replaced part.** A part replaced inside its warranty is the
  claim the owner may want to make, but a part replaced years ago with a long warranty would stay
  on the list until the bound passes. The status filter and each part's `warranty` still answer
  the narrower question.

## Consequences

- A vehicle past one bound and inside the other reads `possibly_covered: true` and
  `in_force: false`. The `whats_coming` description and the project prompt tell the client to
  report `in_force` and to name the bound that passed.
- A mileage bound is judged against an estimate. Near the bound the reading can be wrong by the
  miles driven since the last logged reading, so `miles_left` close to zero is a reason to log
  the odometer.
- A vehicle with no odometer reading is estimated at its purchase mileage, or zero without one,
  so a mileage bound on such a vehicle reads as holding.
- Stored records need no change. The reading is derived on every read from columns that already
  exist.
