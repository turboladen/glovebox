//! Full-text search across vehicle, event, document, and planning text — one
//! domain operation over the FTS5 indexes created by migrations 000013 and
//! 000021.
//!
//! Each FTS5 table is external-content (index only), so every subquery joins
//! back to its content table to produce domain-shaped hits: `vehicle_id` is
//! derived through parents where needed (line item → service record,
//! followup → incident, finding → report, schedule item → owning
//! platform/template/vehicle), and line-item hits fold into their parent
//! service record so callers think in domain records, not rows.

use sea_orm::*;

use crate::error::{DomainError, DomainResult};

/// Overall result cap across all scopes.
const LIMIT: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchScope {
    All,
    Vehicles,
    Services,
    Incidents,
    Builds,
    Documents,
    Research,
    Maintenance,
    Todo,
}

impl SearchScope {
    /// Parse the wire form
    /// (`all|vehicles|services|incidents|builds|documents|research|maintenance|todo`).
    pub fn parse(s: &str) -> DomainResult<Self> {
        match s {
            "all" => Ok(Self::All),
            "vehicles" => Ok(Self::Vehicles),
            "services" => Ok(Self::Services),
            "incidents" => Ok(Self::Incidents),
            "builds" => Ok(Self::Builds),
            "documents" => Ok(Self::Documents),
            "research" => Ok(Self::Research),
            "maintenance" => Ok(Self::Maintenance),
            "todo" => Ok(Self::Todo),
            other => Err(DomainError::BadRequest(format!(
                "unknown search scope '{other}' (expected one of: all, vehicles, services, \
                 incidents, builds, documents, research, maintenance, todo)"
            ))),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchHit {
    /// Domain record kind: `vehicle`, `service`, `incident`,
    /// `incident_followup`, `build`, `document`, `research_finding`,
    /// `schedule_item`, or `work_item`.
    pub kind: String,
    pub id: i32,
    pub vehicle_id: Option<i32>,
    pub title: String,
    pub snippet: String,
    /// bm25 relevance — lower (more negative) is a better match. Scores are
    /// per-FTS-table statistics, so ranks are only strictly comparable within a
    /// `kind`; cross-kind ordering in `all`-scope results is heuristic.
    pub rank: f64,
}

/// Sanitize free text into an FTS5 MATCH expression.
///
/// Each whitespace token becomes a double-quoted phrase (internal `"` doubled),
/// joined by spaces (implicit AND) — so user text like `brake "pad` or bare
/// `AND`/`NOT` can never be parsed as FTS5 syntax. Tokens with no alphanumeric
/// content (pure punctuation like `(`) produce no searchable terms and are
/// dropped; `None` means nothing searchable remains.
fn fts_match_expr(query: &str) -> Option<String> {
    let tokens: Vec<String> = query
        .split_whitespace()
        .filter(|t| t.chars().any(char::is_alphanumeric))
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" "))
    }
}

/// One UNION ALL arm: a `kind`-tagged SELECT over one FTS table joined back to
/// its content table. `?1` is the MATCH expression; when `vehicle_filter` is
/// set, `?2` is the vehicle id compared against `vehicle_col` (documents with
/// NULL `vehicle_id` never match, by design).
fn subquery(
    kind: &str,
    fts: &str,
    joins: &str,
    id_col: &str,
    vehicle_col: &str,
    title_expr: &str,
    vehicle_filter: bool,
) -> String {
    let filter = if vehicle_filter {
        format!(" AND {vehicle_col} = ?2")
    } else {
        String::new()
    };
    format!(
        "SELECT '{kind}' AS kind, {id_col} AS id, {vehicle_col} AS vehicle_id, {title_expr} AS \
         title, COALESCE(snippet({fts}, -1, '[', ']', '\u{2026}', 16), '') AS snippet, \
         bm25({fts}) AS rank FROM {fts} {joins} WHERE {fts} MATCH ?1{filter}"
    )
}

