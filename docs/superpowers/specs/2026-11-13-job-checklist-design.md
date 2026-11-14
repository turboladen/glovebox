# Job checklist and the first web page: design

Date: 2026-11-13. Status: approved by the owner on 2026-11-13.

## Intent

Glovebox can say what maintenance is coming. It cannot help the owner follow through on it.
Preparing for the GTI's 60k service, the owner researched parts with Claude, got a long open-ended
list in a notes app, became overwhelmed, forgot to order some parts and ordered two wrong ones
(rear rotors for a car without the Performance Pack, and no rear pads at all). Every fact needed to
avoid that was known. Nothing tracked it.

An expected job means a chain of decisions for each thing it needs: do I supply it or does the
mechanic, which part, direct replacement or upgrade, bought from whom, have I ordered it, has it
arrived, is it on the car. The owner wants a concise list that shows every need has a decision and
every decision has been carried out, with boxes to tick. The reasons for each decision are kept so
the owner can read them later and so Claude can use them the next time the same choice comes up.

This list is also the first thing the owner wants to see and click on a real page, so this work
brings back a web UI as one narrow slice.

## Goals

1. A job (the planned work the app already has) can list what it needs, and each need carries its
   supplier, its researched options, the decision with its reason, and its purchase.
2. A job collects needs and research long before it has a shop or a date. Research often starts
   months ahead ("plugs at 60k") or turns up while researching something else.
3. An appointment groups jobs for one shop trip and has a purpose in the owner's words
   ("60k service + shore up loose ends"). Its shop and date are optional until known.
4. A to-do list view answers "what do I still have to do?" in the order the owner does it: decide,
   order, wait for arrival, other to-dos. Finished things fold away.
5. A fitment verdict sits on every researched option, so a part that does not fit this car is
   visibly rejected and cannot be chosen.
6. Plain to-dos ("return the HP Plus pads", "ask KW to confirm the VAQ differential") live on the
   same list.
7. The owner can decide and tick things off on a local web page. Claude Desktop can do everything
   the page can, and remains the place where research and paperwork happen.
8. Claude can ask what the owner decided before for a kind of part, and why, across all vehicles.

## Non-goals

- Any web page beyond the two described here. The rest of the web UI is still built one "I need X
  now" slice at a time.
- Creating appointments, jobs, needs or options on the page. Claude does that in conversation.
- A phone layout. The owner uses the page at a desk while ordering parts. It must not break at
  narrow widths, and it is not designed for them.
- Login or access from another machine.
- Named milestone services (`glovebox-3c0g`), quotes (`glovebox-5ncf`), wear measurements
  (`glovebox-v2lc`) and research-finding CRUD (`glovebox-iac2`). Nothing here should make them
  harder.
- Price tracking, order-status lookups or any call to a retailer.

## Anti-goals

The work has failed, even if it runs, when:

- the list reads as open-ended prose that has to be re-read to find what is left to do;
- a need can be sitting undecided or unordered without the page showing it;
- a part known not to fit can be chosen or ordered without a warning;
- the page and Claude Desktop disagree, or something exists only on the page;
- the page passes judgment ("you are behind") where it should state counts.

## The model, in the owner's words

- **Job**: one piece of planned work, such as "front and rear brakes". It exists today as planned
  work (`work_items`). It may belong to one appointment or to none.
- **Need**: one thing a job requires, such as "rear rotors". New.
- **Option**: one researched candidate for a need, such as "Zimmermann 600.3241.20". New.
- **Appointment**: one shop trip, grouping jobs. New.
- **To-do**: a plain checkbox for the vehicle, optionally on an appointment. New.

### Need

A need belongs to one job and holds:

- what is needed (name) and how many;
- who supplies it: `self`, `shop`, or not decided yet;
- the approach: `replacement`, `upgrade`, or not decided yet;
- the chosen option, when there is one;
- the reason for the decision, in the owner's words;
- the part record, once it is bought.

A need's **stage** is derived, never stored. The first row that holds wins, so a linked part
decides the stage whatever the supplier and chosen option say:

| Stage | Holds when |
| --- | --- |
| `installed` | The linked part is installed. |
| `received` | The linked part has a received date. |
| `ordered` | A part is linked. |
| `shop_supplies` | The supplier is `shop`. Nothing more is asked of the owner. |
| `to_order` | The supplier is `self` and an option is chosen. |
| `undecided` | Anything else. |

