---
name: preflight
description: |
  Run all linting, type checking, and tests in parallel to verify the codebase
  is clean before committing. Reports unified pass/fail status.
  Trigger on: "preflight", "check everything", "run all checks", "verify",
  "pre-commit check", "is everything passing".
user_invocable: true
---

# Preflight Check

Run all validation checks in parallel and report a unified status.

## Checks to Run (in parallel)

Launch these as parallel subagents or parallel bash commands:

### 1. Rust Clippy (pedantic)
```bash
cargo clippy -- -D clippy::pedantic
```
Must produce **zero warnings**. The crate-level `#![allow]` in `main.rs` already suppresses intentional pedantic lints (`option_option`, `struct_field_names`, `wildcard_imports`).

### 2. Rust Tests
```bash
cargo test
```
All tests must pass.

### 3. Frontend Type Check
```bash
cd frontend && bun run check
```
Runs `svelte-check` + TypeScript validation. Must produce zero errors.

### 4. Rust Build (release check)
```bash
cargo build
```
Ensures the project compiles without errors.

## Reporting

After all checks complete, report results in this format:

```
Preflight Results
─────────────────────────
 [PASS/FAIL] cargo clippy (pedantic)
 [PASS/FAIL] cargo test
 [PASS/FAIL] frontend check (svelte-check + TS)
 [PASS/FAIL] cargo build
─────────────────────────
 Overall: PASS / FAIL (N/4 passed)
```

If any check fails, show the relevant error output so the user can fix it.

## Optional Extended Checks

If the user says "full preflight" or "extended", also run:

### 5. E2E Tests (requires dev server)
```bash
just test-e2e
```
Note: This requires `just dev` running in another terminal. Warn the user if it's not running.

### 6. Frontend Build
```bash
cd frontend && bun run build
```
Ensures the production frontend build succeeds.
