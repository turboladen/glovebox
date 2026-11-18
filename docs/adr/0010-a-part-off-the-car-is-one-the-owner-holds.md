# ADR-0010: A part corrected off the car is one the owner holds

- **Status:** Accepted
- **Date:** 2026-11-17

## Context

`correct_part` can change a part's status. Setting an installed or replaced part to `purchased`
says the part is not on the car. A part that keeps its installing service, install date and
odometer after that change is still listed by the service, has its install date moved when the
service's date is corrected, and is reverted when the service is deleted, although it already
reads purchased.

Clearing those details is the obvious repair, but `purchased` means more than "not on the car". It
means the owner holds the part. That raises three cases:

- A part the shop supplied was never the owner's. Calling it purchased leaves a shop part that
  belongs to no service, and its need reads `ordered` for a part the owner never bought. Unlinking
  it from the need instead leaves an orphaned part and raises questions about replaced parts and
  about which supplier counts when the same call changes it.
- An owner part installed through `record_service` may never have been marked received, so once
  it is off the car its need reads `ordered` although the owner has it in hand.
- A call that sets `purchased` and also sends install details contradicts itself.

## Decision

**Setting an installed or replaced part to `purchased` clears its install and replace details.**
The installing service, install date and odometer, and the replaced date and odometer, are all set
to empty.

**The part reads received.** When the part has no `received_on`, the correction sets it to the
install date it clears (the purchase date when that is later), or to today when there is no install
date. A `received_on` sent in the same call wins.

**A part the shop supplied is refused.** For an installed part the error says to correct the
service that installed it or to delete the part; for a replaced part, which a corrected service
leaves replaced, it says to delete the part. The supplier that decides is the one the part ends with, so a call that also
sets `supplied_by` to the owner is allowed.

**Contradicting details in the same call are refused.** A part that ends up purchased from the car
takes no installing service, install date or odometer, no replaced date or odometer, and no
clearing of its received date in that call.

The rules live in `part::update` (`validate_coming_off` and the clearing beside it).

## Alternatives considered

- **Refusing the change whenever install details are set.** This forces two calls, one to clear the
  details and one to change the status, for the most common correction.
- **Unlinking a shop part from its need.** The need falls back to `shop_supplies`, but the part
  stays as an orphaned shop part, and whether a replaced part or a part whose supplier the same
  call changes should leave its need has no clear answer.
- **Deleting a shop part as part of the correction.** It deletes a record as a side effect of a
  status change, which `delete_part` already does when that is meant.
- **Keeping install details the same call sends.** That stores a purchased part that names an
  installing service, the state this decision removes.

## Consequences

- Saying a shop part was not fitted goes through `correct_service` (dropping it from `part_ids`) or
  `delete_service`, which apply ADR-0005's rules, or through `delete_part`.
- An owner part that comes off the car shows as received on its need, ready to be fitted again.
- `record_part`, a `correct_part` that sends install details without a status, and a returned part
  set back to purchased can still give a purchased part an installing service. That case is
  tracked as its own issue.
- Taking a part off the car does not reopen its job. When the job should be done again, the job
  is set back to planned, or the service that completed it is corrected.
