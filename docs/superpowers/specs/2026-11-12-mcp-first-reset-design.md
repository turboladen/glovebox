# Glovebox MCP-first reset — design

**Date:** 2026-11-12 · **Status:** awaiting approval

## Intent

Glovebox was built web-first and grew a large surface (six-tab SPA, document storage, builds,
visits) before it was ever used for real. Getting history into it through the web forms was buggy
enough that the app was shelved with one car and 18 thin service records in it.

The reset makes Claude Desktop the only read and write interface. The MCP server and the data
behind it get hardened against real use first. A web UI comes back later, one screen at a time,
each one justified by "I need to see X now".

The question the app exists to answer, asked in Claude Desktop:

> Based on what I've had done to the car, my mileage and the car's age, what maintenance is
> coming, when, how much will it cost, and what parts might I consider buying and bringing to the
> mechanic?

The concrete test case is the 2017 GTI at 64k miles: the 60k service is 4k overdue, and the answer
should say so, list what that service includes, and fold in open recalls, shop advisories and
community-recommended preemptive fixes that make sense to do in the same shop trip.

## Goals

1. Everything the owner needs to record or correct can be done through MCP tools. Nothing requires
   a web form.
2. A service record holds the detail on a real invoice: who did the work, over which dates, what
   each line was (maintenance, mod, repair, inspection), what it cost, and who supplied each part.
3. "What's coming" is one tool call that merges scheduled maintenance, planned work with a
   revisit-by, open recalls and open incidents.
4. The scanned invoices get filed by Claude Desktop reading each PDF and calling the tools.

## Non-goals

- Document storage, upload, OCR or file search. Invoices stay in the owner's own filing; glovebox
  keeps the facts and no pointer to the file.
- Any web UI in this project.
- Integration with financier's advisor MCP.
- Named milestone services (see "What the owner decided").
- MCP authentication or non-local deployment.
- Preserving existing data or migration compatibility. The current database is disposable.

## Anti-goals

- A tool surface where Claude must know the schema, or must choose between near-duplicate tools.
- Facts the owner asked to have structured (part supplier, work kind, revisit-by) ending up only
  in free-text notes.
- Building any UI before stage 3 has been lived with.

## What the owner decided

- Incidents stay.
- Document storage goes.
- Builds go. Mods are things that happen at a service, including DIY work such as an ECU flash.
- Visits go as a separate concept. A service has a start and an end date instead, because the car
  is often at the shop for several days.
- Shop advisories must be captured: "took it in for X, asked them to look at Y, they said Y is
  going out but can last another six months".
- No invoice path or file reference on a service.
- Maintenance is tracked per item, each on its own interval, and Claude groups overdue items into
  "your 60k service" when answering. Named milestone services (VW's 10k … 60k checklists as real
  objects) are wanted eventually and are out of scope here; nothing in this work should make them
  harder to add. The seeded GTI schedule is thin (two items at 60k, no cost estimates), so
  researching VW's actual schedule and filling in items and costs through the schedule tools is
  part of stage 3.
- Existing data can be wiped; no migration path is needed.
- The web UI will be rebuilt incrementally later.

## Design calls made by the author (not yet confirmed by the owner)

Each of these is the author's judgment. The first two were put to the owner directly and
confirmed on 2026-11-12.

1. **Delete the frontend, the Playwright suite and the HTTP API now** (recoverable from a
   `pre-mcp-reset` git tag). Keeping them frozen would mean every schema change below must also
   keep ~20 unused handlers and a SPA compiling, and the dropped tables must stay. The
   `glovebox-backend` crate is removed; it returns when the first web screen is needed.
2. **Claude Desktop connects over stdio**, the way financier's `advisor-mcp` does: `glovebox-mcp`
   gains a binary that Desktop launches itself, so there is no server to keep running. The
   Streamable HTTP router is removed along with the backend that mounted it.
3. **One squashed initial migration** replaces the 22 existing ones, containing only the kept
   tables in their new shape.
4. **The platform → model template → vehicle schedule hierarchy stays**, with its VW MQB seed
   data. It is invisible in use and is where the GTI's schedule comes from.
5. **Schedule links stay at the service level**, not per line item. A service counts as
   factory-scheduled when it is linked to a schedule item marked factory-recommended.
