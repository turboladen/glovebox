# Using glovebox with Claude Desktop

`glovebox-mcp` is a local MCP server that Claude Desktop starts over stdio. It keeps the garage in
a SQLite database and exposes tools for recording services, planning work, and answering "what
does the car need?". Claude Desktop and the local web page described below are the interfaces to the
data.

## Build and install

1. Run `just install` from the repository root. It builds `target/release/glovebox-mcp` and prints
   the binary's path and the path of Claude Desktop's config file.
2. Merge the `glovebox` entry from
   [`claude_desktop_config.snippet.json`](claude_desktop_config.snippet.json) into the `mcpServers`
   object of `~/Library/Application Support/Claude/claude_desktop_config.json`. Create the file with
   the snippet's contents if it does not exist. Both paths in the snippet are absolute and point at
   this repository, so change them if the repository lives somewhere else.
3. Quit and reopen Claude Desktop.
4. Open Claude Desktop's connectors list and confirm that the `glovebox` server is connected and
   lists its tools, such as `record_service`, `whats_coming` and `plan_work`.

The server creates the database file, and its parent directory, on first start and applies the
migrations. The seeded VW MQB platform and its Golf GTI and Golf Alltrack templates are in every
new database.

## Connect NotePlan

Some of the car's history lives in NotePlan notes. NotePlan ships its own MCP server; enable it
from NotePlan's settings and connect it in Claude Desktop the way NotePlan documents. Glovebox does
not configure or read NotePlan. The project prompt tells Claude to read the notes when it needs
history that is not in glovebox yet.

## Start a project

Create a Claude Desktop project and paste the contents of
[`glovebox-project-prompt.md`](glovebox-project-prompt.md) as the project instructions. Have every
glovebox conversation inside that project so the instructions apply.

## First conversation

- [ ] Ask Claude to add the car. It calls `list_model_templates` and then `add_vehicle` with the
      matching template, so the car inherits its maintenance schedule.
- [ ] Give Claude the current odometer reading so it calls `log_mileage`. A mistyped reading is
      found with `list_mileage` and removed with `delete_mileage`; a car added twice by mistake is
      removed with `delete_vehicle`, which only works while the car has no history.
- [ ] Ask Claude to research the manufacturer's real maintenance schedule for the car and to
      reconcile it with `list_schedule`. The seeded schedule is thin: it has few items and no cost
      estimates. Claude adds missing jobs with `add_schedule_item` and adjusts existing items,
      inherited ones included, with `update_schedule_item`.
- [ ] Hand Claude one scanned invoice as a PDF and have it file the invoice. Then ask for the
      service back with `get_service` and check the lines, parts and suppliers.
- [ ] Ask "what's coming?" and check that the answer reflects what you just filed.
- [ ] Ask Claude to plan an appointment for an upcoming job. It calls `plan_appointment`, puts the
      jobs on it, adds what each job needs and the options it researched, and asks you for each
      decision. `get_checklist` then shows what is left to decide, order and receive.

## The web page

`just web` serves a page of what is open on each car at <http://127.0.0.1:4827/> and opens it in
the browser. On it you can set a need's supplier and approach, choose an option and give the
reason, mark a need ordered or received, mark a part wrong and being returned, and tick, add and
delete to-dos. Everything else, such as creating appointments, jobs, needs and options, happens in
Claude Desktop. The page listens on `127.0.0.1` only and reads the same database as the Claude
Desktop server, so both programs can run at once.

To try the page on made-up data, run `just seed test` and then `just web test`. The test
environment uses `data/glovebox-test.db` on port 4828. Every page shows its environment's name in
the header, `live` included, and the database file's name as well when `GLOVEBOX_DB_PATH` chose
the file.

## Upgrading

A new version can change the database's structure, and a program older than the database refuses
to open it. The Claude Desktop server only checks when it starts, so one that is already running
keeps serving after the page has updated the file. Upgrade in this order:

1. Stop the page if it is running.
2. Run `just install`, then quit and reopen Claude Desktop, so the server it starts is the new one.
3. Run `just web`, which builds the page from the same source before serving it.

## Environment

- **`GLOVEBOX_ENV`** names the environment. It defaults to `live`, which uses `data/glovebox.db`.
  Any other name, such as `test`, uses `data/glovebox-<name>.db`.
- **`GLOVEBOX_DB_PATH`** sets the database path. Without it the path follows `GLOVEBOX_ENV` and is
  relative to the directory the server starts in, so the snippets set an absolute path.
- **`RUST_LOG`** sets the log filter, such as `info` or `debug`. It defaults to `info`. Logs go to
  stderr, which Claude Desktop writes to `~/Library/Logs/Claude/mcp-server-glovebox.log`. Stdout
  carries only protocol frames.

## A test environment

To try the server against a throwaway database, add a second server entry by hand to Claude
Desktop's config, copying `claude_desktop_config.test.snippet.json`. It sets `GLOVEBOX_ENV` to
`test` and an absolute `GLOVEBOX_DB_PATH` ending in `data/glovebox-test.db`.
`just reset-env test` deletes that database; no recipe deletes the live one.

## Smoke test without Claude Desktop

Pipe an `initialize` request into the binary and look for a JSON-RPC response on stdout:

```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
  | GLOVEBOX_DB_PATH=/tmp/glovebox-smoke.db target/release/glovebox-mcp
```

## Backups

The server copies the database to `data/backups/` before any update that changes the database's
structure, and otherwise once a day while it holds a car. It keeps the newest 14 copies, the newest
copy of every month, and the largest copy. `just backup` (the `glovebox backup` command) makes a copy on demand and fails if there
is no database to copy.

The copies are on the same disk as the database. They protect against a deleted or damaged
database file, not against losing the folder or the computer, so include `data/` in whatever backs
up this machine.

To restore a copy:

1. Quit Claude Desktop.
2. Move `data/glovebox.db`, and `data/glovebox.db-wal` and `data/glovebox.db-shm` if they exist,
   to somewhere outside `data/backups/`.
3. Copy the backup file to `data/glovebox.db`.
4. Start Claude Desktop again.
