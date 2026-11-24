# ADR-0024: VIN attributes are readable

- **Status:** Accepted
- **Date:** 2026-11-24

## Context

A VIN decode from NHTSA returns about 140 non-empty values for a car. Eight of them fill the
vehicle record (year, make, model, trim, body style, engine, transmission and drivetrain), and
all of them are stored as `vehicle_attributes` rows with the source `vin_decode`. No tool read
those rows, so the plant, the door count, the airbag locations and the rest were stored and
unreachable.

The decode ran only inside `add_vehicle`. A car added without a VIN, or added while NHTSA was
unreachable, had no way to be decoded short of deleting it and adding it again, and
`update_vehicle` setting a VIN runs no lookup.

The `add_vehicle` handler made the NHTSA request itself and passed the answer to the shared
service, which put a step of the domain's sequence in the MCP crate.

## Decision

**`list_vehicle_attributes` reads the stored attributes.** It returns every attribute of the
vehicle as `key`, `value` and `source`, sorted by key, and an empty list for a car with no stored
decode. The `get_vehicle` description names it. The decoder already leaves out empty values, so
the read filters nothing.

**`decode_vin` decodes a car on file.** It takes a `vehicle_id`, looks the stored VIN up, replaces
the vehicle's `vin_decode` attributes with the answer, and fills each of the eight fields that is
empty on the vehicle. A field that holds a value keeps it, whoever set it, which is the
precedence `add_vehicle` has: the caller, then the model template, then the decode. The reply
carries the `vehicle`, `filled_fields` and `attributes_stored`.

**A failed lookup is a reply, not an error.** As in `add_vehicle`, the reply carries
`vin_decode_error`, and the vehicle and its stored attributes stay as they were. A vehicle with
no VIN is refused with a message that names `update_vehicle`.

**The shared library owns the whole sequence.** `vehicle::create_with_vin` and
`vin_decode::decode_for_vehicle` check what can be refused, make the request, and then open their
write transaction, so no write lock is held while NHTSA answers. Each has a `pub(crate)` form
that takes the lookup as a closure, as `research::check_recalls_with` does, so the tests run
without the network. `decode_for_vehicle` reads the vehicle again inside its transaction and
refuses when the VIN is not the one it looked up.

## Alternatives

- **An `attributes` field on the `get_vehicle` reply.** `get_vehicle` is called in most
  conversations, and 140 rows of mostly irrelevant values on each call would crowd out the record
  the client asked for.
- **Decoding inside `update_vehicle` whenever the VIN changes.** A correction to a plate or a
  note would then depend on a web request, and the owner could not decode a car whose VIN was
  already right.
- **Letting `decode_vin` overwrite the eight fields.** The decode often returns every trim or
  engine the model line offers, and the owner's or the template's value is the more exact one.
  `update_vehicle` changes a field the owner wants replaced.
- **Reporting a failed lookup as a tool error.** The client would read it as a refusal of the
  car, and `add_vehicle` and `check_recalls` both report a failed lookup inside a normal reply.

## Consequences

- A car added before this decision is decoded with one `decode_vin` call, and its attributes
  are then readable. Nothing is migrated: the rows `add_vehicle` stored are read as they are.
- `decode_vin` on a car that is fully described fills nothing and still refreshes the
  attributes, so it is safe to run again.
- An attribute NHTSA stops returning disappears from the vehicle on the next successful decode,
  because the stored set is replaced as a whole.
- `vin_decode::store_attributes` is public so the MCP tests can store a decode without the
  network. It checks that the vehicle exists.
- A blank vehicle name is not refused before the request, because no service refuses one.
