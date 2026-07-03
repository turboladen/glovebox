# 2hea — Feature reassessment: MCP-first glovebox

**Date:** 2026-07-02
**Epic:** `glovebox-2hea`
**Context:** Post-Phase-1-retrofit (PRs #16–#24: `glovebox-shared` / `glovebox-backend` / `glovebox-mcp`). First real MCP use (Claude Desktop importing NotePlan car notes) validated the surface and exposed the gaps this spec resolves.
**Purpose being served (Steve's words):** keep tabs on what's been done; know what's coming (plan, budget, don't over-drive); save memories/notes about everything; plan the very next service and coordinate future ones ("clutch in 6 months — schedule other maintenance around it").

---

## Decisions

| # | Decision | Resolution |
|---|----------|------------|
| ① | In-app AI | **Fully retired** — chat, suggestions, invoice-parse, research-generate, provider layer, 3 tables. Claude-over-`/mcp` is the AI interface. NHTSA recalls stay (not AI). New MCP verb `file_research_finding` replaces generate's persistence path. |
| ② | Parts | **Keep, simplified** — `part_slots` dropped (slot names preserved into a plain `location` text field on parts). New MCP verb `record_part`. |
| ③ | Taxonomy | **Observations + accidents unify into `incident`** (name chosen over "event", which is a cross-app pattern form word per the 0qiq discipline). Constrained categories; correspondence generalizes to followups on any incident; service links become M2M; `recurrence_of_id` chains. |
| ④ | Payer | `service_record.paid_by` (`self \| insurance \| third_party`, default `self`) + `payer_note`. Costs and build rollups split out-of-pocket vs covered. |
| ⑤ | Overdue | Resolvable three ways: **link** a real service record (MCP `record_service` gains `schedule_item_ids`), **minimal record** ("done before tracking"), or **dismiss** via the existing vehicle-level `enabled=false` override (surfaced in UI + a `dismiss_schedule_item` MCP verb). No new state. Plus: **plan it** (→ ⑦). |
| ⑥ | UI | Add Builds tab, global search box, Activity feed landing view. Chat tab dies; Observations+Accidents merge into Incidents tab. |
| ⑦ | Planning (new) | **`work_item` + `visit` primitives** — the "list of things I'm actually gonna do." Items sourced from recalls / overdue schedule items / incidents / builds / ad-hoc; visits group items with date + shop + cost rollup; completing a visit produces the linked service record, closing every loop. |
| ⑧ | Budget (new) | `est_cost_cents` on maintenance schedule items → derivable 12-month spend forecast (intervals × actual mileage rate + planned visits). |
| ⑨ | Notes (new) | `save_note(vehicle_id, note)` MCP verb — thin alias over `log_incident(category: note)`. The memories system is incidents + documents + FTS; this is its front door. |
| ⑩ | Warranty (new) | Nullable warranty expiry (date and/or mileage) on `vehicle` and `part`; `check_due_maintenance` flags "possibly covered." |
| — | Migrations | **Keep using migrations for now** (Steve's imported data is the test bed). Squash-and-restart before "shipping" — deferred, tracked as a bead. |

Considered and rejected (YAGNI): fuel/fill-up tracking, push notifications, tire/seasonal-swap tracking, multi-driver features, an issue/thread parent entity above incidents (recurrence chains suffice), a snooze/waive state for schedule items (minimal-record + override + plan cover the cases).

---

## Design by unit

Staging approach: **sequenced work units, retrofit-style** — one branch/PR per unit, each through the full relay (implementer → `/code-review high --fix` → independent reviewer → PR → CI → merge), `just ci` + e2e as gates. Order respects data-model dependencies (incidents before planning, since work items link to incidents):

```
A retire-AI  →  C parts-simplify  →  D payer  →  B incidents  →  E overdue  →  G planning  →  F ui
```

### Unit A — Retire the in-app AI

**Remove:** `ChatTab.svelte`, suggestions surface, invoice-parse flow (frontend); `api/ai.rs`, `api/conversations.rs` + routes; `services/ai_ops.rs`, `services/conversation.rs`, the whole `services/ai/` layer (providers, registry, context builder); entities + tables `ai_provider_config`, `conversation`, `chat_message` (migration exports chat history to a JSON file before dropping — cheap insurance); `AppState` loses `Arc<AiProviderRegistry>`; reqwest stays (NHTSA/vin).
**Keep:** document upload + `extracted_text` (feeds FTS/MCP); `research::check_recalls` and all recall-finding persistence; research reports/findings tables and tab.
**Add:** MCP verb `file_research_finding(vehicle_id, category, title, description?, source_url?, severity?)` → creates a finding under a per-vehicle report of `report_type: "external_research"` (created on first use, reused thereafter — mirrors how recall persistence anchors findings to a `recalls_only` report). This is how Claude persists research it does.
**Note:** `strip_code_fences` and any ai-module helpers still used elsewhere move to a neutral home before the module dies.

### Unit C — Parts simplification

Migration: add `part.location` (nullable text), backfill from the part's slot name, drop `part.slot_id` and the `part_slots` table; delete `services/part_slot.rs`, `inputs/part_slot.rs`, `api/part_slots.rs` + routes, slot UI in PartsTab.
MCP: new `record_part(vehicle_id, name, cost_cents?, status?, location?, installed_service_id?, build_id?, url?, notes?)` — ownership-guarded like siblings.
Build progress and costs dedupe are unaffected (they key on parts, not slots).

### Unit D — Payer tracking

Migration: `service_record.paid_by` TEXT NOT NULL DEFAULT 'self' + `payer_note` TEXT NULL (appended; entity fields go last per column-order rule). Whitelist validation in `service_record` service (`self|insurance|third_party`, mirroring status-whitelist patterns).
`costs::summary`: adds `out_of_pocket_cents` / `covered_cents` splits (overall + monthly); `build::progress` rollup gains the same split. MCP `record_service` input + `cost_summary` output updated; Costs UI shows the split. The $15k insurance repair and the neighbor's side-swipe become queryable facts.

### Unit B — Incident unification (the big one)

**Schema:** new `incidents` table = observation fields + nullable accident fields (`occurred_at` semantics preserved, other-party/insurance/claim fields) + `category` (constrained: `noise | leak | warning_light | damage | accident | note`) + `recurrence_of_id` (nullable self-FK) + `build_id`. New `incident_followups` (from `accident_correspondence`, generalized: date, medium/contact, summary, notes) and `incident_service_link` (M2M; subsumes both `accident_service_link` and `observation.resolved_service_id`).
**Migration:** move observations (category mapped/normalized; the miscategorized "accident" import row lands correctly) and accidents (category `accident`) into `incidents`; correspondence → followups; service links + `resolved_service_id` → `incident_service_link`; drop old tables. FTS migration: drop `fts_observations`/`fts_accidents`/`fts_accident_correspondence`, add `fts_incidents` + `fts_incident_followups`; `SearchScope::Observations|Accidents` → `Incidents`.
**Services:** `services/incident.rs` replaces observation+accident services (paxy ownership discipline: followups and links vehicle-scoped; recurrence link must reference a same-vehicle incident).
**MCP:** `log_incident` (category enum in schema; description steers accidents here and mentions recurrence linking) replaces `log_observation`; `save_note` alias (⑨) lands here; activity feed + resources vocabulary updated.
**UI:** Observations + Accidents tabs → one Incidents tab (category filter, followups, recurrence chain, linked services).

### Unit E — Overdue resolvability

MCP `record_service` gains `schedule_item_ids` (shared service already supports it); `check_due_maintenance` description instructs linking when recorded work satisfies an item. "Record minimal service" affordance (UI + MCP guidance) for done-but-unrecorded. `dismiss_schedule_item(vehicle_id, schedule_item_id, reason?)` MCP verb creates the vehicle-level `enabled=false` override (existing mechanism, now surfaced); Schedule UI shows and can toggle overrides. Folds in `glovebox-w6ws`: `mileage_log.service_record_id` FK migration + create/delete maintenance + activity/reminders keying on the FK (fixes the feed dedupe holes).

### Unit G — Planning + budget + warranty

**Schema:** `work_items` (vehicle FK; title; notes; nullable source FKs: `schedule_item_id`, `research_finding_id`, `incident_id`, `build_id`; `est_cost_cents`; `status: planned|scheduled|done|dropped`; nullable `visit_id`; timestamps) and `visits` (vehicle FK; `planned_date`; `shop_name`/`shop_id`; notes; `status: planned|scheduled|completed|canceled`; nullable `service_record_id` set on completion; timestamps). `maintenance_schedule_item.est_cost_cents` (⑧). Warranty fields (⑩): `vehicle.warranty_expires_on`/`warranty_expires_miles`, `part.warranty_expires_on`/`warranty_expires_miles` (all nullable).
**Completion flow:** `complete_visit(visit_id, actuals...)` creates the service record (payer-aware), wires `schedule_item_ids` from the visit's schedule-sourced items, links incident-sourced items via `incident_service_link`, marks items `done`, sets `visit.service_record_id`. Recalls close because their finding's work item is done; reminders clear via the schedule links.
**Budget forecast:** shared fn deriving ~12-month outlook from schedule intervals × the vehicle's actual mileage rate × `est_cost_cents`, plus planned visits' rollups; surfaced in `check_due_maintenance` output and Costs.
**MCP verbs:** `plan_work`, `list_planned_work`, `schedule_visit`, `complete_visit` (+ `check_recalls`/`check_due_maintenance` descriptions gain "offer to plan" guidance; warranty flag in `check_due_maintenance`).
**Folds in `glovebox-le5y`:** `reminders::calculate_reminders` → `DomainResult` (this unit rewires reminders anyway).

### Unit F — UI additions

Builds tab (list + progress detail), **Plan tab** (work items by status + visits with cost rollups + complete-visit flow), global search box (existing `GET /api/search`), Activity feed as the vehicle landing view (`activity::recent` over a new HTTP route). Final tab bar: **Activity · History · Schedule · Plan · Incidents · Parts · Builds · Costs · Documents · Research**. Playwright + TEST_PLAN.md updated per UI change (this unit and B, C, D where tabs change).

---

## Bead mapping

- `ls3p` (parts) → unit C · `pio3` (payer) → unit D · `csbb` (taxonomy) → unit B · `eb9q` (overdue) + `w6ws` (mileage FK) → unit E · `le5y` (reminders DomainResult) → unit G · `skde` (builds FTS + activity HTTP) → FTS-builds arm folds into B's FTS migration; activity-over-HTTP folds into F · `brm2` (MCP auth) stays deferred (non-LAN only).
- New beads at plan time: one per unit (A–G sequence chain), plus deferred **squash-migrations-before-ship** (P4).

## Risks & invariants

- **Every unit is migration-bearing except F** — migrations stay append-only for now (Steve's data is the test bed); the squash comes later, as its own decision.
- **B is the riskiest** (three-table merge + FTS rebuild): migration must be verified against a copy of the real dev DB, not just `test_db()`; e2e specs for observations/accidents tabs are rewritten with the Incidents tab.
- **A is a one-way door** (chat history deleted): the pre-drop JSON export is mandatory in the migration.
- Layering + paxy ownership discipline hold everywhere (`check-layering.sh` in CI already enforces the former; wrong-parent-indistinguishable tests required for every new link: work_item sources, visit↔items, incident links, followups).
- MCP tool count grows ~14 → ~22; tool descriptions must keep cross-referencing (plan_work ↔ check_due_maintenance ↔ complete_visit) so LLM navigation stays coherent.
- Behavioral gates per unit: `just ci` + `just test-e2e-ci` green; boot smoke; for A, grep-level verification that no `AiProvider`/`ai_ops` references survive.

## Non-goals

- No auth on `/mcp` (LAN posture unchanged; `brm2` covers non-LAN).
- No fuel tracking, notifications, tires, multi-driver (rejected above).
- No migration squash yet.
- No Phase-2 cross-app extraction — this is still glovebox-local; pattern learnings (incident model, plan primitive) feed back to `../personal-domain-pattern` as notes, not shared code.

## Provenance

Decisions made interactively with Steve, 2026-07-02, on the back of the first real MCP import (Claude Desktop + NotePlan notes). Supersedes the smoke-finding beads' individual scopes where this spec restructures them.
