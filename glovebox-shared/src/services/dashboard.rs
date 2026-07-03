//! Garage-wide dashboard aggregation (2hea unit F, decision ⑥): everything
//! the login landing needs in ONE call — per-vehicle status summaries for
//! the sidebar, the "needs attention" rollup, upcoming visits, the summed
//! 12-month budget, and active-build snapshots.
//!
//! Composition, not new queries: reminders come from
//! [`reminders::calculate_reminders`], forecasts from
//! [`budget::forecast_from`] (fed the already-computed reminders), findings/
//! incidents/work items/visits/builds from their list fns. The outer shape
//! is a per-vehicle loop — this is a personal app with FEW vehicles, so N
//! sequential per-vehicle computations are deliberate and fine.

use sea_orm::DatabaseConnection;
use serde::Serialize;

use crate::{
    entities::vehicle,
    error::DomainResult,
    services::{
        budget, build,
        build::BuildProgress,
        incident, reminders, research,
        visit::{self, VisitWithItems},
        work_item,
    },
};

/// Just enough of an active build for a sidebar chip.
#[derive(Debug, Serialize)]
pub struct ActiveBuildBrief {
    pub id: i32,
    pub name: String,
}

/// One vehicle's status line: the full record (the sidebar and scoped
/// Overview both need name/year/make/model/photo) plus derived counts.
/// Archived vehicles appear (the sidebar lists them) but with zeroed
/// counts and no forecast — an archived car needs no attention.
#[derive(Debug, Serialize)]
pub struct VehicleSummary {
    pub vehicle: vehicle::Model,
    pub estimated_mileage: Option<i32>,
    pub overdue_count: usize,
    pub due_soon_count: usize,
    /// Research findings with `category = "recall"` and `status = "new"`.
    pub open_recall_count: usize,
    /// Unresolved incidents, excluding `category = "note"` — notes are
    /// remembered facts, not problems needing attention.
    pub unresolved_incident_count: usize,
    /// Participating work items not attached to any visit (the to-do
    /// backlog).
    pub unscheduled_work_count: usize,
    /// This vehicle's 12-month forecast total ([`budget::forecast_from`]).
    pub forecast_total_cents: i64,
    /// The first `active`-status build, if any.
    pub active_build: Option<ActiveBuildBrief>,
}

/// One row in the "Needs attention" block. `entity_id` is the underlying
/// record (schedule item / research finding / incident) so quick actions
/// like "plan it" can link the source; `deep_link_hint` names where the
/// row navigates (`plan/due`, `records/research`, `timeline`).
#[derive(Debug, Serialize)]
pub struct AttentionItem {
    pub vehicle_id: i32,
    pub vehicle_name: String,
    /// `overdue`, `due_soon`, `recall`, or `incident`.
    pub kind: String,
    pub label: String,
    pub entity_id: i32,
    pub deep_link_hint: String,
}

/// An open visit with its owning vehicle named for garage-wide display.
#[derive(Debug, Serialize)]
pub struct UpcomingVisit {
    pub vehicle_id: i32,
    pub vehicle_name: String,
    #[serde(flatten)]
    pub visit: VisitWithItems,
}

/// An active build's live progress with its owning vehicle named.
#[derive(Debug, Serialize)]
pub struct BuildSnapshot {
    pub vehicle_id: i32,
    pub vehicle_name: String,
    #[serde(flatten)]
    pub progress: BuildProgress,
}

#[derive(Debug, Serialize)]
pub struct GarageDashboard {
    pub vehicles: Vec<VehicleSummary>,
    pub attention: Vec<AttentionItem>,
    /// Open (planned/scheduled) visits across all vehicles.
    pub upcoming_visits: Vec<UpcomingVisit>,
    /// Σ per-vehicle 12-month forecast totals. Integer cents.
    pub budget_total_cents: i64,
    pub active_builds: Vec<BuildSnapshot>,
}

