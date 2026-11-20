# ADR-0021: A shop is one record

- **Status:** Accepted
- **Date:** 2026-11-21

## Context

Services and appointments named their shop in the free-text `shop_name`. The owner's records held
one dealer under two names ("Clovis Volkswagen" and "Volkswagen of Clovis"), one shop with and
without its address ("KW Automotive" and "KW Automotive (520 N Blackstone, Fresno)"), and two
locations of one chain told apart only by an address inside the name. A question such as "what
has this shop cost me" had no single place to look. The `shops` table and the `shop_id` columns
existed with no write path (ADR-0013).

## Decision

**A shop is a row of the shop directory, and a service or appointment names it by `shop_id`.**
`shop_name` stays as a column, always NULL, and a CHECK on each table keeps it so. Every read
prints the name of the shop `shop_id` names, under the same `shop_name` key clients already read.
A search finds a service by its shop's name, address or notes through an FTS index on `shops`.

**The directory refuses a second shop of the same name** (ASCII case and surrounding spaces aside)
unless both carry different addresses, as two locations of one chain do. A shop with no address
must get one before a second location is added.

**A tool names a shop by `shop_id`, or by a `shop_name` that matches exactly one shop.** A name
that matches none is refused and points at `add_shop`; a name that matches several is refused and
lists them. Sending both arguments is refused. A blank name names no shop.

**Duplicates are merged, not deleted.** `merge_shops` moves every service and appointment to the
kept shop, which keeps its name and takes each detail it lacks; notes are joined, and each value
the kept shop already had a different one of is listed in `discarded`. `delete_shop` refuses a
shop that any record names, because the foreign key's `ON DELETE SET NULL` would leave the
history with no shop.

**`get_shop` reads a shop's history** across every vehicle: its services with each total and the
owner's out-of-pocket share, and its appointments as every appointment read summarizes them.

**The upgrade moves every stored name into the directory**
(`m20261121_000002_shops_from_names`). A trailing `(…)` becomes the address. Names that are equal
by the same comparison become one shop unless their addresses differ; then each address is its
own shop, and the entries with no address share one more. In the owner's records the two KW
entries become one shop with its address, the two America's Tire locations stay two shops, and
the two Clovis dealer names stay two shops until the owner merges them. The migration reuses a
shop already in the directory with the same name and address.

## Alternatives considered

- **Keeping `shop_name` as a copy that a rename or merge rewrites.** Readers and search would not
  change, but the same fact would live in two places, and every write that missed one would make
  them disagree.
- **Keeping `shop_name` as the invoice's wording beside `shop_id`.** It gives the column two
  meanings, and a merge would leave the old spelling on every moved row.
- **Creating a shop from an unknown name.** It saves the client one call, but every new spelling
  would become a duplicate to merge later, which is the problem this decision removes.
- **Merging names that differ** (by edit distance or a list of synonyms). "Clovis Volkswagen" and
  "Volkswagen of Clovis" share no prefix, and only the owner knows two names are one shop.

## Consequences

- A client lists the shops before naming one, and asks the owner before adding a shop that looks
  like a listed one.
- The upgrade leaves the two dealer names as two shops, so the first `list_shops` shows both and
  the owner merges them once.
- A shop with no address next to a location with one, which only the upgrade can produce, makes a
  name ambiguous, so a client names either one by id.
- An older build reads the NULL `shop_name` as no shop, and its writes of a shop name are refused
  by the CHECK, so every program is rebuilt together (the usual upgrade order applies).
