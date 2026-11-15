# Web design pass: the ledger look

Date: 2026-11-14. Tracking issue: glovebox-701u. Reference mockup:
`docs/design/design-pass/d.html` (static, appointment page, light and dark).

## Intent

The web pages are a working record, like the notebook an owner keeps in the glovebox. The owner
reads many rows of parts and decisions and acts on a few of them. The look has to make three things
obvious on every row: what is a label printed on the page, what is a value in the record, and what
can be acted on. It also has to separate what still needs the owner from what is settled.

The earlier brief asked for a Porsche Design feel. Three mockups in that direction were judged too
decorative for a page whose job is working with data, and that brief is withdrawn.

## Goals

- The same kind of fact sits in the same column on every row, under a header.
- One typeface, one size for all data, two weights.
- Things waiting on the owner stand out, and settled things recede.
- No state is told apart by color alone (the owner has a mild color-vision deficiency).
- Light and dark themes both work.

## Non-goals

- No change to what the pages do: the routes, forms, redirects and the shared library stay as they
  are. `return_part` still requires a reason.
- No new page. Home and job pages take the same rules as the appointment page.
- Supplier and approach stay plain labeled values, not pills.

## Anti-goals

- More than four type sizes, or a second typeface.
- A boxed outline on anything that is not a form field.
- Decoration that carries no information (lined-paper textures, heavy bars, ornamental rules).
- Any script, external resource, or image file.

## Constraints

- The content security policy gains `font-src 'self'` and nothing else. Pill marks and the tick are
  drawn with CSS borders and gradients, which the policy does not restrict.
- IBM Plex Sans (SIL Open Font License 1.1) is compiled into `glovebox-web` with `include_bytes!`
  and served from GET routes. Ship unmodified `woff2` files from IBM's own release
  (github.com/IBM/plex): the regular and semibold weights, or the variable file. An unmodified
  file may keep the family name, which the license reserves, and the release's `LICENSE.txt` is
  copied beside the files. The mockup's `fonts/plex.woff2` is a third-party Latin subset, good for
  the mockup only. Confirm the shipped files carry the `zero` feature and the `×` glyph.
- Every count and sentence about the records still comes from a shared function. `view.rs` changes
  labels only, as listed under "Wording". A shared function may become `pub` so `view.rs` can call
  it, with no change to its logic.
- The layering gate, the request guard and the no-script rule are unchanged.

## Approaches considered

1. **Three instrument-face skins over the existing markup** (neutral with one accent, petrol
   ground, olive ground with a left rail). Rejected by the owner: attractive but not businesslike,
   and the need row still stacked five text styles in one cell.
2. **A ledger with real columns, new row markup and one text style.** Chosen. It answers the
   owner's actual complaints (what is form, what is value, what are my notes), and it keeps the
   petrol ground and panels the owner found easiest to read.
3. **An HTML `<table>`.** Not used. A need opens in place with `<details>`, and a table row cannot
   be a `<summary>`. Rows are CSS grids that share one column template, so columns align without a
   table.

## The design

### Text

| Role | Size | Weight | Color |
|---|---|---|---|
| Label (column header, field name, the word before a value) | 0.8125rem | 400 | label gray |
| Data (every value, sentence, pill and button) | 1rem | 400, or 600 for a row's name, a button and a needs-you pill | ink |
| Heading (group and job names, section titles, counts) | 1.1875rem | 600 | ink |
| Page title | 1.75rem | 600 | ink |

Gray text is always a label and never a value. A missing value ("Not decided", "No part chosen")
is the one exception and is also gray. Numerals are tabular, and the slashed zero is on everywhere.

### Color

Tokens, light then dark:

| Token | Light | Dark | Use |
|---|---|---|---|
| ground | `#CFDEDD` | `#0C262C` | page |
| panel | `#F3F7F6` | `#14353D` | a group or a job |
| inset | `#E3ECEA` | `#103039` | an open need |
| ink | `#0E2B31` | `#EAF2EF` | values |
| label | `#4A666C` | `#9DB9BB` | labels |
| rule | `#C6D5D3` | `#2A4C55` | row lines |
| field / field-line | `#FFFFFF` / `#6F8C91` | `#0A2126` / `#5E8288` | form fields |
| accent / on-accent | `#F4BE2C` / `#0E2B31` | `#F6C544` / `#0C262C` | needs the owner |
| ok / ok-bg | `#1D7352` / `#D3EADB` | `#7FD6A8` / `#1D4A3B` | settled |
| wait / wait-bg | `#1F5FA8` / `#D5E5F6` | `#8CC2F5` / `#1B4062` | on its way |
| bad / bad-bg | `#B5341F` / `#F7DAD3` | `#FF8A73` / `#5A2B24` | problem |
| plain-bg | `#DFE8E6` | `#24474F` | plain fact |

### Pills

