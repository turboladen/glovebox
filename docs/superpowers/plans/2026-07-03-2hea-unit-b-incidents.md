# 2hea Unit B — Incident Unification: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Observations + accidents unify into one `incident` primitive (constrained categories, followups on any incident, M2M service links, recurrence chains) — fixing the taxonomy hole the NotePlan import exposed.

**Architecture:** Migration 000018 creates `incidents` + `incident_followups` + `incident_service_link`, copies all rows with a deterministic id-offset mapping (observations keep their ids; accidents get `offset + id`), rebuilds FTS (incidents + followups + the builds arm absorbed from `skde`), drops the old tables. One `services/incident.rs` replaces two services; MCP gets `log_incident` + `save_note` (tools 16→17); the UI gets a deliberately minimal interim Incidents tab (unit F's Timeline is the real home). Spec: Unit B of `docs/superpowers/specs/2026-07-02-2hea-feature-reassessment-design.md`. This is the riskiest unit — R-note: verify the migration on a populated DB copy, not just `test_db()`.

**Tech Stack:** Rust workspace, SeaORM migrations (raw SQL for FTS + mixed DML), rmcp, Svelte 5, Playwright.

## Global Constraints

- Bead `glovebox-w5xt`. Branch `2hea/unit-b-incidents` (checked out). No push/PR. Never stage `.beads/`.
- Every task ends green; `just fmt` before commits; full `just ci` + e2e + **populated-DB migration verification** before done.
- **Category whitelist (lossless union — existing data stays valid, no remapping):** `general | noise | leak | warning_light | cosmetic | performance | obd_code | damage | accident | note`. `save_note` uses `note`; migrated accidents get `accident`.
- Paxy discipline on every new link: followups/service-links/recurrence vehicle-scoped, wrong-parent indistinguishable from nonexistent, regression test each.
- Old routes/tabs die outright (pre-ship app, no compat shims). Minimal interim UI only — do not polish what unit F restructures.
- Commits end `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: Schema + migration 000018

**Files:**
- Create: `glovebox-shared/src/migration/m20260301_000018_unify_incidents.rs` (register, appended)
- Create: `glovebox-shared/src/entities/incident.rs`, `incident_followup.rs`, `incident_service_link.rs` (register in `entities/mod.rs`; field order = creation order exactly)
- Delete: `glovebox-shared/src/entities/observation.rs`, `accident.rs`, `accident_correspondence.rs`, `accident_service_link.rs` (+ mod lines + any vehicle.rs relations)

**`incidents` columns (creation order = entity order):**
`id` PK AUTOINCREMENT · `vehicle_id` INT NOT NULL FK→vehicles CASCADE · `category` TEXT NOT NULL · `title` TEXT NOT NULL · `description` TEXT NULL · `odometer` INT NULL · `occurred_at` TEXT NOT NULL · `obd_codes` TEXT NULL · `resolved` BOOLEAN NOT NULL DEFAULT 0 · `notes` TEXT NULL · `fault` TEXT NULL · `other_party_name/phone/email/insurance/policy_number` TEXT NULL ×5 · `insurance_claim_number/adjuster/adjuster_phone` TEXT NULL ×3 · `total_repair_cost_cents` INT NULL + `_currency` TEXT NULL · `deductible_cents` INT NULL + `_currency` TEXT NULL · `insurance_payout_cents` INT NULL + `_currency` TEXT NULL · `recurrence_of_id` INT NULL FK→incidents(id) ON DELETE SET NULL · `build_id` INT NULL · `created_at`/`updated_at` TEXT NOT NULL DEFAULT `(datetime('now'))`. Index `idx_incidents_vehicle`.

**`incident_followups`:** `id` PK · `incident_id` INT NOT NULL FK→incidents CASCADE · `occurred_at` TEXT NOT NULL · `contact_method` TEXT NULL · `contact_with` TEXT NULL · `summary` TEXT NOT NULL · `notes` TEXT NULL · `created_at` TEXT NOT NULL DEFAULT `(datetime('now'))`. Index on incident_id.

**`incident_service_link`:** `incident_id` INT NOT NULL FK→incidents CASCADE · `service_record_id` INT NOT NULL FK→service_records CASCADE · composite PK. (Mirror `accident_service_link`'s shape — read it.)

**Data copy (deterministic id-offset; raw SQL, in this order, all one migration):**
1. Copy observations preserving ids: `INSERT INTO incidents (id, vehicle_id, category, title, description, odometer, occurred_at, obd_codes, resolved, notes, build_id, created_at, updated_at) SELECT id, vehicle_id, category, title, description, odometer, observed_at, obd_codes, resolved, notes, build_id, created_at, updated_at FROM observations`.
2. `offset` = `(SELECT COALESCE(MAX(id),0) FROM observations)` — inline the subselect, don't compute in Rust.
3. Copy accidents: `INSERT INTO incidents (id, vehicle_id, category, title, description, odometer, occurred_at, resolved, notes, fault, <all accident-only cols>, created_at, updated_at) SELECT id + offset, vehicle_id, 'accident', substr(description, 1, 100), description, odometer, occurred_at, resolved, notes, fault, <cols>, created_at, updated_at FROM accidents` (title = first 100 chars of the NOT NULL description).
4. Followups: `INSERT INTO incident_followups (incident_id, occurred_at, contact_method, contact_with, summary, notes, created_at) SELECT accident_id + offset, ... FROM accident_correspondence` (id NOT preserved — fresh autoincrement is fine, nothing references followup ids).
5. Links: accident links `SELECT accident_id + offset, service_record_id FROM accident_service_link`; observation resolutions `INSERT INTO incident_service_link SELECT id, resolved_service_id FROM observations WHERE resolved_service_id IS NOT NULL` — use `INSERT OR IGNORE` on the second in case of duplicates.
6. Fix the AUTOINCREMENT sequence: `UPDATE sqlite_sequence SET seq = (SELECT MAX(id) FROM incidents) WHERE name = 'incidents'` (guard with INSERT OR IGNORE into sqlite_sequence first if no row exists).
7. **FTS:** drop the 9 old triggers + 3 FTS tables (`fts_observations`, `fts_accidents`, `fts_accident_correspondence`); create `fts_incidents` (cols: title, description, obd_codes, notes — content=incidents) + `fts_incident_followups` (summary, notes) + **`fts_builds` (name, description — the skde arm)**, each with the canonical external-content trigger trio (copy the exact pattern from 000013, `IF NOT EXISTS` throughout) + `'rebuild'`.
8. Drop old tables (children first): `accident_correspondence`, `accident_service_link`, `accidents`, `observations`.
9. `down()`: irreversible error (house pattern from 000015/000016).

- [ ] **Step 1:** Read 000004/000005 (source table DDL), 000013 (FTS pattern), `accident_service_link.rs`. Write migration + entities. Entity relations: incident belongs_to vehicle + build, has_many followups, self-referencing recurrence (mirror how `part` did belongs_to; a self-FK relation may need a custom def — if SeaORM fights it, skip the relation enum and query by column, noting it).
- [ ] **Step 2:** Compile fallout: `services/observation.rs`/`accident.rs` and everything referencing them will break — Task 2 replaces them; if a single commit for Tasks 1+2 is needed to stay green, that's sanctioned (unit C precedent).

### Task 2: `services/incident.rs` + inputs (replaces observation + accident services)

**Files:**
- Create: `glovebox-shared/src/services/incident.rs`, `glovebox-shared/src/inputs/incident.rs`
- Delete: `services/observation.rs`, `services/accident.rs`, `inputs/observation.rs`, `inputs/accident.rs` (+ mod lines)
- Modify: `glovebox-shared/src/services/activity.rs` (observation queries → incident, kind stays `"incident"`... use kind `"incident"` and update the feed's consumers), `glovebox-shared/src/services/search.rs` (arms: observations+accidents+correspondence → incidents+followups+builds; `SearchScope`: `Observations|Accidents` → `Incidents`, add `Builds`; parse strings `incidents`/`builds`, drop the old two), `glovebox-shared/src/services/build.rs` + `service_record.rs` (observation link references → incident)

**Service fns (merge the two deleted services' surfaces; read both first):**
- `list(db, vehicle_id)` (occurred_at desc) · `get(db, vehicle_id, id)` → view with followups + linked service ids (batch, no N+1) · `create(db, vehicle_id, NewIncident)` — category whitelist (Global Constraints list, house message shape), `occurred_at` defaults now, `service_record_ids` M2M guarded (`require_service_records_owned` — lift from accident.rs), `recurrence_of_id` guarded same-vehicle (NotFound "Incident {id} not found"), `build_id` guarded (existing pattern) · `update(db, vehicle_id, id, UpdateIncident)` — same guards, txn when links change, `updated_at` stamped, `resolved` togglable · `list_followups`/`create_followup(db, vehicle_id, incident_id, ...)` — vehicle-scoped via the incident (lift `require_accident` → `require_incident`).
- Inputs mirror the union field set; double-option on nullable update fields (house convention).

**Tests (TDD where new logic, port where behavior carries):** port the meaningful observation/accident tests to incident; NEW: category whitelist rejection incl. steering message for bad category; recurrence wrong-vehicle → NotFound + nothing mutated; recurrence chain round-trip; followup wrong-vehicle scoping; migrated-shape queries (the migration itself is proven by test_db + Task 5's populated check).

- [ ] Steps: write/port tests red → implement → `cargo test --workspace` green → commit (with Task 1): `feat(2hea-b): incidents unify observations+accidents (migration 000018, service, FTS+builds arm)`.

### Task 3: Backend routes + MCP verbs

**Files:**
- Delete: `glovebox-backend/src/api/observations.rs`, `accidents.rs`; Create: `glovebox-backend/src/api/incidents.rs`; Modify: `api/mod.rs`, `main.rs` (routes: `/api/vehicles/{vehicle_id}/incidents` GET/POST, `/{id}` GET/PUT, `/{id}/followups` GET/POST — old observation/accident routes die)
- Modify: `glovebox-mcp/src/schemas.rs` + `handler.rs`: `log_observation` → **`log_incident`** (category enum in the doc: the full whitelist; description steers "collisions/crashes → category `accident`"; fields: title, category?, description?, odometer?, occurred_at?, obd_codes?, notes?, build_id?, recurrence_of_id?) + **`save_note`** (vehicle_id, note → thin alias: `incident::create` with category `note`, title = first 80 chars of note, description = full note; description: "Remember something about this vehicle — a fact, a preference, a memory. Saved as a note incident, searchable later."). Tools 16 → **17**; `search_records` scope doc gains `incidents`/`builds`, drops `observations`/`accidents`; activity/resource wording updated.
- Integration tests: tools/list 17; log_incident round-trip incl. category rejection; save_note → searchable via search_records (proves the FTS trigger); builds scope finds a seeded build by name; wrong-vehicle probes.

- [ ] Steps: thin handler (extract→shared→Json) → MCP verbs → tests green → commit: `feat(2hea-b): incidents API + log_incident/save_note MCP verbs (builds searchable)`.

### Task 4: Interim UI (deliberately minimal)

**Files:**
- Delete: `frontend/src/components/ObservationsTab.svelte`, `AccidentsTab.svelte`; Create: `IncidentsTab.svelte`; Modify: `VehicleDetail.svelte` (two tabs → one "Incidents"), `api.ts`/`types.ts` (incident types/API replace observation+accident), `frontend/e2e/observations.spec.ts` → `incidents.spec.ts` (rewrite: create incident w/ category, accident-category incident shows insurance fields, followup add, resolve toggle), other specs referencing observations (grep), `TEST_PLAN.md`, `CLAUDE.md` (17 tools, entity/migration counts, tab list)

**Minimal = combined list (occurred_at desc) + category filter chips + create/edit form (accident-only fieldset revealed when category=accident) + followups under an expanded incident + resolve toggle + linked-services display.** No recurrence UI beyond a read-only "recurrence of #N" line (unit F's Timeline does it properly).

- [ ] Steps: build tab → rewrite specs → `bun run check` 0 errors → commit: `refactor(2hea-b): interim Incidents tab replaces Observations+Accidents`.

### Task 5: Gates + populated-DB verification

- [ ] **Populated-DB check (mandatory, unit-C precedent):** copy the dev DB if populated — else seed a scratch copy at migration state 000017 with: 2 vehicles, observations (varied categories, one resolved w/ resolved_service_id, one with build_id), 2 accidents (one with correspondence ×2 + service links ×2 + insurance fields), then boot the branch backend against it. Verify with sqlite3: row counts match (obs+acc = incidents), id-offset mapping correct (spot-check a followup's incident points at the right migrated accident), links complete (incl. the resolved_service_id conversion), categories intact + 'accident', FTS finds an accident description AND a followup summary AND a build name via `/api/search`, `PRAGMA foreign_key_check` clean, old tables gone.
- [ ] `just ci` exit 0; `just test-e2e-ci` (report count arithmetic vs 44); boot smoke: old routes 404, `/api/vehicles/{id}/incidents` 200, `/mcp` tools/list 17.
- [ ] Commit docs bits if any remain: `docs(2hea-b): counts + TEST_PLAN`.

## Self-Review
- Spec coverage: union schema ✓ (whitelist adjusted to reality — lossless union, noted); followups generalize ✓; M2M links + resolved_service_id conversion ✓; recurrence ✓; FTS rebuild + builds arm (skde) ✓; log_incident + save_note ✓ (⑨); minimal interim UI ✓; populated-DB verification ✓ (risk note).
- Placeholders: none — all SQL shapes, field lists, and guard patterns named against real files (read-first steps where the implementer must confirm).
- Type consistency: `incident`/`incident_followup`/`incident_service_link` naming consistent across tasks; category whitelist identical in Task 2 (service) and Task 3 (MCP doc).
