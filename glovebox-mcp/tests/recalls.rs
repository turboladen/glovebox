mod common;

use common::{connect, new_vehicle, seed_recall};
use glovebox_shared::services::vehicle;
use serde_json::{Value, json};

// `check_recalls` makes a live NHTSA request, so no test here calls it, and
// every recall is stored by `seed_recall`.
#[tokio::test]
async fn a_recall_named_on_a_service_reads_as_done_with_that_service() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let recall = seed_recall(&t.db, v.id, "24V110000").await;
    let other = seed_recall(&t.db, v.id, "24V220000").await;

    let service = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2025-03-25",
                "description": "Recall 24V110",
                "recall_finding_ids": [recall]
            }),
        )
        .await;
    assert_eq!(service["performed_recalls"][0]["finding_id"], recall);
    assert_eq!(
        service["performed_recalls"][0]["campaign_number"],
        "24V110000"
    );

    let read = t
        .call(
            "get_service",
            json!({ "vehicle_id": v.id, "service_id": service["id"] }),
        )
        .await;
    assert_eq!(read["performed_recalls"][0]["finding_id"], recall);

    let done = t
        .call(
            "set_recall_status",
            json!({ "vehicle_id": v.id, "finding_id": recall, "status": "done" }),
        )
        .await;
    assert_eq!(done["status"], "done");
    assert_eq!(done["done_service"]["service_date"], "2025-03-25");
    assert_eq!(done["done_service"]["id"], service["id"]);

    let err = t
        .call_err(
            "set_recall_status",
            json!({ "vehicle_id": v.id, "finding_id": recall, "status": "open" }),
        )
        .await;
    assert!(err.contains("linked to a service"), "got: {err}");

    let skipped = t
        .call(
            "set_recall_status",
            json!({ "vehicle_id": v.id, "finding_id": other, "status": "not_applicable" }),
        )
        .await;
    assert_eq!(skipped["status"], "not_applicable");
    assert_eq!(skipped["done_service"], Value::Null);

    t.call_err(
        "set_recall_status",
        json!({ "vehicle_id": v.id, "finding_id": 9999, "status": "done" }),
    )
    .await;

    // `planned` is read from a planned job and cannot be set.
    let err = t
        .call_err(
            "set_recall_status",
            json!({ "vehicle_id": v.id, "finding_id": other, "status": "planned" }),
        )
        .await;
    assert!(
        err.contains("status: unknown variant") && err.contains("`not_applicable`"),
        "got: {err}"
    );
}

#[tokio::test]
async fn list_recalls_shows_every_status_without_a_network_call() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let open = seed_recall(&t.db, v.id, "24V110000").await;
    let done = seed_recall(&t.db, v.id, "24V220000").await;
    let skipped = seed_recall(&t.db, v.id, "24V330000").await;
    let planned = seed_recall(&t.db, v.id, "24V440000").await;
    let item = t
        .call(
            "plan_work",
            json!({ "vehicle_id": v.id, "title": "Recall", "research_finding_id": planned }),
        )
        .await;
    let service = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id, "service_date": "2025-03-25",
                "description": "Recall 24V220", "recall_finding_ids": [done]
            }),
        )
        .await;
    t.call(
        "set_recall_status",
        json!({ "vehicle_id": v.id, "finding_id": skipped, "status": "not_applicable" }),
    )
    .await;

    let out = t.call("list_recalls", json!({ "vehicle_id": v.id })).await;
    let by_id = |id: i32| {
        out["recalls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["finding_id"] == id)
            .unwrap_or_else(|| panic!("recall {id} missing from {out}"))
            .clone()
    };
    assert_eq!(by_id(open)["status"], "open");
    assert_eq!(by_id(open)["campaign_number"], "24V110000");
    assert_eq!(by_id(planned)["status"], "planned");
    assert_eq!(by_id(planned)["work_item_id"], item["id"]);
    assert_eq!(by_id(done)["status"], "done");
    assert_eq!(by_id(done)["done_service"]["id"], service["id"]);
    assert_eq!(by_id(open)["work_item_id"], Value::Null);
    assert_eq!(by_id(skipped)["status"], "not_applicable");
    assert!(by_id(open)["title"].is_string());

    t.call_err("list_recalls", json!({ "vehicle_id": 9999 }))
        .await;
}

