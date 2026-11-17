# ADR-0006: Claude Desktop over MCP is the main interface

- **Status:** Accepted
- **Date:** 2026-11-12

## Context

Glovebox exists to answer one question: given what has been done to the car, its mileage and its
age, what maintenance is coming, when, what will it cost, and which parts are worth buying and
bringing to the shop. Most of the facts behind that answer sit on scanned invoices, and the hard
part of the job is getting them in accurately.

The first version was a single-page web app over an HTTP API, with builds, visits and document
storage. Its forms were too unreliable to enter a car's history, and it was shelved with one car
and a handful of thin records. That code is kept under the `pre-mcp-reset` git tag. Claude can
read an invoice and fill in a structured record, so the interface is the conversation itself.

## Decision

**Claude Desktop is the main interface, and it talks to glovebox over stdio.** The `glovebox-mcp`
binary is launched by Claude Desktop, so no server has to run between conversations. Stdout
carries only protocol frames, and every diagnostic goes to stderr. Every record the owner keeps
can be created, read, corrected and deleted through a tool, so no fact requires another program.

**Tools are domain verbs a person would say.** `record_service`, `plan_work`, `whats_coming` and
`check_recalls` name the owner's intent, not a table, and Claude never has to know the schema or
pick between near-duplicate tools. One `record_service` call files a whole invoice: its lines,
parts and links to schedule items, jobs, recalls and incidents, in one transaction. The invoice
number is the duplicate guard, so filing the same scan twice returns the stored record.
`whats_coming` answers the question above in one call.

**The facts an invoice carries are structured, not left in notes.** A service has a performer
(`shop` or `self`), a start date and an optional end date. Each line has a work kind and each
part a supplier. A shop advisory is an incident noticed during a service plus a planned job. A
recall's status is derived from the jobs and services linked to it, except `not_applicable`,
which the owner sets. Inspection results stay in the service notes, and the mods on a car are
read from mod lines and installed parts, with no separate configuration record.

**All logic lives in `glovebox-shared`.** SQL, validation and business rules are service
functions that take a connection and return `DomainResult<T>`. Each program is a thin surface
over them: an MCP tool body maps arguments to a domain input, calls one service and serializes
the result. `scripts/check-layering.sh` keeps rmcp and axum out of the library.

**There is no HTTP API.** The local web page (`glovebox-web`) renders HTML and accepts form posts
against the same services. It binds `127.0.0.1` only and exposes no JSON endpoints, so there is
nothing for another client to call. MCP has no authentication, because whoever can run the
process already owns the database file.

**Resources are read-only views.** `glovebox://vehicles`, `glovebox://vehicles/{id}` and
`glovebox://vehicles/{id}/activity` let a client browse without a tool call. Every write is a
tool.

**One helper maps domain errors to MCP.** `domain_error` turns `NotFound`, `Invalid` and
`BadRequest` into tool error results whose message reaches the model, so Claude can correct the
call. `Db` and `Internal` become opaque JSON-RPC internal errors, with the detail kept in tracing.
Resource reads use a matching helper that maps `NotFound` to `resource_not_found`.

**Schedules stay per item.** The platform, model template and vehicle hierarchy supplies each
car's schedule, every item keeps its own interval, and schedule links attach to a service, not a
line. Claude groups overdue items into a named service when it answers.

## Alternatives considered

- **Fixing the web forms.** The forms were the part that failed, and a model reading the invoice
  does the data entry better than a person retyping it.
- **Keeping the HTTP API and the old frontend beside MCP.** A second client surface would have to
  track every schema change.
- **Streamable HTTP for MCP.** It needs a server that stays up and a port to guard. Stdio needs
  neither, and Claude Desktop manages the process.
- **Generic CRUD tools per table.** The model would need the schema and would have to sequence
  writes that belong in one transaction.
- **Dropping resources in favor of tools.** They cost little to keep beside the tools.

## Consequences

- A new feature starts as service functions with unit tests in `glovebox-shared`, then gets a tool
  and coverage in `glovebox-mcp/tests`. A page or a CLI command reuses the same services.
- Tool and field descriptions are the user interface. They carry the wire syntax, and service
  messages must read correctly both in a tool result and on the page.
- A stray `println!` anywhere the MCP binary calls breaks every tool call. `tests/stdio.rs` fails
  when stdout carries anything but JSON-RPC frames.
- Document storage is out of scope. Invoices stay in the owner's own filing, and a service keeps
  the facts and the invoice number, never a file path.
- A web page earns its place one screen at a time, when the owner needs to see something a
  conversation shows poorly.
- Named milestone services remain possible, because nothing collapses schedule items into one list.