/// Ranked full-text search across the domain.
///
/// `vehicle_id: Some(v)` verifies the vehicle exists (`NotFound` otherwise) and
/// restricts hits to it. Empty/whitespace-only queries are a `BadRequest`;
/// queries with no searchable terms (pure punctuation) return no hits.
#[allow(clippy::too_many_lines)]
pub async fn search(
    db: &impl ConnectionTrait,
    query: &str,
    scope: SearchScope,
    vehicle_id: Option<i32>,
) -> DomainResult<Vec<SearchHit>> {
    if query.trim().is_empty() {
        return Err(DomainError::BadRequest("query must not be empty".into()));
    }
    if let Some(v) = vehicle_id {
        super::vehicle::require(db, v).await?;
    }
    let Some(match_expr) = fts_match_expr(query) else {
        return Ok(Vec::new());
    };

    let want = |s: SearchScope| scope == SearchScope::All || scope == s;
    let filt = vehicle_id.is_some();
    let mut subs: Vec<String> = Vec::new();

    if want(SearchScope::Vehicles) {
        subs.push(subquery(
            "vehicle",
            "fts_vehicles",
            "JOIN vehicles v ON v.id = fts_vehicles.rowid",
            "v.id",
            "v.id",
            "v.name",
            filt,
        ));
    }
    if want(SearchScope::Services) {
        let service_title = "COALESCE(s.description, 'Service on ' || s.service_date)";
        subs.push(subquery(
            "service",
            "fts_service_records",
            "JOIN service_records s ON s.id = fts_service_records.rowid",
            "s.id",
            "s.vehicle_id",
            service_title,
            filt,
        ));
        // Line-item hits fold into their parent service record.
        subs.push(subquery(
            "service",
            "fts_service_record_line_items",
            "JOIN service_record_line_items li ON li.id = fts_service_record_line_items.rowid \
             JOIN service_records s ON s.id = li.service_record_id",
            "s.id",
            "s.vehicle_id",
            service_title,
            filt,
        ));
    }
    if want(SearchScope::Incidents) {
        subs.push(subquery(
            "incident",
            "fts_incidents",
            "JOIN incidents i ON i.id = fts_incidents.rowid",
            "i.id",
            "i.vehicle_id",
            "i.title",
            filt,
        ));
        subs.push(subquery(
            "incident_followup",
            "fts_incident_followups",
            "JOIN incident_followups f ON f.id = fts_incident_followups.rowid JOIN incidents i ON \
             i.id = f.incident_id",
            "f.id",
            "i.vehicle_id",
            "f.summary",
            filt,
        ));
    }
    if want(SearchScope::Builds) {
        subs.push(subquery(
            "build",
            "fts_builds",
            "JOIN builds b ON b.id = fts_builds.rowid",
            "b.id",
            "b.vehicle_id",
            "b.name",
            filt,
        ));
    }
    if want(SearchScope::Documents) {
        subs.push(subquery(
            "document",
            "fts_documents",
            "JOIN documents d ON d.id = fts_documents.rowid",
            "d.id",
            "d.vehicle_id",
            "d.title",
            filt,
        ));
    }
    if want(SearchScope::Research) {
        subs.push(subquery(
            "research_finding",
            "fts_research_findings",
            "JOIN research_findings rf ON rf.id = fts_research_findings.rowid JOIN \
             research_reports rr ON rr.id = rf.report_id",
            "rf.id",
            "rr.vehicle_id",
            "rf.title",
            filt,
        ));
    }
    if want(SearchScope::Maintenance) {
        // A schedule item is owned by exactly one of platform / model template /
        // vehicle, and applies to every vehicle that resolves it (vehicle →
        // template → platform), so the join fans out to one hit per applicable
        // vehicle — each is a distinct navigable destination (Plan → Due).
        // Override/dismissal resolution is deliberately NOT replicated here: an
        // overridden or disabled item is still discoverable (it lives in
        // Schedule ⚙'s Dismissed section), and its `?hl` on Due silently no-ops
        // per the deep-link convention. Items whose platform/template has no
        // vehicles have no destination and drop out via the inner join.
        subs.push(subquery(
            "schedule_item",
            "fts_maintenance_schedule_items",
            "JOIN maintenance_schedule_items msi ON msi.id = fts_maintenance_schedule_items.rowid \
             JOIN vehicles v ON v.id = msi.vehicle_id OR (msi.model_template_id IS NOT NULL AND \
             v.model_template_id = msi.model_template_id) OR (msi.platform_id IS NOT NULL AND \
             msi.platform_id = (SELECT mt.platform_id FROM model_templates mt WHERE mt.id = \
             v.model_template_id))",
            "msi.id",
            "v.id",
            "msi.name",
            filt,
        ));
    }
    if want(SearchScope::Todo) {
        subs.push(subquery(
            "work_item",
            "fts_work_items",
            "JOIN work_items wi ON wi.id = fts_work_items.rowid",
            "wi.id",
            "wi.vehicle_id",
            "wi.title",
            filt,
        ));
    }

    // Dedupe by (kind, id, vehicle_id) IN SQL, before the LIMIT: a service
    // matching in its own text and in several line items is one domain record,
    // and duplicate raw rows must not starve other matching records out of the
    // capped result set. vehicle_id joins the key for the schedule-item arm,
    // where one inherited item legitimately fans out to one hit per applicable
    // vehicle (every other kind has exactly one vehicle_id per (kind, id), so
    // the extra key is a no-op for them). SQLite's bare-columns-with-MIN
    // semantics make title/snippet come from the best-ranked row of each group;
    // kind/id/vehicle_id tiebreakers keep equal-rank ordering deterministic.
    // The CTE must be MATERIALIZED: if SQLite flattens the arms into the
    // aggregate query, bm25() lands outside its FTS cursor and fails with
    // "unable to use function bm25 in the requested context".
    let sql = format!(
        "WITH raw AS MATERIALIZED ({}) SELECT kind, id, vehicle_id, title, snippet, MIN(rank) AS \
         rank FROM raw GROUP BY kind, id, vehicle_id ORDER BY rank ASC, kind ASC, id ASC, \
         vehicle_id ASC LIMIT {LIMIT}",
        subs.join(" UNION ALL ")
    );
    let values: Vec<Value> = match vehicle_id {
        Some(v) => vec![match_expr.into(), v.into()],
        None => vec![match_expr.into()],
    };
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            &sql,
            values,
        ))
        .await?;

    let mut hits = Vec::new();
    for row in rows {
        hits.push(SearchHit {
            kind: row.try_get("", "kind")?,
            id: row.try_get("", "id")?,
            vehicle_id: row.try_get("", "vehicle_id")?,
            title: row.try_get("", "title")?,
            snippet: row.try_get("", "snippet")?,
            rank: row.try_get("", "rank")?,
        });
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{entities, test_support::test_db};

    async fn seed_vehicle(db: &impl ConnectionTrait, name: &str) -> i32 {
        entities::vehicle::ActiveModel {
            name: Set(name.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn seed_service(db: &impl ConnectionTrait, vehicle_id: i32, notes: &str) -> i32 {
        entities::service_record::ActiveModel {
            vehicle_id: Set(vehicle_id),
            service_date: Set("2026-01-15".into()),
            description: Set(Some("Front brake job".into())),
            notes: Set(Some(notes.into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn seed_document(db: &impl ConnectionTrait, vehicle_id: Option<i32>, text: &str) -> i32 {
        entities::document::ActiveModel {
            vehicle_id: Set(vehicle_id),
            title: Set("Invoice".into()),
            file_path: Set("docs/invoice.pdf".into()),
            file_name: Set("invoice.pdf".into()),
            extracted_text: Set(Some(text.into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn finds_service_notes_across_all_scopes() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, "Golf R").await;
        let sid = seed_service(&db, vid, "replaced brake pads and rotors").await;

        let hits = search(&db, "brake pads", SearchScope::All, None)
            .await
            .unwrap();
        assert!(!hits.is_empty());
        let hit = hits.iter().find(|h| h.kind == "service").unwrap();
        assert_eq!(hit.id, sid);
        assert_eq!(hit.vehicle_id, Some(vid));
        assert_eq!(hit.title, "Front brake job");
        assert!(hit.snippet.contains("brake"));
    }

    #[tokio::test]
    async fn results_are_rank_ordered() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, "Golf R").await;
        seed_service(&db, vid, "replaced brake pads").await;
        seed_service(
            &db,
            vid,
            "brake brake brake pads pads brake fluid flush brake",
        )
        .await;
        entities::incident::ActiveModel {
            vehicle_id: Set(vid),
            category: Set("noise".into()),
            title: Set("Brake squeal".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let hits = search(&db, "brake", SearchScope::All, None).await.unwrap();
        assert!(hits.len() >= 3);
        assert!(
            hits.windows(2).all(|w| w[0].rank <= w[1].rank),
            "hits must be ordered by ascending bm25 rank: {hits:?}"
        );
    }

    #[tokio::test]
    async fn vehicle_filter_excludes_other_vehicles() {
        let db = test_db().await;
        let mine = seed_vehicle(&db, "Golf R").await;
        let other = seed_vehicle(&db, "Jetta").await;
        let mine_svc = seed_service(&db, mine, "replaced brake pads").await;
        seed_service(&db, other, "replaced brake pads too").await;
        // A document with no vehicle must never match a vehicle-scoped search.
        seed_document(&db, None, "brake pad receipt").await;

        let hits = search(&db, "brake", SearchScope::All, Some(mine))
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.vehicle_id == Some(mine)));
        assert!(hits.iter().any(|h| h.kind == "service" && h.id == mine_svc));
        assert!(!hits.iter().any(|h| h.kind == "document"));
    }

    #[tokio::test]
    async fn scope_filter_restricts_kinds() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, "Golf R").await;
        seed_service(&db, vid, "replaced brake pads").await;
        let doc = seed_document(&db, Some(vid), "brake pad receipt from FCP").await;

        let hits = search(&db, "brake", SearchScope::Documents, None)
            .await
            .unwrap();
        assert!(hits.iter().all(|h| h.kind == "document"));
        assert!(hits.iter().any(|h| h.id == doc));

        let hits = search(&db, "brake", SearchScope::Services, None)
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.kind == "service"));
    }

    #[tokio::test]
    async fn update_trigger_keeps_index_in_sync() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, "Golf R").await;
        let sid = seed_service(&db, vid, "flushed coolant").await;

        let mut active: entities::service_record::ActiveModel =
            entities::service_record::Entity::find_by_id(sid)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .into();
        active.notes = Set(Some("replaced serpentine belt".into()));
        active.update(&db).await.unwrap();

        let new_hits = search(&db, "serpentine", SearchScope::All, None)
            .await
            .unwrap();
        assert!(new_hits.iter().any(|h| h.kind == "service" && h.id == sid));
        let old_hits = search(&db, "coolant", SearchScope::All, None)
            .await
            .unwrap();
        assert!(!old_hits.iter().any(|h| h.kind == "service" && h.id == sid));
    }

    #[tokio::test]
    async fn delete_removes_from_index() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, "Golf R").await;
        let sid = seed_service(&db, vid, "flushed coolant").await;
        entities::service_record::Entity::delete_by_id(sid)
            .exec(&db)
            .await
            .unwrap();
        let hits = search(&db, "coolant", SearchScope::All, None)
            .await
            .unwrap();
        assert!(hits.is_empty());
    }

    #[tokio::test]
    async fn line_item_hits_fold_into_parent_service() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, "Golf R").await;
        // Parent notes also match "brake" so the fold must dedupe to one hit.
        let sid = seed_service(&db, vid, "brake service").await;
        entities::service_record_line_item::ActiveModel {
            service_record_id: Set(sid),
            description: Set("OEM brake pad set".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        entities::service_record_line_item::ActiveModel {
            service_record_id: Set(sid),
            description: Set("brake rotor pair".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let hits = search(&db, "brake", SearchScope::Services, None)
            .await
            .unwrap();
        let service_hits: Vec<_> = hits
            .iter()
            .filter(|h| h.kind == "service" && h.id == sid)
            .collect();
        assert_eq!(service_hits.len(), 1, "folded hits must dedupe: {hits:?}");
    }

    #[tokio::test]
    async fn duplicate_raw_rows_cannot_starve_other_records_out_of_the_limit() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, "Golf R").await;
        // Service A: more matching line items than the whole result cap.
        let sid_a = seed_service(&db, vid, "brake overhaul").await;
        for i in 0..55 {
            entities::service_record_line_item::ActiveModel {
                service_record_id: Set(sid_a),
                description: Set(format!("brake part {i}")),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        // Service B: a single weak match — must still appear (dedupe happens
        // before the LIMIT, so A's 56 raw rows are one hit, not the whole cap).
        let sid_b = seed_service(&db, vid, "also touched the brake line").await;

        let hits = search(&db, "brake", SearchScope::Services, None)
            .await
            .unwrap();
        assert_eq!(hits.len(), 2, "expected exactly two folded hits: {hits:?}");
        assert!(hits.iter().any(|h| h.id == sid_a));
        assert!(hits.iter().any(|h| h.id == sid_b));
    }

    #[tokio::test]
    async fn finds_vehicle_incident_followup_build_and_research() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, "Golf R with unicorn paint").await;
        entities::incident::ActiveModel {
            vehicle_id: Set(vid),
            category: Set("noise".into()),
            title: Set("Turbo whistle".into()),
            description: Set(Some("whistle under boost".into())),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let crash = entities::incident::ActiveModel {
            vehicle_id: Set(vid),
            category: Set("accident".into()),
            title: Set("rear-ended at a stoplight".into()),
            description: Set(Some("rear-ended at a stoplight".into())),
            occurred_at: Set("2026-02-01".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        entities::incident_followup::ActiveModel {
            incident_id: Set(crash.id),
            occurred_at: Set("2026-02-02".into()),
            summary: Set("Adjuster approved bumper repair".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        entities::build::ActiveModel {
            vehicle_id: Set(vid),
            name: Set("Big turbo kilonewton build".into()),
            description: Set(Some("IS38 swap with methanol injection".into())),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let report = entities::research_report::ActiveModel {
            vehicle_id: Set(vid),
            generated_at: Set("2026-02-03 00:00:00".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        entities::research_finding::ActiveModel {
            report_id: Set(report.id),
            category: Set("common_issue".into()),
            title: Set("Water pump failure".into()),
            description: Set(Some("plastic impeller cracks".into())),
            status: Set("open".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let assert_kind = |hits: &[SearchHit], kind: &str, vid: i32| {
            assert!(
                hits.iter()
                    .any(|h| h.kind == kind && h.vehicle_id == Some(vid)),
                "expected a {kind} hit for vehicle {vid}: {hits:?}"
            );
        };
        let hits = search(&db, "unicorn", SearchScope::All, None)
            .await
            .unwrap();
        assert_kind(&hits, "vehicle", vid);
        let hits = search(&db, "whistle", SearchScope::All, None)
            .await
            .unwrap();
        assert_kind(&hits, "incident", vid);
        let hits = search(&db, "stoplight", SearchScope::All, None)
            .await
            .unwrap();
        assert_kind(&hits, "incident", vid);
        let hits = search(&db, "adjuster bumper", SearchScope::Incidents, None)
            .await
            .unwrap();
        assert_kind(&hits, "incident_followup", vid);
        let hits = search(&db, "kilonewton", SearchScope::Builds, None)
            .await
            .unwrap();
        assert_kind(&hits, "build", vid);
        let hits = search(&db, "methanol", SearchScope::All, None)
            .await
            .unwrap();
        assert_kind(&hits, "build", vid);
        let hits = search(&db, "impeller", SearchScope::Research, None)
            .await
            .unwrap();
        assert_kind(&hits, "research_finding", vid);
    }

    async fn seed_schedule_item(db: &impl ConnectionTrait, vehicle_id: i32, name: &str) -> i32 {
        entities::maintenance_schedule_item::ActiveModel {
            vehicle_id: Set(Some(vehicle_id)),
            name: Set(name.into()),
            enabled: Set(true),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn seed_work_item(db: &impl ConnectionTrait, vehicle_id: i32, title: &str) -> i32 {
        entities::work_item::ActiveModel {
            vehicle_id: Set(vehicle_id),
            title: Set(title.into()),
            status: Set("planned".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn finds_schedule_item_by_name() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, "Golf R").await;
        let item = seed_schedule_item(&db, vid, "Engine air filter").await;

        let hits = search(&db, "air filter", SearchScope::All, None)
            .await
            .unwrap();
        let hit = hits.iter().find(|h| h.kind == "schedule_item").unwrap();
        assert_eq!(hit.id, item);
        assert_eq!(hit.vehicle_id, Some(vid));
        assert_eq!(hit.title, "Engine air filter");

        // The dedicated scope returns only schedule items.
        let hits = search(&db, "air filter", SearchScope::Maintenance, None)
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.kind == "schedule_item"));
    }

    #[tokio::test]
    async fn inherited_schedule_items_fan_out_per_applicable_vehicle() {
        let db = test_db().await;
        // Platform → template → two vehicles; the platform-level item must
        // surface once per car (each is a distinct Plan → Due destination).
        let platform = entities::platform::ActiveModel {
            name: Set("MQB".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let template = entities::model_template::ActiveModel {
            platform_id: Set(Some(platform.id)),
            model: Set(Some("Golf".into())),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let mut vids = Vec::new();
        for name in ["Golf R", "GTI"] {
            let v = entities::vehicle::ActiveModel {
                name: Set(name.into()),
                model_template_id: Set(Some(template.id)),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
            vids.push(v.id);
        }
        let item = entities::maintenance_schedule_item::ActiveModel {
            platform_id: Set(Some(platform.id)),
            name: Set("Haldex fluid change".into()),
            enabled: Set(true),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;

        let hits = search(&db, "haldex", SearchScope::Maintenance, None)
            .await
            .unwrap();
        let mut hit_vids: Vec<_> = hits
            .iter()
            .filter(|h| h.kind == "schedule_item" && h.id == item)
            .map(|h| h.vehicle_id)
            .collect();
        hit_vids.sort_unstable();
        assert_eq!(
            hit_vids,
            vec![Some(vids[0]), Some(vids[1])],
            "one hit per applicable vehicle: {hits:?}"
        );

        // Vehicle-scoped: the same inherited item resolves to just that car.
        let hits = search(&db, "haldex", SearchScope::All, Some(vids[1]))
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].vehicle_id, Some(vids[1]));
    }

    #[tokio::test]
    async fn finds_work_item_by_title() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, "Golf R").await;
        let wid = seed_work_item(&db, vid, "Install dogbone insert").await;

        let hits = search(&db, "dogbone", SearchScope::All, None)
            .await
            .unwrap();
        let hit = hits.iter().find(|h| h.kind == "work_item").unwrap();
        assert_eq!(hit.id, wid);
        assert_eq!(hit.vehicle_id, Some(vid));
        assert_eq!(hit.title, "Install dogbone insert");

        let hits = search(&db, "dogbone", SearchScope::Todo, None)
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.kind == "work_item"));
    }

    #[tokio::test]
    async fn vehicle_filter_excludes_planning_hits_of_other_vehicles() {
        let db = test_db().await;
        let mine = seed_vehicle(&db, "Golf R").await;
        let other = seed_vehicle(&db, "Jetta").await;
        let my_item = seed_schedule_item(&db, mine, "Cabin air filter").await;
        seed_schedule_item(&db, other, "Cabin air filter too").await;
        let my_work = seed_work_item(&db, mine, "air ride install").await;
        seed_work_item(&db, other, "air suspension quote").await;

        let hits = search(&db, "air", SearchScope::All, Some(mine))
            .await
            .unwrap();
        assert!(hits.iter().all(|h| h.vehicle_id == Some(mine)));
        assert!(
            hits.iter()
                .any(|h| h.kind == "schedule_item" && h.id == my_item)
        );
        assert!(
            hits.iter()
                .any(|h| h.kind == "work_item" && h.id == my_work)
        );
    }

    #[tokio::test]
    async fn operator_and_quote_injection_never_errors() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, "Golf R").await;
        seed_service(&db, vid, "replaced brake pads").await;

        for query in [
            "brake \"pad",
            "NOT AND (",
            "\"",
            "((( )))",
            "brake OR pads",
            "col:brake",
            "brake*",
            "-brake +pads",
        ] {
            let result = search(&db, query, SearchScope::All, None).await;
            assert!(result.is_ok(), "query {query:?} must not error: {result:?}");
        }
        // Bare operators quoted as terms: matches nothing rather than parsing as syntax.
        assert!(
            search(&db, "NOT AND (", SearchScope::All, None)
                .await
                .unwrap()
                .is_empty()
        );
        // "OR" is quoted into a literal term (implicit AND), so this cannot be
        // parsed as an OR-expression: no row contains the token "or" -> empty.
        let hits = search(&db, "brake OR pads", SearchScope::All, None)
            .await
            .unwrap();
        assert!(
            hits.is_empty(),
            "quoted 'OR' must be a literal term, not an operator"
        );
    }

    #[tokio::test]
    async fn empty_query_is_bad_request() {
        let db = test_db().await;
        for q in ["", "   ", "\t\n"] {
            assert!(matches!(
                search(&db, q, SearchScope::All, None).await.unwrap_err(),
                DomainError::BadRequest(_)
            ));
        }
    }

    #[tokio::test]
    async fn missing_vehicle_is_not_found() {
        let db = test_db().await;
        assert!(matches!(
            search(&db, "brake", SearchScope::All, Some(999))
                .await
                .unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn scope_parse_round_trips_and_rejects_unknown() {
        assert_eq!(SearchScope::parse("all").unwrap(), SearchScope::All);
        assert_eq!(
            SearchScope::parse("vehicles").unwrap(),
            SearchScope::Vehicles
        );
        assert_eq!(
            SearchScope::parse("services").unwrap(),
            SearchScope::Services
        );
        assert_eq!(
            SearchScope::parse("incidents").unwrap(),
            SearchScope::Incidents
        );
        assert_eq!(SearchScope::parse("builds").unwrap(), SearchScope::Builds);
        for retired in ["observations", "accidents"] {
            assert!(
                matches!(
                    SearchScope::parse(retired).unwrap_err(),
                    DomainError::BadRequest(_)
                ),
                "retired scope '{retired}' must be rejected"
            );
        }
        assert_eq!(
            SearchScope::parse("documents").unwrap(),
            SearchScope::Documents
        );
        assert_eq!(
            SearchScope::parse("research").unwrap(),
            SearchScope::Research
        );
        assert_eq!(
            SearchScope::parse("maintenance").unwrap(),
            SearchScope::Maintenance
        );
        assert_eq!(SearchScope::parse("todo").unwrap(), SearchScope::Todo);
        assert!(matches!(
            SearchScope::parse("nonsense").unwrap_err(),
            DomainError::BadRequest(_)
        ));
    }
}
