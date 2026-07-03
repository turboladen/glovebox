# 2hea Unit A — Retire the In-App AI: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the in-app AI feature entirely (chat, suggestions, invoice-parse, research-generate, provider layer, 3 tables) and add the `file_research_finding` MCP verb so Claude-over-`/mcp` replaces it.

**Architecture:** Top-down removal that keeps every task compiling: detach the HTTP surface first, prune research's AI half, delete the shared AI modules, then drop entities + tables by migration. The one addition (MCP verb + shared helper) lands after the removals. Spec: `docs/superpowers/specs/2026-07-02-2hea-feature-reassessment-design.md` (Unit A).

**Tech Stack:** Rust workspace (glovebox-shared / glovebox-backend / glovebox-mcp), SeaORM migrations, rmcp, Svelte 5 SPA, Playwright.

## Global Constraints

- Bead: `glovebox-mxh1`. Branch: `2hea/unit-a-retire-ai` (already checked out). No push/PR from the implementer.
- **Every task ends green:** `cargo build --workspace` + `cargo test --workspace` at minimum; full `just ci` at Tasks 6–7. `just fmt` (nightly) before each commit.
- Tables `ai_provider_config`, `conversation`, `chat_message` are dropped **outright** — no export (never-shipped app, per Steve).
- KEEP: document upload + `extracted_text`; `research::check_recalls` + `persist_recall_findings` + all recall persistence; research reports/findings tables and their list/get/update fns; ResearchTab.
- None of the three dropped tables is FTS-indexed (verify: they are absent from `SPECS` in `m20260301_000013`) — no FTS work in this unit.
- `.beads/` never staged. Commits end with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: Detach the HTTP surface (routes, handlers, AppState)

**Files:**
- Modify: `glovebox-backend/src/main.rs` (imports ~line 25, `AppState` ~32, registry init ~69–75, routes ~91–124)
- Delete: `glovebox-backend/src/api/ai.rs`, `glovebox-backend/src/api/conversations.rs`
- Modify: `glovebox-backend/src/api/mod.rs` (drop the two `pub mod` lines)

**Interfaces:**
- Produces: `AppState { db, config }` (no `ai` field) — later tasks and the MCP mount rely on `state.db` only (MCP mount already does).

- [ ] **Step 1: Remove routes + state from main.rs**

In `glovebox-backend/src/main.rs`: change the shared import to `use glovebox_shared::{config::AppConfig, migration::Migrator};` (drop `services::ai::registry::AiProviderRegistry`); delete the `ai: Arc<AiProviderRegistry>` field from `AppState`; delete the `let ai = Arc::new(AiProviderRegistry::new(db.clone()));` block and the `has_provider` logging lines; remove `ai` from the `AppState` construction; delete every route line referencing `api::ai::*` (status, parse-invoice, chat, models, chat/history, providers, providers/{id}, suggestions) and `api::conversations::*` (the three conversation routes). Keep `/api/health`, all vehicle sub-resources, `/api/search`, the `/mcp` mount, `/files`, and the SPA fallback untouched.

- [ ] **Step 2: Delete the handlers**

```bash
git rm glovebox-backend/src/api/ai.rs glovebox-backend/src/api/conversations.rs
```

Remove `pub mod ai;` and `pub mod conversations;` from `glovebox-backend/src/api/mod.rs`.

- [ ] **Step 3: Build + test (shared still holds unused ai modules — expected)**

Run: `cargo build --workspace && cargo test --workspace`
Expected: PASS. If clippy-pedantic later flags now-dead shared code, that code is deleted in Tasks 2–3 — do not silence it here.

- [ ] **Step 4: Commit**

```bash
git add -A glovebox-backend
git commit -m "refactor(2hea-a): detach AI/conversation HTTP surface and AppState registry"
```

### Task 2: Prune research's AI half

**Files:**
- Modify: `glovebox-shared/src/services/research.rs`

**Interfaces:**
- Produces: `research.rs` with NO `services::ai` imports; keeps `check_recalls`, `persist_recall_findings`, `NewFinding` (used by recall persistence), `list_reports`, `get_report`, `list_findings`, `update_finding`, `ReportWithFindings`.

- [ ] **Step 1: Verify the dead-code boundary**