6. **Inspection results** (pass/fail, certificate number) go in the service notes, not in
   dedicated columns.
7. **The invoice number stays as the duplicate guard.** The owner ruled out a file path on a
   service; the existing `invoice_ref` column is the number printed on the invoice, so it is kept.
8. **"What mods are on the car"** is derived from mod-kind line items and installed parts. There
   is no separate current-configuration record.

## Data model

Kept as they are unless noted: `vehicles`, `platforms`, `model_templates`,
`maintenance_schedule_items`, `service_schedule_links`, `mileage_log`, `shops`,
`research_reports`, `research_findings`, `incident_followups`, and the FTS tables for kept
entities.

Removed: `builds`, `visits`, `documents`, and every `build_id` / `visit_id` column and FTS table
that refers to them. `vehicle_attributes` stays, because the VIN decoder writes to it.

### Service record

- `performed_by`: `shop` or `self`. Shop name stays as it is for shop work.
- `service_date` becomes the start date; a new optional `end_date` is the pick-up date. A same-day
  job has no end date.
- `invoice_ref` stays and keeps its present job. It holds the invoice or work order number printed
  on the paperwork (never a file path), it is unique per vehicle, and re-recording a service with
  the same number returns the existing record. Filing a stack of scans is the case that guard
  exists for. Renaming it to `invoice_number` is the builder's choice. There is no separate work
  order field.
- `build_id` is removed.

### Service line item

- `work_kind`: one of `maintenance`, `mod`, `repair`, `inspection`. Required. A service that is
  both maintenance and a mod has lines of each kind. This is separate from the existing free-text
  `category` column, whose fate is the builder's choice.

### Part

- `supplied_by`: `self` or `shop`. Existing `seller`, `installed_service_id` and warranty-expiry
  fields carry the rest ("I bought the brakes from FCP Euro, warranty until …").
- `build_id` is removed.

### Work item (the to-do list)

- `revisit_by_date` and `revisit_by_miles`: both optional. Either makes the item show up in
  "what's coming" with a due status, the same way a schedule item does.
- Status is `planned`, `done` or `dropped`. `visit_id` and `build_id` are removed.

### Incident

- A new category, `shop_observation`.
- A way to say which service an incident was noticed during. Whether that reuses
  `incident_service_links` with a role or adds a column is the builder's choice.
- `build_id` is removed.

An incident links to services in two roles: the service it was **noticed during** and the service
(or services) that **resolved** it. The owner wants to follow that chain from either end, e.g. a
2023 service where the mechanic noticed a failing valve and the 2024 service that replaced it:

- Reading a service returns the incidents noticed during it, each with its current status and the
  resolving service if there is one, and the incidents it resolved, each with the service where it
  was first noticed.
- Reading an incident returns both links and its follow-ups.
- Recording a service can name the incidents it resolves. Completing a work item that is linked to
  an incident through a service resolves the incident with that service.

A shop advisory is therefore two linked records: a `shop_observation` incident tied to the service
where it was noticed, and a work item for the fix with a revisit-by.

### Recalls

Recalls found by the NHTSA check are stored as research findings, as they are now, deduplicated
by campaign number. The owner's complaint about the current app is that a stored recall is hard to
tie to the work that addresses it, so the link is first-class:

- Each recall has a status the owner can see: `open`, `planned` (a work item is linked to it),
  `done` (a service is linked to it, with that service's date), or `not_applicable` (the campaign
  covers the model year but not this car). `planned` and `done` are derived from the links;
  `not_applicable` is set by hand.
- Recording a service can name the recalls it performed. Planning work can name the recall it
  addresses. Completing that work item through a service carries the recall to `done`.
- The recall check returns every known recall with its status and its linked work item or
  service. It does not present already-known recalls as new.
- Open and planned recalls appear in the "what's coming" answer.

The NHTSA lookup is by make, model and year, not by VIN, so it cannot say whether a recall was
performed on this car. `done` comes only from the owner's own records. The Desktop project prompt
therefore tells Claude, when filing an invoice, to match any campaign number on it (NHTSA or
manufacturer, e.g. 24V110 / 20UF) against known recalls and link the service. A tool to mark a
recall done or not applicable without a service covers work that left no paperwork.