#[tokio::test]
async fn a_recall_job_marked_done_without_a_service_reads_done() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let recall = seed_recall(&t.db, v.id, "24V110000").await;
    let item = t
        .call(
            "plan_work",
            json!({ "vehicle_id": v.id, "title": "Recall", "research_finding_id": recall }),
        )
        .await;
    t.call(
        "update_planned_work",
        json!({ "vehicle_id": v.id, "work_item_id": item["id"], "status": "done" }),
    )
    .await;

    let out = t.call("list_recalls", json!({ "vehicle_id": v.id })).await;
    assert_eq!(out["recalls"][0]["status"], "done");
    assert_eq!(out["recalls"][0]["work_item_id"], item["id"]);
    assert_eq!(out["recalls"][0]["done_service"], Value::Null);

    let err = t
        .call_err(
            "set_recall_status",
            json!({ "vehicle_id": v.id, "finding_id": recall, "status": "open" }),
        )
        .await;
    assert!(err.contains("has a job marked done"), "got: {err}");
}

async fn file_finding(t: &common::TestClient, vehicle_id: i32, title: &str) -> i64 {
    let filed = t
        .call(
            "file_research_finding",
            json!({ "vehicle_id": vehicle_id, "category": "known_issue", "title": title }),
        )
        .await;
    common::id(&filed)
}

// A finding is filed, listed, read, planned, corrected and deleted through
// tool calls only, and its job outlives it.
#[tokio::test]
#[expect(clippy::too_many_lines, reason = "one finding's whole life")]
async fn a_finding_is_filed_read_planned_corrected_and_deleted() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();

    let filed = t
        .call(
            "file_research_finding",
            json!({
                "vehicle_id": v.id,
                "category": "known_issue",
                "title": "Water pump fails near 60k",
                "description": "The housing cracks and leaks coolant.",
                "source_url": "https://example.com/t",
                "severity": "recommended"
            }),
        )
        .await;
    let id = common::id(&filed);
    assert_eq!(filed["status"], "open");
    assert_eq!(filed["category"], "known_issue");
    assert_eq!(filed["severity"], "recommended");
    assert_eq!(filed["work_item"], Value::Null);
    assert_eq!(filed["done_service"], Value::Null);

    let listed = t
        .call("list_research_findings", json!({ "vehicle_id": v.id }))
        .await;
    assert_eq!(listed["findings"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["findings"][0]["id"], id);
    assert_eq!(
        listed["findings"][0]["description"],
        "The housing cracks and leaks coolant."
    );

    let job = t
        .call(
            "plan_work",
            json!({
                "vehicle_id": v.id,
                "title": "Replace the water pump",
                "research_finding_id": id
            }),
        )
        .await;
    let read = t
        .call(
            "get_research_finding",
            json!({ "vehicle_id": v.id, "finding_id": id }),
        )
        .await;
    assert_eq!(read["status"], "planned");
    assert_eq!(
        read["work_item"],
        json!({
            "id": job["id"],
            "title": "Replace the water pump",
            "status": "planned",
            "appointment_id": null
        })
    );
    let planned = t
        .call(
            "list_research_findings",
            json!({ "vehicle_id": v.id, "status": "planned", "has_planned_work": true }),
        )
        .await;
    assert_eq!(planned["findings"][0]["id"], id);
    let open = t
        .call(
            "list_research_findings",
            json!({ "vehicle_id": v.id, "status": "open" }),
        )
        .await;
    assert_eq!(open["findings"], json!([]));

    let updated = t
        .call(
            "update_research_finding",
            json!({
                "vehicle_id": v.id,
                "finding_id": id,
                "title": "Water pump housing leaks near 60k",
                "category": "forum_report",
                "severity": "critical",
                "source_url": null
            }),
        )
        .await;
    assert_eq!(updated["title"], "Water pump housing leaks near 60k");
    assert_eq!(updated["category"], "forum_report");
    assert_eq!(updated["severity"], "critical");
    assert_eq!(updated["source_url"], Value::Null);
    assert_eq!(
        updated["description"],
        "The housing cracks and leaks coolant."
    );
    assert_eq!(updated["status"], "planned");

    let service = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v.id,
                "service_date": "2026-03-25",
                "description": "Water pump",
                "work_item_ids": [job["id"]]
            }),
        )
        .await;
    let done = t
        .call(
            "get_research_finding",
            json!({ "vehicle_id": v.id, "finding_id": id }),
        )
        .await;
    assert_eq!(done["status"], "done");
    assert_eq!(done["work_item"]["status"], "done");
    assert_eq!(done["done_service"]["id"], service["id"]);
    assert_eq!(done["done_service"]["service_date"], "2026-03-25");
    let err = t
        .call_err(
            "update_research_finding",
            json!({ "vehicle_id": v.id, "finding_id": id, "status": "open" }),
        )
        .await;
    assert!(err.contains("has a job marked done"), "got: {err}");

    let deleted = t
        .call(
            "delete_research_finding",
            json!({ "vehicle_id": v.id, "finding_id": id }),
        )
        .await;
    assert_eq!(deleted["deleted"], "research finding");
    assert_eq!(deleted["id"], id);
    assert_eq!(deleted["title"], "Water pump housing leaks near 60k");
    assert_eq!(
        deleted["unlinked_work_items"],
        json!([{
            "id": job["id"],
            "title": "Replace the water pump",
            "status": "done",
            "appointment_id": null
        }])
    );
    let jobs = t
        .call(
            "list_planned_work",
            json!({ "vehicle_id": v.id, "include_done": true }),
        )
        .await;
    assert_eq!(jobs["items"][0]["id"], job["id"]);
    assert_eq!(jobs["items"][0]["status"], "done");
    assert_eq!(jobs["items"][0]["research_finding_id"], Value::Null);
    let left = t
        .call("list_research_findings", json!({ "vehicle_id": v.id }))
        .await;
    assert_eq!(left["findings"], json!([]));
}