```bash
grep -rn "parse_ai_findings\|build_community_wisdom_prompt" glovebox-shared glovebox-backend glovebox-mcp --include="*.rs" | grep -v "research.rs"
```
Expected: no output (both are research-internal, only called by `generate`).

- [ ] **Step 2: Remove `generate` and its helpers**

Delete from `glovebox-shared/src/services/research.rs`: `pub async fn generate` (~line 188), `fn build_community_wisdom_prompt` (~429), `fn parse_ai_findings` (~475), and their tests (`build_prompt_*`, `parse_ai_findings_plain_json`, `parse_ai_findings_code_fenced`, `parse_ai_findings_invalid_json`, `generate_persists_report_and_get_reads_it_back` — the last one tests `generate`; keep `update_finding_*`, `list_reports_missing_vehicle_is_not_found`, and recall/report tests that don't call `generate`). Fix the `use crate::{ ... services::{ ai::{...}, nhtsa::{...} } }` import block to keep only `nhtsa`. If a kept test relied on `generate` for setup, replace that setup with direct `research_report::ActiveModel` inserts (the pattern already used by `update_finding_validates_status_and_persists`).

- [ ] **Step 3: Build + test**

Run: `cargo test -p glovebox-shared research`
Expected: PASS (recall/report/finding tests remain; no `ai::` references).

- [ ] **Step 4: Commit**

```bash
git add glovebox-shared/src/services/research.rs
git commit -m "refactor(2hea-a): remove AI report generation from research (recalls stay)"
```

### Task 3: Delete the shared AI modules

**Files:**
- Delete: `glovebox-shared/src/services/ai_ops.rs`, `glovebox-shared/src/services/conversation.rs`, `glovebox-shared/src/services/ai/` (whole dir: `mod.rs`, `claude.rs`, `openai_compat.rs`, `noop.rs`, `registry.rs`, `context.rs`, and any `mock`), `glovebox-shared/src/inputs/ai_provider.rs`
- Modify: `glovebox-shared/src/services/mod.rs`, `glovebox-shared/src/inputs/mod.rs`

- [ ] **Step 1: Verify nothing else consumes them**

```bash
grep -rn "ai_ops\|services::ai\b\|services::ai::\|inputs::ai_provider\|conversation::" glovebox-shared/src glovebox-backend/src glovebox-mcp/src --include="*.rs" | grep -vE "services/ai/|ai_ops\.rs|conversation\.rs"
```
Expected: only the `mod.rs` declaration lines you are about to remove. (`strip_code_fences` lives in `services/ai/mod.rs` and its remaining consumers died in Tasks 1–2 — it goes down with the module.)

- [ ] **Step 2: Delete + deregister**

```bash
git rm -r glovebox-shared/src/services/ai glovebox-shared/src/services/ai_ops.rs glovebox-shared/src/services/conversation.rs glovebox-shared/src/inputs/ai_provider.rs
```
Remove `pub mod ai;`, `pub mod ai_ops;`, `pub mod conversation;` from `services/mod.rs` and `pub mod ai_provider;` from `inputs/mod.rs`.

- [ ] **Step 3: Build + test the workspace**

Run: `cargo build --workspace && cargo test --workspace`
Expected: PASS. `activity.rs` does not touch conversations (verify with the build). Test counts drop (the moved-in ai/conversation tests die with their modules) — record the new counts.

- [ ] **Step 4: Commit**

```bash
git add -A glovebox-shared
git commit -m "refactor(2hea-a): delete ai_ops/conversation services and the AI provider layer"
```

### Task 4: Drop the tables + entities (migration 000015)

**Files:**
- Create: `glovebox-shared/src/migration/m20260301_000015_drop_ai_tables.rs`
- Modify: `glovebox-shared/src/migration/mod.rs` (register)
- Delete: `glovebox-shared/src/entities/ai_provider_config.rs`, `glovebox-shared/src/entities/conversation.rs`, `glovebox-shared/src/entities/chat_message.rs`
- Modify: `glovebox-shared/src/entities/mod.rs`; `glovebox-shared/src/entities/vehicle.rs` if it declares a conversation relation (grep first)

- [ ] **Step 1: Write the migration**

```rust
//! Drops the in-app AI tables (2hea unit A). The app never shipped; the data
//! is explicitly not worth keeping (chat history, provider configs).

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        // Children before parents (chat_message FKs conversation).
        db.execute_unprepared("DROP TABLE IF EXISTS chat_messages")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS conversations")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS ai_provider_configs")
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Intentionally irreversible: the feature is retired, not versioned.
        Err(DbErr::Migration(
            "2hea unit A drops the AI tables permanently; restore from a DB backup instead".into(),
        ))
    }
}
```

**First verify the physical table names** (`chat_messages` vs `chat_message` etc.) against the entity files' `#[sea_orm(table_name = "...")]` attributes BEFORE deleting them, and adjust the SQL to match exactly.

- [ ] **Step 2: Register + delete entities**

Add `mod m20260301_000015_drop_ai_tables;` + its `Box::new(...)` entry (appended last) in `migration/mod.rs`. Then:

```bash
grep -n "conversation\|chat_message\|ai_provider" glovebox-shared/src/entities/vehicle.rs glovebox-shared/src/entities/mod.rs
git rm glovebox-shared/src/entities/ai_provider_config.rs glovebox-shared/src/entities/conversation.rs glovebox-shared/src/entities/chat_message.rs
```
Remove their `pub mod` lines from `entities/mod.rs` and any `Relation`/`Related` impls the vehicle grep found.

- [ ] **Step 3: Test (test_db runs all migrations, proving 000015 applies)**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add -A glovebox-shared
git commit -m "feat(2hea-a): migration 000015 drops AI tables; remove their entities"
```

### Task 5: `file_research_finding` MCP verb

**Files:**
- Modify: `glovebox-shared/src/services/research.rs` (new fn + tests)
- Modify: `glovebox-mcp/src/handler.rs` (+ `schemas.rs` if inputs live there — follow the existing per-tool input-struct pattern)
- Modify: `glovebox-mcp/tests/mcp_integration_test.rs`

**Interfaces:**
- Produces: `research::file_finding(db, vehicle_id, input: NewFiledFinding) -> DomainResult<research_finding::Model>` and MCP tool `file_research_finding`.

- [ ] **Step 1: Write the failing shared test** (in `research.rs`'s test module)

```rust
#[tokio::test]
async fn file_finding_creates_and_reuses_external_research_report() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let f1 = file_finding(
        &db,
        vid,
        NewFiledFinding {
            category: "maintenance".into(),
            title: "DSG service interval is 40k, not 60k".into(),
            description: Some("Per community consensus for this gearbox".into()),
            source_url: Some("https://example.com/thread".into()),
            severity: Some("info".into()),
        },
    )
    .await
    .unwrap();
    let f2 = file_finding(
        &db,
        vid,
        NewFiledFinding {
            category: "recall".into(),
            title: "Second finding".into(),
            description: None,
            source_url: None,
            severity: None,
        },
    )
    .await
    .unwrap();
    // Same anchor report, created once, type external_research.
    assert_eq!(f1.report_id, f2.report_id);
    let report = get_report(&db, vid, f1.report_id).await.unwrap();
    assert_eq!(report.report.report_type.as_deref(), Some("external_research"));
    assert_eq!(report.findings.len(), 2);
    // Missing vehicle is indistinguishable from nonexistent.
    assert!(matches!(
        file_finding(&db, 999, NewFiledFinding {
            category: "note".into(), title: "x".into(),
            description: None, source_url: None, severity: None,
        })
        .await
        .unwrap_err(),
        DomainError::NotFound(_)
    ));
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p glovebox-shared file_finding` → FAIL (fn not defined).

- [ ] **Step 3: Implement**

In `research.rs` (input struct near the top with the other types; fn after `update_finding`):

```rust
#[derive(Debug)]
pub struct NewFiledFinding {
    pub category: String,
    pub title: String,
    pub description: Option<String>,
    pub source_url: Option<String>,
    pub severity: Option<String>,
}

/// Persist an externally-researched finding (e.g. filed by an MCP client)
/// under this vehicle's `external_research` anchor report — created on first
/// use, reused thereafter (mirrors how recalls anchor to `recalls_only`).
pub async fn file_finding(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    input: NewFiledFinding,
) -> DomainResult<research_finding::Model> {
    vehicle::Entity::find_by_id(vehicle_id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound("Vehicle not found".to_string()))?;

    let report = match research_report::Entity::find()
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .filter(research_report::Column::ReportType.eq("external_research"))
        .one(db)
        .await?
    {
        Some(r) => r,
        None => {
            research_report::ActiveModel {
                vehicle_id: Set(vehicle_id),
                report_type: Set(Some("external_research".to_string())),
                summary: Set(Some("Findings filed via MCP".to_string())),
                ..Default::default()
            }
            .insert(db)
            .await?
        }
    };

    Ok(research_finding::ActiveModel {
        report_id: Set(report.id),
        category: Set(input.category),
        title: Set(input.title),
        description: Set(input.description),
        source_url: Set(input.source_url),
        severity: Set(input.severity),
        status: Set("new".to_string()),
        ..Default::default()
    }
    .insert(db)
    .await?)
}
```

(Adjust field names against the actual `research_report`/`research_finding` entities — e.g. `report_type`/`summary` optionality — before compiling.)

- [ ] **Step 4: Run to verify pass** — `cargo test -p glovebox-shared file_finding` → PASS.

- [ ] **Step 5: Add the MCP tool**

Follow the existing `#[tool]` pattern in `glovebox-mcp/src/handler.rs` exactly (input struct with schemars doc-comments, `input_schema = schema_for_type::<...>()` override, `LenientParameters`, errors via the shared `domain_result` helper):

```rust
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct FileResearchFindingInput {
    /// Vehicle id, from `list_vehicles`.
    pub vehicle_id: i32,
    /// Finding kind, e.g. "maintenance", "recall", "upgrade", "issue", "note".
    pub category: String,
    /// Short title, e.g. "DSG service interval is 40k, not 60k".
    pub title: String,
    /// Longer description of what was found.
    pub description: Option<String>,
    /// Where this was found (URL).
    pub source_url: Option<String>,
    /// "info", "low", "medium", "high", or "critical".
    pub severity: Option<String>,
}
```

Tool description (LLM-facing): "Save research you've done about this vehicle (forum consensus, TSBs, known issues, upgrade notes) as a persistent finding. Use after answering research questions so the knowledge isn't lost when the conversation ends. Findings appear in the app's Research view."

- [ ] **Step 6: Integration test**

In `glovebox-mcp/tests/mcp_integration_test.rs`: bump the `tools/list` count assertion 14 → 15 and add:

```rust
#[tokio::test]
async fn file_research_finding_persists_and_is_readable() {
    let (app, db) = setup().await;
    let session = handshake(&app).await;
    let vid = seed_vehicle(&db).await; // reuse the existing seed helper
    let body = post_rpc(
        &app,
        &session,
        call_tool(
            "file_research_finding",
            serde_json::json!({
                "vehicle_id": vid,
                "category": "maintenance",
                "title": "DSG interval 40k",
                "source_url": "https://example.com/t"
            }),
        ),
    )
    .await;
    assert_success(&body);
    assert!(body.contains("DSG interval 40k"));
    // Wrong vehicle -> clean tool error
    let err = post_rpc(
        &app,
        &session,
        call_tool(
            "file_research_finding",
            serde_json::json!({"vehicle_id": 999, "category": "x", "title": "y"}),
        ),
    )
    .await;
    assert_tool_error(&err);
}
```

- [ ] **Step 7: Run** — `cargo test -p glovebox-mcp` → PASS (unit + integration incl. the new one). Then commit:

```bash
git add glovebox-shared/src/services/research.rs glovebox-mcp
git commit -m "feat(2hea-a): file_research_finding MCP verb (external_research anchor report)"
```

### Task 6: Frontend removal

**Files:**
- Delete: `frontend/src/components/ChatTab.svelte`, `frontend/src/components/SuggestionsCard.svelte`, `frontend/e2e/chat.spec.ts`, `frontend/e2e/suggestions.spec.ts`, `frontend/e2e/invoice-parse.spec.ts`
- Modify: `frontend/src/components/VehicleDetail.svelte` (chat tab wiring), `frontend/src/components/ScheduleTab.svelte` (SuggestionsCard usage), `frontend/src/components/DocumentsTab.svelte` (parse-invoice action), `frontend/src/components/Settings.svelte` + `frontend/src/components/Garage.svelte` (AI-provider settings surface — read them first; if Settings is ONLY provider config, delete it and its Garage entry point), `frontend/src/lib/api.ts` + `frontend/src/lib/types.ts` (aiApi/conversation/provider/suggestion types + fns), `TEST_PLAN.md`

- [ ] **Step 1: Read each modify-listed component and excise the AI surface** — remove the Chat tab entry + component import, the SuggestionsCard render + import, the "Parse invoice" button/flow in DocumentsTab (upload + extracted-text display STAY), the provider-settings UI, and every `aiApi`/conversation/provider function + type in `api.ts`/`types.ts`. Grep to prove it clean:

```bash
grep -rn "aiApi\|conversation\|parse-invoice\|parseInvoice\|suggestion\|provider" frontend/src --include="*.svelte" --include="*.ts" | grep -vi "search" 
```
Expected: no AI-feature hits (unrelated matches like CSS words reviewed by eye).

- [ ] **Step 2: Delete files**

```bash
git rm frontend/src/components/ChatTab.svelte frontend/src/components/SuggestionsCard.svelte \
       frontend/e2e/chat.spec.ts frontend/e2e/suggestions.spec.ts frontend/e2e/invoice-parse.spec.ts
```

- [ ] **Step 3: Update TEST_PLAN.md** — remove the chat/suggestions/invoice-parse sections; note the MCP replacement (one line each).

- [ ] **Step 4: Frontend gates**

Run: `cd frontend && bun run check && bun run build`
Expected: 0 errors. Then `just test-e2e-ci` → expected count = 53 − (chat + suggestions + invoice-parse spec tests); record the actual number, all passing.

- [ ] **Step 5: Commit**

```bash
git add -A frontend TEST_PLAN.md
git commit -m "refactor(2hea-a): remove chat/suggestions/invoice-parse UI and their e2e specs"
```

### Task 7: Docs + final verification

**Files:**
- Modify: `CLAUDE.md` (Architecture: remove the AI-layer paragraph + `Arc<AiProviderRegistry>` mention from AppState; Commands/tabs lists if they name Chat/Suggestions), `docs/superpowers/specs/2026-06-30-mde0-phase1-retrofit-sequencing.md` is historical — do NOT edit it.

- [ ] **Step 1: Update CLAUDE.md** to match reality (AppState = db + config; no AI layer; MCP tool count 15).

- [ ] **Step 2: Zero-residue grep**

```bash
grep -rni "AiProvider\|ai_ops\|chat_message\|ai_provider_config\|parse_invoice\|invoice-parse" \
  glovebox-shared/src glovebox-backend/src glovebox-mcp/src frontend/src CLAUDE.md --include="*.rs" --include="*.svelte" --include="*.ts" || echo CLEAN
```
Expected: `CLEAN` (or only historical doc/spec paths, which are exempt).

- [ ] **Step 3: Full gates**

Run: `just ci` → exit 0 (fmt, layering, build/test/clippy-pedantic, frontend). `just test-e2e-ci` → all remaining specs pass. Boot smoke: `cargo run -p glovebox-backend` bg → `/api/health` 200; `/api/ai/status` → **404**; `/mcp` initialize → 200 and `tools/list` shows 15 tools; kill. Record real outputs.

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md
git commit -m "docs(2hea-a): CLAUDE.md reflects retired AI layer"
```

## Self-Review

- **Spec coverage:** every Unit-A bullet mapped — remove list (T1/T3/T4/T6), keep list (constraints + T2), `file_research_finding` with `external_research` anchor (T5), helper relocation resolved as delete-with-module (inventory: no surviving consumers), tables dropped without export (T4), CLAUDE.md (T7). No gaps.
- **Placeholder scan:** none; removal steps carry exact paths + verify-greps; additions carry full code (with explicit adjust-against-entity notes where field optionality must be checked in-repo).
- **Type consistency:** `NewFiledFinding`/`file_finding` names match across T5 shared test, impl, and MCP tool; `AppState { db, config }` consistent T1→T7.