## MCP surface

The tool set is the whole user interface, so it has to cover creation and correction for every
kept concept. Names follow the existing rule: a verb a person would say.

**Removed:** `list_builds`, `get_build_progress`, `update_build_status`, `schedule_visit`,
`complete_visit`, `cancel_visit`, `attach_document`, `find_documents`. References to builds,
visits, documents and deep links are removed from the remaining tool descriptions.

**Changed:**

- `record_service` takes the new fields, line items with `work_kind`, and parts with
  `supplied_by` in the same call. It also takes ids of work items it completes and recall findings
  it resolves, incidents it resolves, and incidents noticed during it, alongside the schedule item
  ids it already takes. All of it commits as one unit.
- `check_due_maintenance` becomes the single "what's coming" answer: schedule items with status,
  open work items (with revisit-by status where set), unresolved recalls, unresolved incidents,
  and the 12-month cost forecast. Renaming it is the builder's choice.
- `plan_work` takes revisit-by date and mileage.
- `log_incident` accepts `shop_observation` and the noticed-during service.

**Added** (exact names are the builder's choice):

- Add a vehicle and update a vehicle. Today a vehicle can only be created in the web UI. Adding a
  vehicle with a VIN decodes it through NHTSA and fills in whatever details the caller left out;
  a failed lookup does not block adding the car.
- Correct and delete for services, parts, incidents and work items. An import mistake must be
  fixable in the same conversation.
- Add, update and disable a schedule item for one vehicle, including its estimated cost. This is
  how a biennial smog check or a community-recommended preemptive replacement becomes a recurring
  item, and how cost estimates improve as real invoices arrive.
- Mark a work item done or dropped without recording a service.
- Mark a recall done or not applicable without recording a service.
- Read one service in full (lines, parts, and its linked schedule items, recalls, work items and
  incidents in both roles) and read one incident in full, if no existing tool already returns
  that.

`check_recalls` changes as described under Recalls.

## Claude Desktop setup

Mirror `../financier/docs/claude-desktop/`: a `just install` recipe that builds the release
binary, a config snippet for `claude_desktop_config.json`, and a project prompt. The prompt tells
Claude how to file an invoice (one `record_service` call with lines and parts, then link schedule
items, recalls and work items it satisfies, then log any advisory the shop wrote on it) and how to
answer the "what's coming" question (call the due-maintenance tool first, research costs and
parts second, save anything durable as a research finding or schedule estimate).

## Stages

1. **Trim.** Tag `pre-mcp-reset`. Update every Rust dependency to its latest stable release,
   majors included, once the removed code is gone, so the new work is not built on old versions. Remove the frontend, e2e suite, HTTP API and backend crate.
   Remove builds, visits and documents from the domain library and MCP. Squash migrations. Add the
   stdio binary. Close the open beads that only concern removed code, each with a reason pointing
   at this reset.
2. **Fill the gaps.** The data model and tool changes above, each with service-level unit tests in
   `glovebox-shared` and coverage in the MCP integration test. Wrong-parent ids must return the
   same `NotFound` as nonexistent ones, as the project already requires.
3. **First real use.** Install into Claude Desktop, start from an empty database, add the GTI,
   file the scanned invoices, then ask the target question. Shortfalls found here become beads and
   get fixed before any UI work is considered.

Stage 3 is done by the owner in Claude Desktop, not by the builder. The builder's work ends with a
working install and a rehearsal against a throwaway database: add a vehicle, file one realistic
multi-line invoice that includes a self-supplied part and a shop advisory, and confirm the
"what's coming" call reflects all of it.

## Left to the builder

- Tool names for the added tools, and whether `check_due_maintenance` is renamed.
- How the noticed-during link on incidents is stored.
- Whether line item `category` survives next to `work_kind`.
- Which open beads close as obsolete and which still apply.
- How `CLAUDE.md`, `AGENTS.md`, `TEST_PLAN.md`, the `justfile`, CI and the project skills
  (`new-entity`, `new-e2e-test`) are updated or removed to match.
- Whether resources (`glovebox://…`) are kept over stdio or dropped in favor of tools.