/// Build the garage-wide dashboard. Per-vehicle loop by design (few
/// vehicles); reminders are computed once per vehicle and feed both the
/// counts and the budget forecast.
#[allow(clippy::too_many_lines)] // one linear per-vehicle aggregation pass
pub async fn garage(db: &DatabaseConnection) -> DomainResult<GarageDashboard> {
    let all_vehicles = crate::services::vehicle::list(db).await?;

    let mut vehicles: Vec<VehicleSummary> = Vec::new();
    let mut attention: Vec<AttentionItem> = Vec::new();
    let mut upcoming_visits: Vec<UpcomingVisit> = Vec::new();
    let mut budget_total_cents: i64 = 0;
    let mut active_builds: Vec<BuildSnapshot> = Vec::new();

    for v in all_vehicles {
        if v.archived_at.is_some() {
            vehicles.push(VehicleSummary {
                vehicle: v,
                estimated_mileage: None,
                overdue_count: 0,
                due_soon_count: 0,
                open_recall_count: 0,
                unresolved_incident_count: 0,
                unscheduled_work_count: 0,
                forecast_total_cents: 0,
                active_build: None,
            });
            continue;
        }

        let rems = reminders::calculate_reminders(db, v.id).await?;
        let forecast = budget::forecast_from(db, v.id, &rems).await?;

        let mut overdue_count = 0;
        let mut due_soon_count = 0;
        for r in &rems.reminders {
            match r.status.as_str() {
                "overdue" => {
                    overdue_count += 1;
                    attention.push(AttentionItem {
                        vehicle_id: v.id,
                        vehicle_name: v.name.clone(),
                        kind: "overdue".into(),
                        label: overdue_label(r),
                        entity_id: r.schedule_item.id,
                        deep_link_hint: "plan/due".into(),
                    });
                }
                "upcoming" => {
                    due_soon_count += 1;
                    attention.push(AttentionItem {
                        vehicle_id: v.id,
                        vehicle_name: v.name.clone(),
                        kind: "due_soon".into(),
                        label: due_soon_label(r),
                        entity_id: r.schedule_item.id,
                        deep_link_hint: "plan/due".into(),
                    });
                }
                _ => {}
            }
        }

        // Open recalls: new recall-category findings.
        let new_findings = research::list_findings(db, v.id, Some("new".into())).await?;
        let open_recalls: Vec<_> = new_findings
            .iter()
            .filter(|f| f.category == "recall")
            .collect();
        for f in &open_recalls {
            attention.push(AttentionItem {
                vehicle_id: v.id,
                vehicle_name: v.name.clone(),
                kind: "recall".into(),
                label: f.title.clone(),
                entity_id: f.id,
                deep_link_hint: "records/research".into(),
            });
        }

        // Unresolved incidents (notes excluded — they're facts, not
        // problems).
        let incidents = incident::list(db, v.id).await?;
        let unresolved: Vec<_> = incidents
            .iter()
            .filter(|i| !i.incident.resolved && i.incident.category != "note")
            .collect();
        for i in &unresolved {
            attention.push(AttentionItem {
                vehicle_id: v.id,
                vehicle_name: v.name.clone(),
                kind: "incident".into(),
                label: i.incident.title.clone(),
                entity_id: i.incident.id,
                deep_link_hint: "timeline".into(),
            });
        }

        let open_items = work_item::list(db, v.id, false).await?;
        let unscheduled_work_count = open_items.iter().filter(|i| i.visit_id.is_none()).count();

        for open in visit::list(db, v.id, false).await? {
            upcoming_visits.push(UpcomingVisit {
                vehicle_id: v.id,
                vehicle_name: v.name.clone(),
                visit: open,
            });
        }

        let mut active_build = None;
        for b in build::list(db, v.id).await? {
            if b.status == "active" {
                if active_build.is_none() {
                    active_build = Some(ActiveBuildBrief {
                        id: b.id,
                        name: b.name.clone(),
                    });
                }
                active_builds.push(BuildSnapshot {
                    vehicle_id: v.id,
                    vehicle_name: v.name.clone(),
                    progress: build::progress(db, v.id, b.id).await?,
                });
            }
        }

        budget_total_cents += forecast.total_cents;
        vehicles.push(VehicleSummary {
            estimated_mileage: Some(rems.estimated_mileage),
            overdue_count,
            due_soon_count,
            open_recall_count: open_recalls.len(),
            unresolved_incident_count: unresolved.len(),
            unscheduled_work_count,
            forecast_total_cents: forecast.total_cents,
            active_build,
            vehicle: v,
        });
    }

    // Severity-major ordering: overdue, then recalls, then due-soon, then
    // incidents — stable within a kind (vehicle order preserved).
    let severity = |kind: &str| match kind {
        "overdue" => 0,
        "recall" => 1,
        "due_soon" => 2,
        _ => 3,
    };
    attention.sort_by_key(|a| severity(&a.kind));

    Ok(GarageDashboard {
        vehicles,
        attention,
        upcoming_visits,
        budget_total_cents,
        active_builds,
    })
}