A pill is a value from a fixed list. Every pill has a tint and a mark.

| Meaning | Tint | Mark | Values |
|---|---|---|---|
| Needs the owner | accent, weight 600 | empty box | Undecided, To order, Unconfirmed |
| On its way | wait-bg | chevron | Ordered |
| Settled | ok-bg | check | Received, Installed, Shop supplies, Done, Confirmed |
| Problem | bad-bg, weight 600 | cross | Does not fit, Rejected |
| Plain fact | plain-bg | small circle | Planned, Planning, and any other status |

The database name in the header uses the accent fill with no mark, because it is a standing
reminder and not a state. A count of things waiting on the owner (Check, Decide, Order, and a
job's open needs above zero) sits in a square accent box. Other counts are plain bold numerals.

Every mark is drawn in CSS, so none depends on a glyph the font may lack.

### Wording

The existing templates' wording stays except for these changes:

| Where | Wording |
|---|---|
| Fit labels (`view.rs`) | "Confirmed", "Unconfirmed", "Does not fit" |
| Supplier and approach with no value (`view.rs`) | "Not decided", under the labels "Supplier" and "Approach" |
| Quantity beside a need's name (`view.rs`) | "× 2" |
| The return opener and its submit button | "Return it" |
| The return form's reason field | "Why are you returning it?" |
| The to-do opener and its submit button | "Add a to-do" |
| The order opener and its submit button | "Mark ordered" |

### Controls

- A solid button commits something. An outlined button opens a form or goes to a row. Underlined
  text does minor things (Delete, Add a to-do, Return it in a step row, Show and Hide).
- An action keeps one name: the control that opens a form and the button that submits it say the
  same thing (Mark ordered, Return it, Add a to-do). "Supplier and approach" opens a form whose
  button says "Save".
- Text fields, selects and textareas are the only boxed things: 1px field-line, 4px radius, field
  background. Radios, checkboxes and the tick are drawn to match.
- A form sits in a bordered panel-colored box. An error is a line with a 4px bad-colored bar at its
  start, and the invalid field gets a 2px bad-colored border.
- The focus ring is 3px accent.

### Layout

- **Header.** Wordmark, "Database" label with the environment pill, the Garage link, and "Today"
  with the date. The reminder sentence sits below it in label style. An undo banner is a panel.
- **Appointment head.** The title, then labeled values in one line: Vehicle, Shop, Date, Status,
  Needs open, To-dos open, Jobs.
- **Next steps.** One panel per group with its name and count. Each row is job, need (weight 600),
  step sentence, action, in fixed columns. A form opens under the sentence. The job is a plain
  value with no label. A row in Done keeps its stage pill after the sentence and its "Return it"
  opener under it. The order form keeps the option's listing link.
- **By job.** One panel per job. Its head shows the name, the status pill and "N needs open". Below
  it is one header line: Need, Stage, Brand, Part number, Description, Seller, Price (right
  aligned), Fit. The head and the header line stick to the top of the window while the job's rows
  scroll.
- **Need row.** Line one fills the columns from the chosen option. A need with no chosen option
  shows, across the part columns, the same step sentence its Next steps row shows (from
  `checklist.rs`, capitalized in CSS as the step rows do), then the options note when it has
  options. That covers a need the shop supplies and a need settled by a part with no option. Line two
  starts at the Brand column: Supplier, Approach and Reason as label and value pairs. The purchase
  sentence, each need warning, and the option's link (when it has one) each take their own line
  below that, printed as the shared function gives them.
- **Open need.** The row takes the inset background and a 3px ink bar at its start. Its options
  follow in the same columns, with the radio at the end of the Stage column, so each option lines
  up under the chosen part. A blocked option is gray, a part that does not fit is struck through,
  and the reason sits on its own line. Then "Your decision" (the reason field and Save decision)
  and "Actions" (the outlined buttons), each named by a label in the Need column.
- **Narrow widths.** Below 68rem the header line is hidden and each row becomes a list of label
  and value pairs, the label taken from the column name.

## Left to the builder

- Class names, and how the templates are split.
- The exact narrow-width layout, checked by eye at phone and tablet widths.
- How the home page and the job page apply these rules. The home page lists appointments and
  unscheduled jobs as rows with labeled values.
- Where an option's link sits when the option is not the chosen one.
- Small adjustments to column widths so no seeded value wraps at 1280px.

## Verification

- `just ci` passes. The `glovebox-web` tests change only where they assert a label that changed,
  the markup around a value, or the policy string. A new test covers the font route.
- On `just web test` with seeded data: walk home, appointment and job in light and dark at wide and
  narrow widths against the mockup, and exercise choose, order, receive, return, tick and one
  error.
- The browser console shows no policy violation, and the font loads from `127.0.0.1`.
- A grayscale screenshot still tells every pill state apart by its mark and word.
