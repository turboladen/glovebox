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
cargo clippy --workspace -- -D clippy::pedantic
```
Must produce **zero warnings**. The crate-level `#![allow]` in the crate roots already suppresses intentional pedantic lints (`option_option`, `struct_field_names`, `wildcard_imports`).

### 2. Rust Tests
```bash
cargo test --workspace
```
All tests must pass.

### 3. Rust Build (release check)
```bash
cargo build --workspace
```
Ensures the project compiles without errors.

## Reporting

After all checks complete, report results in this format:

```
Preflight Results
─────────────────────────
 [PASS/FAIL] cargo clippy (pedantic)
 [PASS/FAIL] cargo test
 [PASS/FAIL] cargo build
─────────────────────────
 Overall: PASS / FAIL (N/3 passed)
```

If any check fails, show the relevant error output so the user can fix it.
