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

These are the gates `just ci` and CI run; the Justfile is the source of truth for each command.

### 1. Format
```bash
just fmt-check
```
Nightly rustfmt, because `rustfmt.toml` uses nightly-only options.

### 2. Layering
```bash
just check-layering
```

### 3. Spell-check
```bash
just typos
```
Needs `typos-cli` (`brew install typos-cli`).

### 4. Clippy over every target
```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
```
Must produce **zero warnings**. `[workspace.lints.clippy]` in `Cargo.toml` denies the pedantic group,
and `--all-targets` lints the tests too.

### 5. Tests
```bash
cargo test --workspace --locked
```

### 6. Build
```bash
cargo build --workspace --locked
```

## Reporting

After all checks complete, report results in this format:

```
Preflight Results
─────────────────────────
 [PASS/FAIL] fmt-check
 [PASS/FAIL] check-layering
 [PASS/FAIL] typos
 [PASS/FAIL] cargo clippy
 [PASS/FAIL] cargo test
 [PASS/FAIL] cargo build
─────────────────────────
 Overall: PASS / FAIL (N/6 passed)
```

If any check fails, show the relevant error output so the user can fix it.
