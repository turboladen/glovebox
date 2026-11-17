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
