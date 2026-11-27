# ADR-0030: Wear is read from typed readings

- **Status:** Accepted
- **Date:** 2026-11-26

## Context

Shop invoices report how worn the brakes and tires are: "front pads 4 mm (caution), rear pads
2 mm (FAIL)", "tires 7/32 front, 9/32 rear (min 3/32)". Those readings lived only in a service's
notes, so nothing could answer "when will I need brakes or tires?". The GTI's invoices show the
shape of the data:

- Shops report by axle, never by wheel.
- The minimum a shop states varies by visit (2 mm on one, 3 mm on another).
- One brake check is a percentage ("front 50%+, rear 30%"), which is not a length.
- A rotation swaps front and rear tires, so "front tread" measures different tires over time.
- Replacements happen between readings (the 2022-09-15 front pads and rotors), and the tires were
  replaced between 2021-09 (3/32) and 2022-06 (7/32) with no part on record.

## Decision

**A reading is stored as written.** `wear_readings` holds the component (`brake_pads`,
`brake_rotors`, `tire_tread`), the axle (`front`, `rear`), the value and unit the shop wrote
(`mm`, `in_32`, `in`), the limit the shop stated beside it if any, the day, the odometer and the
service that took it. Every comparison and rate goes through `uom::si::f64::Length` on read
(`units::Measure`), so 8/32 in and 6.35 mm agree, and a reply quotes a reading as the shop wrote
it. `units.rs` is the only module that names `uom`.

**The tires are one set; brakes are per axle.** A brake series is one component on one axle,
because front and rear brakes are replaced separately. The tires form one series judged by the
lowest reading of each visit, so a rotation does not break the curve and the answer is the one
that matters: when the set needs replacing.

**A tagged part starts a new series.** A part carries an optional `wears_as`
(`parts.wears_component` and `parts.wears_axle`): a brake tag names its axle, a tire tag names
none, and a CHECK keeps the pair valid. The newest install of a tagged part that is installed or
replaced starts its series on its install day. Readings dated on or before that day are history,
because a shop measures at inspection, before it replaces the part ("front pads 2 mm,
replaced"); the rule holds whether the install names its service or not, and after the service
is deleted. A series with no reading since is left out of the outlook, and so is a reading dated
after today.

**The limit is the newest one a shop stated in the series, else a default.** The defaults are
3 mm for pads and 2/32 in for tread (the US legal minimum). Rotors have none, because a rotor's
minimum is stamped on the rotor; a rotor series with no stated limit is shown and not projected.

**The rate is a least-squares fit over the series.** It is thickness over odometer; a reading with
no odometer takes one estimated from the mileage log. Two readings are needed, and a rate that is
flat or rising gives no projection and a `note` saying the readings do not show wear, which on
real data means a replacement is missing from the parts. Miles left count from the latest
reading to the limit at that rate, less the miles driven since, where the car's mileage is the
estimate from the mileage log or the newest reading's odometer, whichever is higher; the day
comes from the vehicle's daily rate. A newest reading with no odometer gives no projection.
Correcting a service's date or mileage moves its readings that took them. A latest reading at or below its limit is `overdue`; otherwise the status is judged
with the schedule's default 1,000-mile and 30-day warnings.

**The outlook is derived on every read.** `wear::outlook` builds `whats_coming`'s `wear` from the
reminders it already computes, and nothing about a projection is stored.

**A reading belongs to its service's record.** Readings on an invoice go in `record_service`'s
`wear_readings` and take its date and mileage. Deleting the service deletes them, except after
`done_another_way`, which keeps them with no service. A reading never makes a service count as
work done, so it does not change when `delete_service` asks what happened.

Percentages are not recorded; they stay in the notes.

## Alternatives considered

- **Tires per axle, ignoring rotation.** Simpler, but every rotation mixes two sets of tires into
  one rate.
- **Tires per axle, following rotations.** The most precise, but it needs every rotation logged,
  and one was skipped on 2026-11-18.
- **A separate replacement entry, or a reading marked "new".** The first can disagree with the
  parts list, which already records the install; the second keeps projecting the old parts until
  someone measures the new ones.
- **An exact integer base unit (nanometers) or a fixed unit per component.** The integer form
  allows SQL math nothing needs; a fixed unit cannot hold 6.5 mm or a metric gauge.
- **Converting percentages with a new-part thickness.** The answer is only as good as the guess.

## Consequences

- After the upgrade nothing reads differently: no reading exists until one is recorded. The GTI's
  readings are backfilled from its service notes through `record_wear_reading` with each
  `service_id`, and parts 61 and 62 are tagged as the front pads and rotors.
- Until a tire part is recorded for the set bought between 2021-09 and 2022-06, the tire series
  rises across that gap and shows no projection, with its note.
- A front-only tire replacement resets the whole set's series. That follows from tracking the
  tires as one set.
- Linking a shop advisory to the reading that prompted it, tire pressure and per-wheel readings
  are left out.
