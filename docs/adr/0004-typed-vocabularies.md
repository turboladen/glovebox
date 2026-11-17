# ADR-0004: Typed vocabularies

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

Twelve columns take their value from a fixed list: an incident's category, a service's payer, a
part's status and so on. Each was a `String` checked against a `VALID_*` list in its service, and
the same text was compared as a bare literal (`"planned"`, `"self"`) in services, tests and the
web view. The MCP input schemas typed these arguments as strings, so Claude Desktop learned the
allowed values only from the prose of a description.

Three facts constrain the choice.

- `DeriveActiveEnum` builds an identifier from each stored text, and `"self"` becomes the keyword
  `Self`, so the derive does not compile for it under SeaORM 2.0. Three lists store `self`.
- A value the enum does not know fails the read of its whole row.
- Claude Desktop does not resolve `$ref`, and schemars writes a unit enum as a plain `enum` list
  only when no variant carries a doc comment.

## Decision

**Each vocabulary is one enum, defined once in `glovebox-shared`.** The `vocabulary!` macro in
`vocab.rs` takes `Variant => "text"` pairs and gives the enum the same text everywhere: the stored
column value (`DeriveValueType` over `FromStr` and `Display`), the serde form in tool arguments
and tool output, and the `as_str` a page prints. The enum lives in the entity module that owns its
column. An enum that no column stores, such as a status the code derives or a tool argument
that picks a behavior, lives in the service that uses it. The database stays TEXT and no stored
value changes.

**Text is parsed once, at the edge.** A tool argument is read by serde, and text from a web form
by `vocab::parse`, which names the field and lists the allowed values. Shared inputs and services
hold the enum and validate nothing.

**The tool schema lists the values.** `glovebox-shared` has an optional `schemars` feature, which
`glovebox-mcp` enables, so an input struct names the shared enum directly. A variant is explained
in the argument's description or in a `//` comment, never in a doc comment on the variant.

**A tool error names the argument.** A value outside the list is refused by serde before any
service runs. The `Parameters` wrapper reads arguments through `serde_path_to_error`, so the
message carries the path, such as `line_items[0].paid_by`, beside serde's list of allowed values.

**A migration refuses a value it cannot read.** `m20261115_000001_vocabulary_values` changes
nothing and stops, naming the table, column, count and values, while any vocabulary column holds
a value outside its list. Its lists are the values allowed when it was written and they stay
fixed, so a value an enum loses later still passes here and reaches the migration that rewrites
it.

## Alternatives considered

- **Mirror enums in `glovebox-mcp`, converted to shared types.** The shared crate would need no
  schemars feature, but each list would be written twice and kept in step by hand.
- **`DeriveActiveEnum` where it compiles and a hand-written impl for the three `self` lists.** Two
  mechanisms for one idea, and the reader has to know which enum uses which.
- **Taking the migration's lists from the enums.** One copy of each list, but a value removed
  from an enum would then be refused here, before the migration that rewrites it could run.
- **Storing `self` under another text.** The derive would compile, but the owner's rows would be
  rewritten and the stored value would differ from the one tools print.
- **CHECK constraints on the columns.** They would stop a bad value at the write, but the only
  writers are these enums, and adding a constraint to an existing table means editing its stored
  definition for all twelve columns.

## Consequences

- A value outside a list cannot exist in a shared input, a service or an entity, and the
  hand-written "Must be one of" checks are gone.
- A bad tool argument reads `category: unknown variant ...` in place of the service's own
  message, and every other malformed argument gains its path as well.
- Adding a value to a list is one line in the macro call. Removing or renaming one needs a
  migration that rewrites the stored rows first.
- Raw SQL that writes a vocabulary column bypasses the enum and must write a listed value.
- Some vocabularies have no column. For example, a recall's settable status (`open`,
  `done`, `not_applicable`) maps to stored finding text that differs, and the status it is shown
  with adds `planned`, which only a planned job produces.