Linking a part does not require a chosen option. It sets the supplier to the part's own supplier
when the need has none. A need with a linked part and no chosen option is listed with a plain
statement that no decision is recorded, so the reason can be added afterward.

A shop-supplied need may still have options and a chosen one, because "ask the shop for the OE
part" is a decision worth recording. It never asks for an order.

### Option

An option belongs to one need and holds manufacturer, part number, description, seller, price,
link, and:

- **fitment**: `confirmed`, `unconfirmed` or `does_not_fit`, with a note giving the evidence
  ("340 x 30 mm, Performance Pack, checked against sales code AU16X2");
- **rejected**, with the reason, when the owner or Claude rules it out.

Rejected options are kept. They are the history Claude learns from. Choosing an option whose
fitment is `does_not_fit` is refused. Choosing one whose fitment is `unconfirmed` is allowed, and
the list keeps saying so until the fitment is confirmed.

### Buying

"Mark ordered" on a need creates the part record from the chosen option (name, manufacturer, part
number, seller, price, link), with the owner as supplier and the order's date, price and order
number, and links it to the need. The part record is the single home of purchase and warranty
facts, as it is today.

A need can instead be linked to a part that is already recorded. The eight FCP parts already in the
live database for the 60k service must be attachable to their needs without being re-entered.
Existing parts are not backfilled with a received date: a linked part reads `ordered` until someone
marks it received, and the link action accepts a received date so Claude can ask the owner once and
set both.

"Mark received" stores the date the part arrived on the part. Arrival is its own step because the
box check on arrival is when a wrong part is caught.

### Returning a wrong part

A need that is `ordered` or `received` has a "wrong part, returning it" action that takes a reason
and whether the chosen option itself was the mistake. In one write it:

- marks the part `returned` (a fourth part status beside `purchased`, `installed` and `replaced`)
  with the reason and the date, and keeps the record;
- unlinks the part from the need, which sends the need back to `to_order`, or to `undecided` with
  the option marked rejected for that reason when the option was the mistake;
- adds a to-do "Return <part> to <seller>" on the need's appointment, or on the vehicle when the
  job has none.

A returned part is left out of parts spending in `cost_summary` and stays visible in the need's
history and in past decisions, so Claude can say what was ordered by mistake before. The page and
Claude can both do this.

### Appointment

An appointment belongs to one vehicle and holds a purpose (required), a shop (optional, named the
way a service record names its shop), a date (optional), notes, and a status: `planning`, `done` or
`cancelled`. Whether it is booked is read from the date.

- A job is assigned to an appointment, moved to another, or unassigned at any time. Its needs,
  options and reasons go with it.
- Filing the invoice names the appointment. Nothing is installed or completed by naming it: the
  jobs the invoice completes and the parts it installs are named explicitly, as they are today,
  because a shop does not always fit everything that was brought. The completed jobs need not all
  belong to the appointment. A part the shop supplied, created inline on the invoice, may name the
  need it fills. The result of filing lists every need on a completed job whose part is linked and
  not installed, so Claude can ask whether it was fitted.
- `done` is derived: an appointment is done when it has at least one job and every job on it is
  done or dropped. If a service is deleted and its jobs reopen, the appointment is open again. An
  appointment with no jobs is `planning`.
- Two things are stored: `cancelled`, and a closed date set by hand. Closing or cancelling while
  jobs are still planned unassigns those jobs, so unfinished work returns to the unscheduled pool
  with its research intact.
- Deleting an appointment never deletes a job.

### To-do

A to-do belongs to a vehicle, optionally to an appointment, and holds a title, notes and the date it
was done. Ticking and un-ticking are both allowed.

## The to-do list view

One function in the domain library produces the checklist for an appointment, and the same shape
for the unscheduled jobs of a vehicle. Both the page and the MCP read tool render it. It contains:

- the appointment (purpose, shop, date) and counts: needs open, to-dos open;
- **next steps**, grouped in this order: `decide` (stage `undecided`), `order` (`to_order`),
  `arriving` (`ordered`), `todos` (open to-dos), then `done` (everything else, including
  shop-supplied needs);
