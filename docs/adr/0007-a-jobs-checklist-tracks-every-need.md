# ADR-0007: A job's checklist tracks every need from decision to install

- **Status:** Accepted
- **Date:** 2026-11-13

## Context

Glovebox can say what maintenance is coming, but a shop visit also needs follow-through. Each
thing a job requires is a chain of decisions: who supplies it, which part, a direct replacement or
an upgrade, and from which seller. Each decision then has to be carried out: the part is ordered,
it arrives and the box is checked, and it goes on the car. Research for a job often starts months
before it has a shop or a date. When that chain lives in free-form notes, parts are forgotten and
wrong ones are ordered, although every fact needed to avoid it was known. The owner wants a short
list that shows every need has a decision and every decision has been carried out, with the
reasons kept so that Claude can use them the next time the same choice comes up.

## Decision

**An appointment groups jobs and to-dos for one shop trip.** It belongs to a vehicle and holds a
purpose in the owner's words, with the shop, date and notes optional until known. A job is planned
work (`work_items`) and sits on one appointment or on none. A job without one is in the
unscheduled pool, where it collects needs and research before any visit is booked. A to-do is a
plain checkbox for the vehicle, optionally on an appointment, and can be ticked and un-ticked.
Deleting an appointment never deletes a job, a to-do or a service.

**A job lists needs, and a need lists researched options.** A need holds a name, a quantity, its
supplier (`self`, `shop` or not decided), its approach (`replacement`, `upgrade` or not decided),
the chosen option, the reason in the owner's words, the date it was decided and its linked part.
An option holds the manufacturer, part number, description, seller, price and link, a fitment
verdict (`confirmed`, `unconfirmed` or `does_not_fit`) with a note giving the evidence, and a
rejection with its reason. Rejected options are kept, because they are the history Claude learns
from.

**A need's stage is derived and never stored.** `need::stage` reads it from the need and its
linked part, and the first row that holds wins:

| Stage | Holds when |
| --- | --- |
| `installed` | The linked part is installed or replaced. |
| `received` | The linked part has a received date. |
| `ordered` | A part is linked. |
| `shop_supplies` | The supplier is `shop`. |
| `to_order` | The supplier is `self` and an option is chosen. |
| `undecided` | Anything else. |

A need is open when its stage is `undecided`, `to_order` or `ordered` and its job is still
planned. Every open-needs count goes through `need::counts_as_open`.

**The owner decides.** No tool chooses a supplier or an option, and the tool descriptions tell
Claude to ask and record the answer. An option that does not fit or has been rejected cannot be
chosen. An unconfirmed one can, and the checklist says so until it is confirmed.

**Ordering creates a part record.** `need::mark_ordered` creates the part from the chosen option,
supplied by the owner, and links it to the need, so the part stays the single home of purchase
and warranty facts. A part that is already recorded can be linked instead, with its arrival date
when the owner already has it. Marking a part received is its own step, because the box check on
arrival is when a wrong part is caught.

**Returning a part is its own action.** `need::return_part` marks the part `returned` with the
reason and date and keeps the record. It unlinks the part from the need, and when the option
itself was the mistake it clears the choice and rejects that option for the same reason. It also
adds a to-do to take the part back. A returned part is left out of parts spending.

**A closed or cancelled appointment holds no open work.** An appointment reads `done` when it has
jobs and every one is done or dropped. Only a hand-closed date and a cancelled date are stored.
Closing is refused while a job is planned or a to-do is open, and the refusal names each one.
Cancelling takes every planned job and open to-do off the appointment, with its research intact.
Neither kind accepts a new job, to-do or invoice. When a job returns to planned or a to-do is
un-ticked, a hand-closed appointment opens again and a cancelled one lets the item go
(`appointment::settle_reopened_jobs`). ADR-0005 covers the case where deleting a service reopens
jobs.

**Filing the invoice names what it did.** `record_service` may name an appointment, but naming it
installs and completes nothing. The jobs it completes and the parts it installs are named
explicitly, because a shop does not always fit everything that was brought. Its result lists the
linked parts it did not install so that Claude can ask about each.

**One shared function builds the checklist.** `checklist` produces the view of an appointment, of
one job or of the unscheduled jobs. Its next steps group the needs as decide, order, arriving,
to-dos and done, and its warnings are plain statements. The MCP tools and the web page both
render it. `past_decisions` searches needs across every vehicle and returns each decision with
its reason and its rejected options.

## Alternatives considered

- **Storing the stage.** Every write that touches a need, an option or a part would have to keep
  it current, and any write that missed it would make the list lie.
- **A stored appointment status.** It could disagree with the jobs. Deriving `done` from the jobs
  means a reopened job reopens the appointment with no extra write.
- **Closing an appointment unassigns its planned jobs, as cancelling does.** A closed appointment
  records a trip that happened, so work still open on it needs the owner's word first.
- **Purchase details on the need.** That would split purchase and warranty facts between two
  tables and leave parts bought before the checklist outside it.

## Consequences

- A part linked to a need decides its stage whatever the supplier and choice say. A linked part
  with no chosen option shows a note that no decision is recorded.
- A need with no chosen option holds a reason only while the shop supplies it.
- Marking a need ordered has no undo on the page, because it creates a purchase record. A mistaken
  order is corrected through Claude.
- A to-do from a return goes on the job's appointment only while that appointment is open, and on
  the vehicle otherwise, because no list shows the to-dos of a closed or cancelled appointment.