fn overdue_label(r: &reminders::ReminderStatus) -> String {
    let name = &r.schedule_item.name;
    match (r.miles_remaining, r.days_remaining) {
        (Some(mi), _) if mi < 0 => format!("{name} — overdue by {} mi", -mi),
        (_, Some(days)) if days < 0 => format!("{name} — overdue by {} days", -days),
        _ => format!("{name} — overdue"),
    }
}

fn due_soon_label(r: &reminders::ReminderStatus) -> String {
    let name = &r.schedule_item.name;
    match (r.miles_remaining, r.days_remaining) {
        (Some(mi), _) if mi >= 0 => format!("{name} — due in {mi} mi"),
        (_, Some(days)) if days >= 0 => format!("{name} — due in {days} days"),
        _ => format!("{name} — due soon"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::*;

    use crate::{
        entities::{maintenance_schedule_item, research_finding, research_report},
        inputs::{build::NewBuild, incident::NewIncident, visit::NewVisit, work_item::NewWorkItem},
        services::{build as build_svc, incident as incident_svc, work_item as work_item_svc},
        test_support::test_db,
    };

    async fn seed_vehicle(db: &impl ConnectionTrait, name: &str) -> i32 {
        use crate::entities::vehicle;
        vehicle::ActiveModel {
            name: Set(name.into()),
            purchase_date: Set(Some("2020-01-01".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    /// 12-month item on a 2020-purchased vehicle, never serviced → overdue.
    async fn seed_overdue_item(db: &impl ConnectionTrait, vehicle_id: i32, cents: i32) -> i32 {
        maintenance_schedule_item::ActiveModel {
            vehicle_id: Set(Some(vehicle_id)),
            name: Set("Brake fluid flush".into()),
            interval_months: Set(Some(12)),
            est_cost_cents: Set(Some(cents)),
            enabled: Set(true),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn seed_recall(db: &impl ConnectionTrait, vehicle_id: i32) -> i32 {
        let report = research_report::ActiveModel {
            vehicle_id: Set(vehicle_id),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        research_finding::ActiveModel {
            report_id: Set(report.id),
            category: Set("recall".into()),
            title: Set("Recall: fuel pump".into()),
            status: Set("new".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    fn work(title: &str, est: Option<i32>) -> NewWorkItem {
        NewWorkItem {
            title: title.into(),
            notes: None,
            schedule_item_id: None,
            research_finding_id: None,
            incident_id: None,
            build_id: None,
            est_cost_cents: est,
            visit_id: None,
        }
    }

    #[tokio::test]
    async fn empty_garage_dashboards_to_nothing() {
        let db = test_db().await;
        let d = garage(&db).await.unwrap();
        assert!(d.vehicles.is_empty());
        assert!(d.attention.is_empty());
        assert!(d.upcoming_visits.is_empty());
        assert!(d.active_builds.is_empty());
        assert_eq!(d.budget_total_cents, 0);
    }

    #[tokio::test]
    async fn aggregates_counts_attention_visits_budget_and_builds() {
        let db = test_db().await;
        let golf = seed_vehicle(&db, "Golf").await;
        let van = seed_vehicle(&db, "Vanagon").await;

        // Golf: an overdue item, an open recall, an unresolved incident,
        // a resolved incident, and a note (the latter two must not count).
        let sched = seed_overdue_item(&db, golf, 10_000).await;
        let recall = seed_recall(&db, golf).await;
        let inc = incident_svc::create(
            &db,
            golf,
            NewIncident {
                category: "noise".into(),
                title: "Squeak".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let resolved = incident_svc::create(
            &db,
            golf,
            NewIncident {
                category: "leak".into(),
                title: "Old leak".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        incident_svc::update(
            &db,
            golf,
            resolved.incident.id,
            crate::inputs::incident::UpdateIncident {
                resolved: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        incident_svc::create(
            &db,
            golf,
            NewIncident {
                category: "note".into(),
                title: "Prefers 5W-40".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Golf planning: one unattached item, one item in an open visit.
        work_item_svc::create(&db, golf, work("Wipers", Some(2_500)))
            .await
            .unwrap();
        let attached = work_item_svc::create(&db, golf, work("Brakes", Some(10_000)))
            .await
            .unwrap();
        visit::create(
            &db,
            golf,
            NewVisit {
                planned_date: Some("2026-08-01".into()),
                shop_name: Some("Joe's".into()),
                shop_id: None,
                notes: None,
                work_item_ids: Some(vec![attached.id]),
            },
        )
        .await
        .unwrap();

        // Vanagon: an active build (plus a planned one that must not count).
        let active = build_svc::create(
            &db,
            van,
            NewBuild {
                name: "Westy revival".into(),
                description: None,
                target_date: None,
            },
        )
        .await
        .unwrap();
        build_svc::update(
            &db,
            van,
            active.id,
            crate::inputs::build::UpdateBuild {
                status: Some("active".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        build_svc::create(
            &db,
            van,
            NewBuild {
                name: "Someday lift kit".into(),
                description: None,
                target_date: None,
            },
        )
        .await
        .unwrap();

        let d = garage(&db).await.unwrap();

        assert_eq!(d.vehicles.len(), 2);
        let g = d.vehicles.iter().find(|s| s.vehicle.id == golf).unwrap();
        assert_eq!(g.overdue_count, 1);
        assert_eq!(g.due_soon_count, 0);
        assert_eq!(g.open_recall_count, 1);
        assert_eq!(g.unresolved_incident_count, 1, "resolved + note excluded");
        assert_eq!(g.unscheduled_work_count, 1, "attached item not counted");
        assert!(g.estimated_mileage.is_some());
        assert!(g.active_build.is_none());

        let v = d.vehicles.iter().find(|s| s.vehicle.id == van).unwrap();
        assert_eq!(
            v.active_build.as_ref().map(|b| b.name.as_str()),
            Some("Westy revival")
        );

        // Attention: overdue → recall → incident, all Golf's, with the
        // right sources and deep-link hints.
        assert_eq!(
            d.attention
                .iter()
                .map(|a| (a.kind.as_str(), a.entity_id, a.deep_link_hint.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("overdue", sched, "plan/due"),
                ("recall", recall, "records/research"),
                ("incident", inc.incident.id, "timeline"),
            ]
        );
        assert!(d.attention[0].label.contains("Brake fluid flush"));
        assert!(d.attention.iter().all(|a| a.vehicle_name == "Golf"));

        // Upcoming visits: the one open visit, vehicle-labeled, rolled up.
        assert_eq!(d.upcoming_visits.len(), 1);
        assert_eq!(d.upcoming_visits[0].vehicle_name, "Golf");
        assert_eq!(d.upcoming_visits[0].visit.est_total_cents, 10_000);

        // Budget: Golf's forecast total (overdue occurrence 10_000 skipped —
        // its schedule item is... NOT work-item-linked, so it projects:
        // 10_000 projected + 10_000 visit + 2_500 backlog), Vanagon 0.
        let g_expected = 10_000 + 10_000 + 2_500;
        assert_eq!(g.forecast_total_cents, g_expected);
        assert_eq!(d.budget_total_cents, g_expected);

        // Build snapshots: only the active build.
        assert_eq!(d.active_builds.len(), 1);
        assert_eq!(d.active_builds[0].vehicle_name, "Vanagon");
        assert_eq!(d.active_builds[0].progress.build.name, "Westy revival");
    }

    #[tokio::test]
    async fn archived_vehicles_are_listed_but_contribute_nothing() {
        let db = test_db().await;
        let vid = seed_vehicle(&db, "Old Car").await;
        seed_overdue_item(&db, vid, 10_000).await;
        crate::services::vehicle::archive(&db, vid).await.unwrap();

        let d = garage(&db).await.unwrap();
        assert_eq!(d.vehicles.len(), 1, "still listed for the sidebar");
        assert_eq!(d.vehicles[0].overdue_count, 0);
        assert_eq!(d.vehicles[0].estimated_mileage, None);
        assert!(d.attention.is_empty());
        assert_eq!(d.budget_total_cents, 0);
    }
}
