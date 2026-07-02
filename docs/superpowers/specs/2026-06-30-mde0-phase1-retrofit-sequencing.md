# mde0 — Phase 1 retrofit to personal-domain-pattern: sequencing plan

**Date:** 2026-06-30
**Epic:** `glovebox-mde0`
**Source of truth:** `../personal-domain-pattern/README.md` (Draft 2026-05-18)
**References:** Homie (reference impl: `backend/`, `shared/`, `mcp-server/` — 46 tools, 3 resources), fewd (MCP at `/mcp` on the backend port)
**Altitude:** planning / sequencing only. Per-child implementation design happens when each child is picked up.

---

## Goal

Align glovebox with the cross-app "personal domain" pattern so it matches Homie in shape:

```
glovebox-shared   (lib: SeaORM entities, migrations, services, validation, business logic)
glovebox-backend  (thin Axum HTTP/JSON over the lib — no SQL, no business logic in handlers)
glovebox-mcp      (MCP server: semantic domain-verb tools + read-only resources over the same lib)
```

This is Phase 1 of a three-phase arc (1 Retrofit → 2 Extract a shared crate → 3 Orchestrate across apps).
Phase 1 is "match the shape"; it is deliberately not extraction or orchestration.

## Current state

- Single `glovebox` binary crate, edition 2024, ~10.8k LOC.
- `src/{api,entities,migration,services,config.rs,main.rs}`; `services/` already holds the `ai/` provider trait.
- No `rmcp` dependency, no `*-mcp` binary, no FTS5.
- README status: glovebox is "⚠️ single crate" — **next up for Phase 1 retrofit**.

## Vocabulary discipline (Entity / Event / Document / Goal)

Stay typed and domain-specific; the four words are how we *think* cross-app, not trait names.

| Word     | glovebox mapping                                  |
| -------- | ------------------------------------------------- |
| Entity   | Vehicle                                            |
| Event    | Service, Mileage, Observation, Accident            |
| Document | Receipt, manual, photo (file + extracted text)     |
| Goal     | Form only — maps to two car-native concepts: recurring maintenance (`maintenance_schedule_item`, exists) + one-shot `build` (new). See Stage 0 decision. |

---

## Sequence

```
Stage 0  ◆ 0qiq   Decide Goal/Plan modeling          (decision, no code)   ── do first
                         │ informs the vocabulary of everything downstream
Stage 1  ▸ puby   Split → glovebox-shared + glovebox-backend   (the gate)
                         │ unblocks ↓↓
Stage 2  ▸ mq9r   search(query, scope) in shared + HTTP   ──┐ parallel
         ▸ mmiv   glovebox-mcp: domain verbs + resources  ──┘
                  constraint: mq9r's search() lands in shared
                  before mmiv wires the find_documents verb
```

The dependency graph fixes Stage 0 → 1 → 2. `puby` BLOCKS both `mmiv` and `mq9r` (declared in beads).
`0qiq` blocks no code but is sequenced first (decision) so the split and both surfaces are designed
knowing whether Goal is an Entity or a primitive. Stage 2's two children run in parallel after the
split, with one soft ordering: `find_documents(entity_id, query)` is `search()` scoped to documents,
so the shared `search()` should exist before that MCP verb is wired.

---

## What each child delivers + acceptance criteria

### Stage 0 — `0qiq`: Decide Goal/Plan modeling — ✅ RESOLVED 2026-06-30

**Decision:** "Goal" is a cross-app *form*, not a glovebox table (discipline: type things
domain-specifically; the four words are vocabulary, not table/trait names). The Goal form maps to two
glovebox-native concepts, and only one is a gap:

1. **Recurring maintenance targets** (10k service, oil every 5k) → **already served** by the existing
   `maintenance_schedule_item` (recurring Plan-template). No new primitive.
2. **One-shot upgrade / build / restoration targets** (upgrade turbo, engine swap, get it road-legal)
   → **new `build` primitive** (`builds` table, `Build` entity). Car-native name, deliberately not
   `goal`/`project` (Form-level names a converging sibling app would collide on).

**`build` modeling:** lightweight per-vehicle primitive (FK → vehicle), **not** event-sourced.
Progress **derived at query time** from linked typed Events/Parts (`service_record`, `part`,
`observation`) plus a lifecycle status/phase. No polymorphic `*_event` junction (glovebox's split
event tables make that awkward) — single-FK links or existing junction patterns, decided at
implementation time.

**Scope:** Plan-steps table deferred (YAGNI). "Hit 100k miles" milestone out of Phase 1 (derivable).
Latitude to restructure existing tables (`part`/`part_slot`/`service_record`) to seat `build` cleanly.

