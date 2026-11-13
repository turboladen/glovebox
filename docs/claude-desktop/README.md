# Using glovebox with Claude Desktop

`glovebox-mcp` is a local MCP server that Claude Desktop starts over stdio. It keeps the garage in
a SQLite database and exposes tools for recording services, planning work, and answering "what
does the car need?". Claude Desktop is the only interface to the data.

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

## Environment

- **`GLOVEBOX_DB_PATH`** sets the database path. It defaults to `data/glovebox.db` relative to the
  directory the server starts in, so the snippet sets an absolute path.
- **`RUST_LOG`** sets the log filter, such as `info` or `debug`. It defaults to `info`. Logs go to
  stderr, which Claude Desktop writes to `~/Library/Logs/Claude/mcp-server-glovebox.log`. Stdout
  carries only protocol frames.

## Smoke test without Claude Desktop

Pipe an `initialize` request into the binary and look for a JSON-RPC response on stdout:

```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
  | GLOVEBOX_DB_PATH=/tmp/glovebox-smoke.db target/release/glovebox-mcp
```

## Backups

The server copies the database to `data/backups/` before any update that changes the database's
structure, and otherwise once a day while it holds a car. It keeps the newest 14 copies, the newest
copy of every month, and the largest copy. `just backup` makes a copy on demand and fails if there
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
