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
`record_part` takes the list. `correct_part` takes a list that replaces the stored one whole, and
an empty list clears it. `list_parts`, `record_part` and `correct_part` return each part with its
contents.

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
- **Tools that add, change and remove one item at a time.** A kit's list is short and is known
  whole from the order or the box. Replacing it in one call keeps the order without position
  arguments and needs one tool argument where three tools would be needed.

## Consequences

- Contents are entered after the purchase: by `correct_part` for a part that `mark_need_ordered`
  created, or a part recorded on an invoice through `record_service`, until those calls take
  contents themselves.
- The job and appointment pages do not show contents yet. Only the work sheet does.