- **by job**: each job with its needs, every need with its stage, supplier, chosen option, fitment,
  reason, purchase summary, and its other options with their fitment and rejection reasons;
- warnings, each a plain statement: a chosen option whose fitment is unconfirmed, a linked part
  whose part number differs from the chosen option's.

`whats_coming` gains a short entry per open appointment (purpose, date, open counts) so the
canonical question mentions outstanding preparation.

## MCP surface

Claude needs tools to create and edit all of the above and to read the checklist. The builder
chooses the exact tool names and whether closely related edits share a tool, following the
conventions in `CLAUDE.md` (names a person would say, `deny_unknown_fields`, double-option for
clearable fields, no `$ref`, byte-identical `NotFound` for wrong-vehicle ids, the pinned tool list
test). The capabilities required:

- appointments: create, update (purpose, shop, date, notes, status), read the checklist, list,
  delete;
- jobs: `plan_work` and `update_planned_work` accept an appointment; a read of unscheduled jobs
  with their needs;
- needs: add, update (name, quantity, supplier, approach, chosen option, reason), delete;
- options: add, update (including fitment and rejection), delete;
- buying: mark ordered (creating the part), link an existing part, unlink, mark received;
- to-dos: add, update, tick, un-tick, delete;
- `record_service` accepts an appointment;
- past decisions: given a search text ("brake pads"), return the needs across all vehicles whose
  name or options match, each with the chosen option, the reason, the rejected options with their
  reasons, and the date.

The project prompt in `docs/claude-desktop/` gains a section on planning a job: add needs as the
research finds them, record every option considered with a fitment verdict and its evidence, record
why an option was rejected, and ask the owner for the supplier and decision without choosing for
them.

## The web page

### What the owner can do on it

- Set a need's supplier and approach.
- Choose an option from those listed and write the reason.
- Mark a need ordered, with seller, date, price and order number (prefilled from the option).
- Mark a need received.
- Mark an ordered or received part as wrong and being returned, with the reason.
- Tick, un-tick, add and delete to-dos.

Everything else is read-only on the page.

### Pages

- `/`: for each vehicle, its open appointments (purpose, date, open counts) and a "Not scheduled
  yet" list of jobs that have needs and no appointment.
- `/appointments/{id}`: the layout the owner chose from the mockups (option C). The next-steps list
  is on top, with one row per thing to do and its action beside it. The by-job view is below, one
  block per job with a row per need and the need's options, fitment and reason shown with it.
- A view of one unscheduled job with the same need rows and the same actions.