#[tokio::test]
async fn a_finding_is_marked_dismissed_done_and_open_again() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let id = file_finding(&t, v.id, "Intake upgrade").await;

    for status in ["dismissed", "done", "open"] {
        let read = t
            .call(
                "update_research_finding",
                json!({ "vehicle_id": v.id, "finding_id": id, "status": status }),
            )
            .await;
        assert_eq!(read["status"], status);
    }

    // `planned` is read from a planned job and cannot be set.
    let err = t
        .call_err(
            "update_research_finding",
            json!({ "vehicle_id": v.id, "finding_id": id, "status": "planned" }),
        )
        .await;
    assert!(
        err.contains("status: unknown variant") && err.contains("`dismissed`"),
        "got: {err}"
    );
}

#[tokio::test]
async fn the_finding_tools_refuse_to_make_a_recall() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();

    let err = t
        .call_err(
            "file_research_finding",
            json!({ "vehicle_id": v.id, "category": "recall", "title": "Airbag" }),
        )
        .await;
    assert!(
        err.contains("category: a recall comes only from an NHTSA recall check"),
        "got: {err}"
    );
    let recalls = t.call("list_recalls", json!({ "vehicle_id": v.id })).await;
    assert_eq!(recalls["recalls"], json!([]));

    let id = file_finding(&t, v.id, "Water pump").await;
    let err = t
        .call_err(
            "update_research_finding",
            json!({ "vehicle_id": v.id, "finding_id": id, "category": "recall" }),
        )
        .await;
    assert!(
        err.contains("category: a finding cannot become a recall"),
        "got: {err}"
    );
    let err = t
        .call_err(
            "list_research_findings",
            json!({ "vehicle_id": v.id, "category": "recall" }),
        )
        .await;
    assert!(
        err.contains("category: recalls are listed with the vehicle's recalls"),
        "got: {err}"
    );

    // A category or a severity outside its list is refused with the list.
    let err = t
        .call_err(
            "file_research_finding",
            json!({ "vehicle_id": v.id, "category": "maintenance", "title": "x" }),
        )
        .await;
    assert!(
        err.contains("category: unknown variant") && err.contains("`suggested_maintenance`"),
        "got: {err}"
    );
    let err = t
        .call_err(
            "file_research_finding",
            json!({
                "vehicle_id": v.id, "category": "other", "title": "x", "severity": "info"
            }),
        )
        .await;
    assert!(
        err.contains("severity: unknown variant") && err.contains("`informational`"),
        "got: {err}"
    );
}

