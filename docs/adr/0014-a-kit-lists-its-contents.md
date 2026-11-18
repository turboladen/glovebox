# ADR-0014: A kit lists its contents

- **Status:** Accepted
- **Date:** 2026-11-18

## Context

The owner gathers the parts for an appointment against the printed work sheet (ADR 0009). The
sheet printed each bought part's manufacturer and number. Parts bought as a kit, such as an oil
change kit holding oil, a filter, a drain plug and a seal, have a number that only the seller
uses: it is printed on nothing in the box. With only that number on the sheet, the owner could
not tell from the sheet whether everything was in hand. What the kit held was written only in
the part's notes.

## Decision

**A part has an ordered list of contents** in the `part_contents` table. Each item has a name, a
manufacturer, the number on its own packaging, and a quantity. A part that is not a kit has none.
`record_part` takes the list. `correct_part` edits it by item: `add_contents` appends items after
the ones the kit holds, and `remove_contents` names the ids of items to remove, so an item the call
does not name stays as it is. An id that is not one of the part's items fails the call, which then
writes nothing. Every reply that returns a part carries its contents with each item's id: the
part tools, the service reads, the checklist, and the need replies (`mark_need_ordered`,
`link_part_to_need`, `mark_need_received`, `return_part`, `past_decisions`) and `get_work_sheet`.
The printed sheet leaves the ids off.

**The job and appointment pages list a kit's items** in the need's row, under "In the kit", whether
the row shows the linked part or a chosen option.

**The work sheet lists a kit's contents under it.** Each item has its own tick box, and so does
every line the owner brings. Those lines print the bought part's name. The need's name describes
what the job requires, and the part's name is what is on the box. The count still comes from the need,
so a part's name leaves the count out ("Beru spark plugs", not "Beru spark plugs, x4"). A kit
item's quantity is what one kit holds. The shop's lines carry no box
and no contents, because the owner does not gather them.

## Alternatives considered

- **One need per item in the kit.** A need links one part, and a kit is one purchase with one
  cost and one order. Splitting it would mean recording one purchase as several parts, with the
  cost divided by guesswork.
- **Printing the part's notes on the sheet.** Notes are free text, often about warranty or
  fitment, and give no number per item to check against.
- **A list that replaces the stored one whole.** It is one argument and keeps the order without
  positions, but an item left out of the list is deleted. A caller that read a kit somewhere that
  did not show its items, or that sent only the items it meant to add, would delete the rest.
  Adding and removing by id makes every deletion one the caller names.
- **Editing an item in place.** It would need a third argument with an update input per item. A
  kit's items are few and rarely wrong, so a mistyped item is removed and added again, and it
  then prints last.

## Consequences

- Contents are entered after the purchase: by `correct_part` for a part that `mark_need_ordered`
  created, or a part recorded on an invoice through `record_service`, until those calls take
  contents themselves.
- The ids in replies are what a caller sends to remove an item, so a part's replies carry them
  everywhere it appears.
