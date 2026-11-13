# Glovebox: project brief

Glovebox is a personal car maintenance record, and Claude Desktop is its only interface. The owner
talks to Claude, Claude reads scanned invoices and other sources, and Claude files the facts through
the MCP tools in `glovebox-mcp`. The full design is in
`docs/superpowers/specs/2026-11-12-mcp-first-reset-design.md`. Setup and the project prompt are in
`docs/claude-desktop/`.

## The question it answers

> Based on what I've had done to the car, my mileage and the car's age, what maintenance is coming,
> when, how much will it cost, and what parts might I consider buying and bringing to the mechanic?

The test case is a 2017 Golf GTI at 64k miles. Its 60k service is overdue, and the answer says so,
lists what that service includes, and folds in open recalls, shop advisories and
community-recommended preemptive fixes that fit the same shop trip.

## Owner and cars

- Steve, a software developer who prefers Rust.
- A 2017 Volkswagen Golf GTI (modded, 6-speed manual, FWD) and a 2017 Golf Alltrack (DSG, AWD). Both
  are on the MQB platform with the EA888 engine.
- Invoices are scanned to PDF and stay in the owner's own filing. Glovebox keeps the facts, not the
  files.

## What the system does

- **Records services** the way an invoice does: who did the work, over which dates, each line and
  its kind (maintenance, mod, repair, inspection, recall, fee), what it cost, and who supplied each part (the
  owner or the shop), with the seller and warranty expiry for parts the owner bought.
- **Captures shop advisories.** A thing the shop flagged but did not fix is an incident noticed during
  a service, plus a planned work item with a revisit-by date or mileage.
- **Tracks maintenance per item**, each on its own mileage or month interval, inherited from a
  platform and model template and adjustable per car. Claude groups overdue items into "your 60k
  service" when answering.
- **Tracks recalls** with a visible status (open, planned, done, not applicable), linked to the work
  that addresses them.
- **Answers "what's coming" in one call** (`whats_coming`): schedule status, planned work, open
  recalls, unresolved incidents, a 12-month cost forecast and warranty status.
- **Lets every mistake be fixed in the conversation** with correct, update and delete tools.

## Architecture

A cargo workspace of two crates. `glovebox-shared` is the domain library (SeaORM 2.0 on SQLite, the
services, the VIN decoder and the NHTSA recall lookup). `glovebox-mcp` is a thin rmcp stdio server
over it. Claude Desktop launches the binary, so no server runs between conversations. Money is
integer cents and dates are `YYYY-MM-DD`. `CLAUDE.md` describes the layout and the conventions.

## Not in scope

- Document storage, upload, OCR or file search.
- A web UI. One may come back later, one screen at a time, each justified by "I need to see X now".
- Integration with financier.
- Authentication or any non-local deployment. Whoever can run the process owns the data.
- Named milestone services (VW's 10k to 60k checklists as first-class objects). They are wanted
  eventually, and nothing in the data model should make them harder to add.

## Seeded maintenance data

A new database contains the VW MQB (EA888) platform, the Golf GTI and Golf Alltrack templates, and
their maintenance schedule items (`glovebox-shared/src/migration/m20261112_000002_seed_vw_mqb.rs`).
The seeded schedule is thin, so researching the manufacturer's real schedule and filling in items
and cost estimates with the schedule tools is expected early work.
