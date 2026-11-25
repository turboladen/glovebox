# ADR-0008: The web page reads as a ledger

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

The web pages are a working record, like the notebook an owner keeps in the glovebox. The owner
reads many rows of parts and decisions and acts on a few of them. Every row has to make three
things plain: which text is a label printed by the page, which is a value from the record, and
what can be acted on. The page also has to separate what still needs the owner from what is
settled. The owner has a mild color-vision deficiency, so no state may be told apart by color
alone, and the page must work in both light and dark themes.

The page runs no script and loads nothing from another origin, so every visual device has to be
plain HTML and CSS.

## Decision

**The page is a ledger of panels on a tinted ground.** Each group of steps and each job sits in a
panel. An open need takes an inset background and an ink bar at its start. The colors are
tokens on `:root` in the one inline stylesheet in `glovebox-web/templates/base.html`, with a
second set under `prefers-color-scheme: dark`.

**A job's needs sit in columns under one header line**, with prices right aligned. Rows are CSS
grids that share one column template, so each kind of fact sits in the same column on every row, and
an option lines up under the part it would replace. The job's head and the header line stick to the
top of the window while its rows scroll. On a narrow window the header line is hidden and each row
becomes a list of label and value pairs, with a missing value left out together with its label.

**One typeface in two weights and four sizes.** IBM Plex Sans in regular and semibold is compiled
into `glovebox-web` from IBM's unmodified release, with its license beside the files, and served
from `/assets/` by this server. The sizes are label, data, heading and page title, each a token in
the stylesheet. Numerals are tabular. The slashed zero is on only for part numbers and prices, where
a zero sits beside letters or must line up in a column.

**Gray text is a label and dark text is a value.** A missing value such as "Not decided" is the
one gray value.

**A pill is a value from a fixed list, and each kind has a tint and a mark.** `PillKind` in
`glovebox-web/src/view.rs` has five kinds, and an exhaustive match maps each typed value to one,
so a new variant needs a kind before it compiles:

- `todo` (accent fill, semibold, empty box) needs the owner, such as Undecided;
- `wait` (blue tint, chevron) is on its way, such as Ordered;
- `ok` (green tint, check) is settled, such as Installed;
- `bad` (red tint, semibold, cross) is a problem, such as Overdue;
- `plain` (gray tint, small circle) is a fact, such as Planned.

A need still open on a done or dropped job shows a plain stage pill, because it asks nothing of
the owner. Every mark is drawn with CSS borders and gradients, so none depends on a glyph. The
environment name in the header uses the accent fill with no mark, because it is a standing
reminder and not a state. A count of things waiting on the owner sits in a square accent box.

**Only form fields have a boxed outline.** Text fields, selects and textareas have an outline and
the field background, and radios, checkboxes and the tick are drawn to match. A form sits in a
bordered panel. An error is a line with a red bar at its start, the invalid field gets a red border,
and the focus ring is the accent color.

**Each style of control has one role.** A solid button commits something. An outlined button
opens a form or a row. Underlined text does minor things, such as Delete, Show and Hide. The
control that opens a form and the button that submits it carry the same words (Mark ordered,
Return it, Add a to-do).

**Every date is `YYYY-MM-DD`.**

**The shop pages follow the same rules (2026-11-20).** `/shops` is one panel whose ledger lists
each shop with its address in gray beneath the name, its service and appointment counts right
aligned, and its last visit. A shop's page heads with its details and totals as labeled values (Invoiced, the invoices' grand totals including work others paid for, and You paid),
like an appointment's head. It then has one panel for the services, with the totals and the
owner's share right aligned as prices, and one for the appointments, which reuse the phase
pills and add no kind. A missing value is gray: "No services", "No total" and "Not booked". The shared layer writes "not booked", as it does for the home rows. The
pages hold no forms, because the shop directory changes only through Claude Desktop. A shop
name printed elsewhere links to its page, and on the work sheet that link sits in the on-screen
strip, so the printed sheet stays plain.

**A service's page follows the same rules (2026-11-24).** It heads with labeled values, like an
appointment's head: the vehicle, date, shop, odometer, invoice reference and amounts, with the
amounts as prices and "No total" in gray. Each kind of linked record then has its own panel (the
lines, the jobs completed, the quotes, the parts installed, the schedule items, the recalls and
the incidents), and a panel with no rows is left out. Every panel reuses the home page's waiting
columns, so the page adds no column template: a line's quantity and cost sit where a waiting row
shows its date and its job. A line's kind and an incident's kind are plain text, as on the home
page, and an incident's status reuses the `ok` and `plain` pills. A kit's items sit under its part
as they do in a need's row, and the one new stylesheet rule (`.row--wait > .memo`) starts that
list at the third waiting column on a wide window. The page holds no forms, because a service is
corrected only through Claude Desktop. A job's page links to the service that completed it as a
labeled value in its head.

**A vehicle's page follows the same rules (2026-11-24).** It heads with the name as the page
title, the year, make, model and trim on one line, and the car's facts as labeled values, like an
appointment's head. The VIN and the plate use the part-number style, because each mixes letters
and digits. The odometer is the last logged reading with its day, "No reading" in gray when
there is none, and the projected mileage is a separate value labeled "Estimated today" that
carries the word "estimated". An archived vehicle shows "Archived" as a labeled value and adds no
pill kind. The costs and the recent activity are two panels that reuse the home page's waiting
columns, with amounts right aligned as prices, and a panel with no rows is left out. An odometer
reading's row shows its notes, or "No note" in gray, because its kind and odometer columns
already state the reading. The monthly
breakdown of the cost summary stays in Claude Desktop, because a row per month would bury the
totals. The page holds no forms and adds no stylesheet rule. A vehicle's name printed on the
home, service, job and appointment pages links to it.

**A vehicle's printable history departs from the ledger as the work sheet does (2026-11-24).**
ADR 0009 sets that look and ADR 0025 sets the content. On screen and on paper the history is
plain text with no panels and no pills: a heading, the car on one line, labeled facts, then one
block per service and a list of the parts on the car. It reuses the work sheet's classes and
changes two print rules in `templates/base.html`: `.ws a` drops the underline from a link, and
`.ws .l` prints every label black, where the rule covered only the labels of the car's facts.
Both apply to every page printed this way, the work sheet included. On paper the site header is
hidden and the strip of on-screen links is left out.

**The page has no script.** The content security policy in `glovebox-web/src/guard.rs` allows
inline styles and fonts from this server, and nothing else loads.

## Alternatives considered

- **A decorative, luxury-brand look.** It suits a showroom, not a page for working with data.
- **Instrument-face skins over unchanged markup.** They changed the colors without answering what
  is a label, what is a value and what can be acted on, and each need row still stacked several
  text styles in one cell.
- **An HTML `<table>`.** A need opens in place with `<details>`, and a table row cannot be a
  `<summary>`. Shared grid templates align the columns without a table.
- **A font from a CDN, or the system font.** The policy forbids the first, and the second varies by
  machine and may lack the slashed zero and the `×` glyph the rows use.

## Consequences

- A new page reuses the tokens, the five pill kinds and the control styles; it does not add a
  color, a typeface, a fifth size or a boxed outline on something that is not a field.
- A new typed value that shows as a pill must be given a kind in `PillKind`.
- The font files ship inside the binary, so changing the typeface means rebuilding `glovebox-web`.
- A grayscale screenshot must still tell every pill apart by its mark and its word, and that is the
  check for any new pill kind.
