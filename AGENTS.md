# Agent Instructions

This project uses **bd** (beads) for issue tracking. Run `bd onboard` to get started.

## Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work atomically
bd close <id>         # Complete work
bd sync               # Sync with git
```

## Non-Interactive Shell Commands

**ALWAYS use non-interactive flags** with file operations to avoid hanging on confirmation prompts.

Shell commands like `cp`, `mv`, and `rm` may be aliased to include `-i` (interactive) mode on some systems, causing the agent to hang indefinitely waiting for y/n input.

**Use these forms instead:**
```bash
# Force overwrite without prompting
cp -f source dest           # NOT: cp source dest
mv -f source dest           # NOT: mv source dest
rm -f file                  # NOT: rm file

# For recursive operations
rm -rf directory            # NOT: rm -r directory
cp -rf source dest          # NOT: cp -r source dest
```

**Other commands that may prompt:**
- `scp` - use `-o BatchMode=yes` for non-interactive
- `ssh` - use `-o BatchMode=yes` to fail instead of prompting
- `apt-get` - use `-y` flag
- `brew` - use `HOMEBREW_NO_AUTO_UPDATE=1` env var

<!-- BEGIN BEADS INTEGRATION -->
## Issue Tracking with bd (beads)

**IMPORTANT**: This project uses **bd (beads)** for ALL issue tracking. Do NOT use markdown TODOs, task lists, or other tracking methods.

### Why bd?

- Dependency-aware: Track blockers and relationships between issues
- Version-controlled: Built on Dolt with cell-level merge
- Agent-optimized: JSON output, ready work detection, discovered-from links
- Prevents duplicate tracking systems and confusion

### Quick Start

**Check for ready work:**

```bash
bd ready --json
```

**Create new issues:**

```bash
bd create "Issue title" --description="Detailed context" -t bug|feature|task -p 0-4 --json
bd create "Issue title" --description="What this issue is about" -p 1 --deps discovered-from:bd-123 --json
```

**Claim and update:**

```bash
bd update <id> --claim --json
bd update bd-42 --priority 1 --json
```

**Complete work:**

```bash
bd close bd-42 --reason "Completed" --json
```

### Issue Types

- `bug` - Something broken
- `feature` - New functionality
- `task` - Work item (tests, docs, refactoring)
- `epic` - Large feature with subtasks
- `chore` - Maintenance (dependencies, tooling)

### Priorities

- `0` - Critical (security, data loss, broken builds)
- `1` - High (major features, important bugs)
- `2` - Medium (default, nice-to-have)
- `3` - Low (polish, optimization)
- `4` - Backlog (future ideas)

### Workflow for AI Agents

1. **Check ready work**: `bd ready` shows unblocked issues
2. **Claim your task atomically**: `bd update <id> --claim`
3. **Work on it**: Implement, test, document
4. **Discover new work?** Create linked issue:
   - `bd create "Found bug" --description="Details about what was found" -p 1 --deps discovered-from:<parent-id>`
5. **Complete**: `bd close <id> --reason "Done"`

### Auto-Sync

bd automatically syncs with git:

- Exports to `.beads/issues.jsonl` after changes (5s debounce)
- Imports from JSONL when newer (e.g., after `git pull`)
- No manual export/import needed!

### Important Rules

- ✅ Use bd for ALL task tracking
- ✅ Always use `--json` flag for programmatic use
- ✅ Link discovered work with `discovered-from` dependencies
- ✅ Check `bd ready` before asking "what should I work on?"
- ❌ Do NOT create markdown TODO lists
- ❌ Do NOT use external issue trackers
- ❌ Do NOT duplicate tracking systems

For more details, see README.md and docs/QUICKSTART.md.

## Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd sync
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds

## Code Quality

### Clippy
This codebase passes `cargo clippy -- -D clippy::pedantic` with zero warnings. Before submitting work:
```bash
cargo clippy -- -D clippy::pedantic   # Must pass with zero warnings
cargo test                             # All 47+ tests must pass
cargo fmt                              # Must be formatted
cd frontend && bun run check           # svelte-check must pass
```

### Crate-level Lint Allows
`main.rs` has `#![allow]` for intentional conventions — do NOT remove these:
- `clippy::option_option` — update DTOs use `Option<Option<T>>` by design
- `clippy::struct_field_names` — entity fields match DB column names
- `clippy::wildcard_imports` — `sea_orm::*` is idiomatic

### Required Patterns
- **`require_vehicle`**: All vehicle sub-resource handlers must call `require_vehicle(&state.db, vehicle_id).await?` at the top
- **`updated_at`**: All update handlers must explicitly set `updated_at` — SeaORM does NOT auto-set it
- **Batch loading**: List endpoints with related data must use `is_in()` batch queries, never N+1 loops
- **Path traversal**: File operations on user-provided paths must use `canonicalize()` + `starts_with()` checks
- **Numeric safety**: Use `i32::try_from()` for `usize→i32` conversions, integer division for cents→dollars formatting

## Testing

### Test Plan
`TEST_PLAN.md` is the living test plan. It contains manual smoke test steps (TP-01
through TP-12) and maps to Playwright e2e tests in `frontend/e2e/`.

**When adding or changing features:**
1. Update the relevant `TP-XX` section in `TEST_PLAN.md`
2. Add or update the corresponding Playwright test in `frontend/e2e/`
3. If adding a new feature area, create a new `TP-XX` section and spec file

### Running Tests
```bash
just test-e2e          # headless (requires `just dev` in another terminal)
just test-e2e-ui       # headed browser for debugging
cargo test             # backend unit tests
```

### Playwright Conventions
- Tests live in `frontend/e2e/*.spec.ts`
- Config at `frontend/playwright.config.ts`
- Tests run against `http://localhost:5173` (Vite dev server proxies API to backend)
- Use `test.beforeAll` to create test fixtures (vehicles, etc.) via the UI
- Prefer `getByRole`, `getByLabel`, `getByText` selectors over CSS selectors

<!-- END BEADS INTEGRATION -->