**Note:** Homie/Financier are pre-implementation and were **not** used as models — they should
converge on this decision, not the reverse.

**Captured in:** `0qiq` design field + `bd remember` key `mde0-goal-maps-to-build`.
**Done.** The build primitive is a candidate follow-up issue (implementation), tracked separately from
this decision.

### Stage 1 — `puby`: Workspace split (the gate)
- Create a cargo workspace.
- `glovebox-shared` (lib): `entities/`, `migration/`, `services/` (incl. the `ai/` provider trait),
  validation, and business logic.
- `glovebox-backend` (bin): thin Axum HTTP — extract typed inputs → call shared services → return
  typed outputs. **No SQL and no business logic in handlers.**
- Preserve the single-deployable-binary property (backend bakes in `frontend/dist`).
- This split is the prerequisite for a second surface (`glovebox-mcp`) over the same lib.

**Done when:** `cargo build` and `cargo test` are green; the single binary still serves the SPA from
baked-in `frontend/dist`; migrations still auto-run on startup via `Migrator::up()`; Playwright e2e
green. Behaviorally a no-op — the move re-homes code, it does not change behavior.

**Status (2026-07-02): resolved.** Split done via the Phase A mechanical move (PR #16) followed by
per-domain handler thinning B0–B16 (PRs #17, #18, #19). The layering boundary is enforced in CI by
`scripts/check-layering.sh` (no SeaORM usage in `glovebox-backend/src/api`, no axum in
`glovebox-shared`). The error model gained `DomainError::BadRequest` (verbatim 400s alongside the
field-attributed `Invalid`). Follow-up: ownership-check audit (move `vehicle::require` from handlers
into sub-resource service fns) filed as `glovebox-paxy`.

### Stage 2a — `mq9r`: `search(query, scope)`
- FTS5 virtual tables (migration) over Entity + Event text (service notes, observation, accident, etc.)
  and Document extracted text.
- Implement `search(query, scope)` in `glovebox-shared`; surface via HTTP API and (later) an MCP
  tool/resource.
- Use `Expr::cust()` for the FTS SQL per project migration conventions.

**Done when:** search returns ranked hits across vehicle + event text + document text; exposed as ONE
domain operation (not a separate FTS subsystem the UI/LLM must understand).

### Stage 2b — `mmiv`: `glovebox-mcp`
- `glovebox-mcp` crate using `rmcp` (current MCP Rust SDK) over `glovebox-shared`.
- Expose **semantic domain verbs**, not CRUD: e.g. `record_service(vehicle_id, ...)`,
  `log_observation(vehicle_id, note)`, `check_due_maintenance(vehicle_id)`,
  `summarize_recent_activity(vehicle_id)`, `find_documents(vehicle_id, query)`.
- Expose read-only **Resources** as stable URIs: a vehicle's full record, a goal's progress, a
  recent-activity feed.
- Mount approach: see D2.

**Done when:** an MCP client can drive the domain without knowing the schema (verbs read like domain
language; resources are stable addressable URIs).

---

## Key decisions & risks

| ID | Type     | Statement | Disposition |
| -- | -------- | --------- | ----------- |
| D1 | Decision | Goal = Entity (own Events) vs distinct primitive | ✅ RESOLVED 2026-06-30 (`0qiq`): Goal is a form; new car-native `build` primitive (lightweight, derived progress) + existing `maintenance_schedule_item`. |
| D2 | Decision | MCP mount: standalone binary (Homie) vs `/mcp` on the backend port (fewd) | Decide at `mmiv` start. Read both refs first. Standalone keeps surfaces independently deployable; same-port keeps one binary. |
| R1 | Risk     | Business logic embedded in `src/api/` handlers | Stage 1 needs a **handler audit** to pull logic down into `shared`. The move is behaviorally a no-op; test coverage is the safety net. |
| R2 | Risk     | Single-deployable-binary + startup migration auto-run | Must survive the split — backend keeps baking `frontend/dist` and running `Migrator::up()` on boot. |
| R3 | Risk     | Regression during a large refactor | Keep `TEST_PLAN.md` + Playwright e2e green as the regression guard through every stage. |

## Non-goals (Phase 1)

- No shared database / universal entity table — storage stays per-app.
- No Phase 2 extraction (`personal-domain-core`) — that waits until ≥3 apps share the shape.
- No Phase 3 orchestrator / agent memory.
- No JSON blob columns — schemas stay typed and domain-specific.

## Provenance

Scoped 2026-06-30 from epic `glovebox-mde0` and `../personal-domain-pattern/README.md`. Sequence
confirmed with Steve: deliverable = epic sequencing plan; `0qiq` first.
