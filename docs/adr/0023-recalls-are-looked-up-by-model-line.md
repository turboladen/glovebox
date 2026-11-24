# ADR-0023: Recalls are looked up by model line

- **Status:** Accepted
- **Date:** 2026-11-24

## Context

`check_recalls` asks NHTSA's recalls API for a make, model and year. That API lists recalls by
model line: it answers 400 for "Golf GTI" and lists the GTI's recalls under "Golf". The model a
vehicle stores comes from the VIN decoder or the owner and is often the fuller name. NHTSA
answers 400 for an unknown make or year as well, and does not say which value it refused.

Each request may take 20 seconds before it counts as failed, and the lookup runs inside a tool
call that promises a failed lookup does not block the conversation.

## Decision

**A lookup drops the last word of the model until NHTSA lists the name.** "Golf GTI Autobahn" is
asked for, then "Golf GTI", then "Golf". The first name NHTSA lists gives the recalls, and they
are stored on the vehicle as open recalls.

**The reply says when the line was used.** `check_recalls` returns `searched_model` naming the
line NHTSA was asked about when it is not the vehicle's own model. The tool description and the
project prompt tell the client that a recall of the line may not apply to the car, and
`set_recall_status` marks one `not_applicable`.

**The whole lookup has one time budget** (`nhtsa::LOOKUP_BUDGET`), so several names tried in a
row cannot hold a tool call for the sum of their timeouts. One client serves every NHTSA request.

## Alternatives

- **Storing the model line on the vehicle.** The owner would have to know NHTSA's naming, and
  the VIN decoder returns the fuller name.
- **Checking the shorter name against NHTSA's model list for the make and year.** It would catch
  a shorter name that is another vehicle ("Transit" for a "Transit Connect") at the cost of a
  second API and a second request per lookup. It is the right next step if a second make is
  added whose names shorten that way.
- **Looking recalls up by VIN.** NHTSA's public API has no VIN lookup for recalls.

## Consequences

- A stored recall does not record that it came from a model line. `list_recalls`, `whats_coming`
  and the home page show it like any other recall, so the caveat reaches the owner only in the
  conversation that ran the check.
- A model whose shorter name is a different vehicle gets that vehicle's recalls. Both of the
  owner's cars are Golfs, whose line is "Golf".
- A wrong make or year ends with a message that asks the owner to check all three values, since
  NHTSA does not say which it refused.