The mockup the owner reviewed is kept at
`.superpowers/brainstorm/87370-1794600449/content/appointment-list.html` (git-ignored, on the
owner's machine). It fixes the structure and the content of each row. Its colors do not carry over.

### How it is built

These follow the sibling project financier (`../financier/crates/advisor-web`), which the owner
named as the model:

- A third crate, `glovebox-web`, with its own binary, depending on `glovebox-shared`. It runs no
  queries and owns no tables or migrations. Every number or sentence on a page comes from a
  function in `glovebox-shared` that has its test there.
- axum and askama, server-rendered HTML, **no JavaScript**. Every action is an HTML form post
  followed by a redirect back to the page. After ticking a to-do or marking a need received, the page
  shows a banner with an undo form. Marking a need ordered has no undo on the page, because it
  creates a purchase record; a mistaken one is corrected through Claude. Supplier, approach, chosen
  option and reason are simply edited again. A failed write re-renders the page with the domain error's message beside the row.
- One inline stylesheet in the base template with light and dark schemes. Buttons have accessible
  names. The look is described under "Look and feel".
- It binds `127.0.0.1` only, on `GLOVEBOX_WEB_PORT` (default 4827), reads `GLOVEBOX_DB_PATH`,
  rejects requests whose Host or Origin is not local, and sends a content security policy that
  allows no scripts and no external resources.
- `just web` builds the binary, opens the browser and serves the page.
- Tests drive the router in process over an in-memory database with a fixed clock.

`scripts/check-layering.sh` keeps axum and rmcp out of `glovebox-shared` and gains a check that
`glovebox-web` contains no SeaORM queries.

### Look and feel

The owner likes Porsche Design, and the Chronograph 1 watch of the 1970s in particular, and found
the old site's black, red, white and silver scheme a cliche. The direction is that sensibility and
not a copy of it: an instrument face. That means matte near-black and off-white surfaces, type
chosen for legibility first with tabular numerals, thin rules in place of boxes and shadows, a
strict grid, and one accent color used sparingly for the thing that needs attention. No Porsche
marks, typefaces or imagery.

Red and green may be used for state, and the owner has a mild color-vision deficiency, so every
state is also a word and no pair of states differs by hue alone.

The builder uses the `frontend-design` skill, and shows the owner a static mockup of the
appointment page in this style, in light and dark, before the templates are finalized. The
owner's approval of that mockup is a gate in the plan.

### Environments

The owner needs to try changes against a test database without touching live records.

- `GLOVEBOX_ENV` names the environment and defaults to `live`. `live` uses `data/glovebox.db`, and
  any other name uses `data/glovebox-<name>.db`. An explicit `GLOVEBOX_DB_PATH` still wins. Both
  binaries follow the same rule through one function in `glovebox-shared`.
- `just web env=test` serves that environment. A non-live environment uses a different default
  port (4828) so it can run beside the live page, and every page shows the environment's name in
  the header so the two cannot be confused.
- A small command-line program, `glovebox`, holds everything the owner runs by hand, so its rules
  are enforced in tested code and the justfile recipes are single calls with no shell logic. The
  two servers only serve. Its commands: `backup`; `seed --env <name>`, which fills an environment
  with made-up data that exercises every stage (a vehicle, an appointment, jobs, needs at each
  stage, an option that does not fit, a returned part and to-dos); and `reset --env <name>`, which
  deletes that environment's database.
- `seed` and `reset` require a name, refuse `live`, and build the file name themselves, so neither
  can reach the live database. Nothing deletes or overwrites the live database.
- `docs/claude-desktop/` gains a second config snippet for a test server entry
  (`GLOVEBOX_ENV=test`), which the owner adds by hand when wanted.

### Two programs, one database

Claude Desktop's `glovebox-mcp` and `glovebox-web` open the same SQLite file at the same time.
`db::connect` already uses WAL with foreign keys on, and gains a busy timeout (5 seconds) so a
write from one waits for the other. Both run migrations on start. A binary older than the
database's newest migration must refuse to start with a message that says to rebuild. The builder
confirms what sea-orm-migration does in that case and adds the check if it does not already fail.

## Constraints

- **The live database holds the owner's real records.** Migrations are additive and rerun-safe,
  and are verified against a copy of `data/glovebox.db`. Nothing in this work resets, rewrites or
  deletes existing rows.
- Do not touch any Claude Desktop configuration file.
- Naming: tables are named for the car-native thing (`appointments`, `job_needs` or similar,
  `need_options`, `todos`), never for a generic pattern word. `work_items` keeps its name; "job" is
  the owner-facing word for it.
- All conventions in `CLAUDE.md` hold: layering, error model, cross-reference guards with
  wrong-parent tests, integer cents, `YYYY-MM-DD` dates, comment rules, pedantic clippy.

## Left to the builder

- A rejected option cannot be chosen until its rejection is cleared.
- The date a decision was made is stored on the need and is the date past decisions report.
- To-dos with no appointment are listed on `/` under their vehicle.

- Table and column names, tool names and grouping, and the split of work into tasks.
- How quantity maps onto part records (one part record per need, following what `record_part`
  does today).
- Whether "received" is a date column on `parts` or lives on the need, provided the stage rules
  above hold and a part linked to no need can still be marked received.
- Template structure, CSS, and the wording of labels, within the design rules above.

## Done means

1. From Claude Desktop, the 60k appointment can be built from the real data: jobs assigned, needs
   added, the three rear-rotor options recorded with the 272 mm one marked `does_not_fit`, the eight
   existing FCP parts linked.
2. `just web` shows that appointment. Its next-steps list shows rear pads under decide and the rear
   rotors and front pads under order.
3. Choosing an option, marking it ordered and marking it received on the page each move the row to
   the next group, and Claude sees the change on its next read.
4. Asking Claude "what did I choose for rear rotors and why?" returns the decision, the reason and
   the rejected options.
5. Filing the invoice against the appointment marks the parts installed and the appointment done.
6. `just ci` passes.