// A recall's id, another vehicle's finding and an id that names nothing read
// the same through every finding tool, and nothing is changed.
#[tokio::test]
async fn the_finding_tools_read_a_recall_and_a_foreign_finding_as_missing() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let other = vehicle::create(&t.db, new_vehicle("Other")).await.unwrap();
    let recall = seed_recall(&t.db, v.id, "24V110000").await;
    let foreign = file_finding(&t, other.id, "Other car's finding").await;

    for id in [i64::from(recall), foreign, 9999] {
        let expected = format!("Research finding {id} not found");
        for (tool, extra) in [
            ("get_research_finding", json!({})),
            (
                "update_research_finding",
                json!({ "title": "Changed", "status": "dismissed" }),
            ),
            ("delete_research_finding", json!({})),
        ] {
            let mut args = json!({ "vehicle_id": v.id, "finding_id": id });
            args.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            assert_eq!(t.call_err(tool, args).await, expected, "{tool} {id}");
        }
    }

    let recalls = t.call("list_recalls", json!({ "vehicle_id": v.id })).await;
    assert_eq!(recalls["recalls"][0]["finding_id"], recall);
    assert_eq!(recalls["recalls"][0]["status"], "open");
    assert_eq!(recalls["recalls"][0]["title"], "Suction Jet Pump");
    let theirs = t
        .call(
            "get_research_finding",
            json!({ "vehicle_id": other.id, "finding_id": foreign }),
        )
        .await;
    assert_eq!(theirs["title"], "Other car's finding");
    assert_eq!(theirs["status"], "open");

    // The recall tools read a finding as missing in turn.
    let mine = file_finding(&t, v.id, "Water pump").await;
    let err = t
        .call_err(
            "set_recall_status",
            json!({ "vehicle_id": v.id, "finding_id": mine, "status": "done" }),
        )
        .await;
    assert_eq!(err, format!("Recall {mine} not found"));
}

#[tokio::test]
async fn list_recalls_and_list_research_findings_never_show_each_others_rows() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let recall = seed_recall(&t.db, v.id, "24V110000").await;
    let finding = file_finding(&t, v.id, "Water pump").await;

    let recalls = t.call("list_recalls", json!({ "vehicle_id": v.id })).await;
    let recall_ids: Vec<&Value> = recalls["recalls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| &r["finding_id"])
        .collect();
    assert_eq!(recall_ids, vec![&json!(recall)]);

    let findings = t
        .call("list_research_findings", json!({ "vehicle_id": v.id }))
        .await;
    let finding_ids: Vec<&Value> = findings["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| &f["id"])
        .collect();
    assert_eq!(finding_ids, vec![&json!(finding)]);

    t.call_err("list_research_findings", json!({ "vehicle_id": 9999 }))
        .await;
    let err = t
        .call_err(
            "list_research_findings",
            json!({ "vehicle_id": v.id, "limit": 0 }),
        )
        .await;
    assert_eq!(err, "limit: must be at least 1");
}

// An open finding shows in `whats_coming` with its planned job until that
// job goes on an appointment, and a recall never shows among the findings.
#[tokio::test]
async fn whats_coming_lists_an_open_finding_until_its_job_is_booked() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    seed_recall(&t.db, v.id, "24V110000").await;
    let id = file_finding(&t, v.id, "Water pump fails near 60k").await;

    let coming = t.call("whats_coming", json!({ "vehicle_id": v.id })).await;
    assert_eq!(coming["open_findings_total"], 1);
    assert_eq!(coming["open_findings"][0]["id"], id);
    assert_eq!(coming["open_findings"][0]["status"], "open");
    assert_eq!(coming["recalls"].as_array().map(Vec::len), Some(1));

    let job = t
        .call(
            "plan_work",
            json!({
                "vehicle_id": v.id, "title": "Replace the water pump", "research_finding_id": id
            }),
        )
        .await;
    let coming = t.call("whats_coming", json!({ "vehicle_id": v.id })).await;
    assert_eq!(coming["open_findings"][0]["status"], "planned");
    assert_eq!(coming["open_findings"][0]["work_item"]["id"], job["id"]);

    let visit = t
        .call(
            "plan_appointment",
            json!({ "vehicle_id": v.id, "purpose": "60k service" }),
        )
        .await;
    t.call(
        "update_planned_work",
        json!({
            "vehicle_id": v.id, "work_item_id": job["id"], "appointment_id": visit["id"]
        }),
    )
    .await;
    let coming = t.call("whats_coming", json!({ "vehicle_id": v.id })).await;
    assert_eq!(coming["open_findings"], json!([]));
    assert_eq!(coming["open_findings_total"], 0);
}

#[tokio::test]
async fn recalls_tools_reject_unknown_arguments() {
    let t = connect().await;
    common::assert_unknown_field_rejected(
        &t,
        "set_recall_status",
        json!({ "vehicle_id": 1, "finding_id": 1, "status": "done", "campaign": "20UF" }),
        "campaign",
    )
    .await;
    for (tool, args, field) in [
        (
            "file_research_finding",
            json!({ "vehicle_id": 1, "category": "other", "title": "x", "status": "open" }),
            "status",
        ),
        (
            "list_research_findings",
            json!({ "vehicle_id": 1, "severity": "critical" }),
            "severity",
        ),
        (
            "get_research_finding",
            json!({ "vehicle_id": 1, "finding_id": 1, "id": 1 }),
            "id",
        ),
        (
            "update_research_finding",
            json!({ "vehicle_id": 1, "finding_id": 1, "notes": "x" }),
            "notes",
        ),
        (
            "delete_research_finding",
            json!({ "vehicle_id": 1, "finding_id": 1, "force": true }),
            "force",
        ),
    ] {
        common::assert_unknown_field_rejected(&t, tool, args, field).await;
    }
}

// A search hit says whether it is a recall or a finding, and each kind's id
// works in its own tools and is missing in the other's.
#[tokio::test]
async fn a_search_hit_names_its_kind_and_its_id_works_in_that_kinds_tools() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let recall = seed_recall(&t.db, v.id, "24V110000").await;
    let finding = file_finding(&t, v.id, "Water pump housing leaks").await;

    let hits = t
        .call(
            "search_records",
            json!({ "query": "pump", "scope": "research", "vehicle_id": v.id }),
        )
        .await;
    let id_of = |kind: &str| {
        let found: Vec<&Value> = hits
            .as_array()
            .unwrap()
            .iter()
            .filter(|h| h["kind"] == kind)
            .collect();
        assert_eq!(found.len(), 1, "one {kind} hit in {hits}");
        found[0]["id"].clone()
    };
    assert_eq!(id_of("recall"), recall);
    assert_eq!(id_of("research_finding"), finding);
    assert_eq!(hits.as_array().map(Vec::len), Some(2));

    let read = t
        .call(
            "get_research_finding",
            json!({ "vehicle_id": v.id, "finding_id": id_of("research_finding") }),
        )
        .await;
    assert_eq!(read["title"], "Water pump housing leaks");
    let marked = t
        .call(
            "set_recall_status",
            json!({
                "vehicle_id": v.id, "finding_id": id_of("recall"), "status": "not_applicable"
            }),
        )
        .await;
    assert_eq!(marked["status"], "not_applicable");
    let listed = t.call("list_recalls", json!({ "vehicle_id": v.id })).await;
    assert_eq!(listed["recalls"][0]["finding_id"], id_of("recall"));

    t.call_err(
        "get_research_finding",
        json!({ "vehicle_id": v.id, "finding_id": id_of("recall") }),
    )
    .await;
    t.call_err(
        "set_recall_status",
        json!({
            "vehicle_id": v.id, "finding_id": id_of("research_finding"), "status": "done"
        }),
    )
    .await;
}
