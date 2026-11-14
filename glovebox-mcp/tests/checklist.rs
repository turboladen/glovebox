// The job-checklist tools: appointments, needs, options, purchases, to-dos and
// past decisions, driven through tool calls.

mod common;

use common::{TestClient, assert_unknown_field_rejected, connect};
use serde_json::{Value, json};

fn id(value: &Value) -> i64 {
    value["id"]
        .as_i64()
        .unwrap_or_else(|| panic!("payload must carry an id; got: {value}"))
}

async fn add_vehicle(t: &TestClient, name: &str) -> i64 {
    id(&t.call("add_vehicle", json!({ "name": name })).await)
}

async fn plan_appointment(t: &TestClient, vehicle_id: i64, purpose: &str) -> i64 {
    id(&t
        .call(
            "plan_appointment",
            json!({ "vehicle_id": vehicle_id, "purpose": purpose }),
        )
        .await)
}

async fn plan_job(
    t: &TestClient,
    vehicle_id: i64,
    title: &str,
    appointment_id: Option<i64>,
) -> i64 {
    id(&t
        .call(
            "plan_work",
            json!({ "vehicle_id": vehicle_id, "title": title, "appointment_id": appointment_id }),
        )
        .await)
}

async fn add_need(t: &TestClient, vehicle_id: i64, job_id: i64, name: &str) -> i64 {
    id(&t
        .call(
            "add_need",
            json!({ "vehicle_id": vehicle_id, "work_item_id": job_id, "name": name }),
        )
        .await)
}

async fn add_option(t: &TestClient, vehicle_id: i64, need_id: i64, args: Value) -> i64 {
    let mut args = args;
    args["vehicle_id"] = json!(vehicle_id);
    args["need_id"] = json!(need_id);
    id(&t.call("add_need_option", args).await)
}

/// The names of the rows in one next-steps group of a checklist.
fn names(checklist: &Value, group: &str) -> Vec<String> {
    checklist["next_steps"][group]
        .as_array()
        .unwrap_or_else(|| panic!("no group {group}; got: {checklist}"))
        .iter()
        .map(|row| {
            row["name"]
                .as_str()
                .or_else(|| row["title"].as_str())
                .expect("row name or title")
                .to_owned()
        })
        .collect()
}

#[tokio::test]
async fn sixty_k_story() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    let appt = plan_appointment(&t, v, "60k service + shore up loose ends").await;
    let brakes = plan_job(&t, v, "Front and rear brakes", Some(appt)).await;
    let plugs = plan_job(&t, v, "Spark plugs", Some(appt)).await;
    let rotors = add_need(&t, v, brakes, "Rear rotors").await;
    add_need(&t, v, brakes, "Rear pads").await;
    t.call(
        "add_need",
        json!({ "vehicle_id": v, "work_item_id": plugs, "name": "Spark plugs", "supplied_by": "shop" }),
    )
    .await;

    let too_small = add_option(
        &t,
        v,
        rotors,
        json!({
            "description": "272 mm rotor", "manufacturer": "Zimmermann",
            "part_number": "600.3241.20", "fitment": "does_not_fit",
            "fitment_note": "272 mm belongs to the base brakes"
        }),
    )
    .await;
    let right = add_option(
        &t,
        v,
        rotors,
        json!({
            "description": "310 mm rotor", "manufacturer": "Brembo", "part_number": "08.B413.11",
            "fitment": "confirmed", "fitment_note": "Performance Pack", "price_cents": 18_000
        }),
    )
    .await;
    add_option(
        &t,
        v,
        rotors,
        json!({ "description": "310 mm slotted rotor", "manufacturer": "Power Stop" }),
    )
    .await;

    // A part that does not fit cannot be chosen, and the refusal leaves the
    // other fields of the same call unapplied.
    let err = t
        .call_err(
            "update_need",
            json!({
                "vehicle_id": v, "need_id": rotors, "supplied_by": "self",
                "chosen_option_id": too_small, "decision_reason": "cheapest"
            }),
        )
        .await;
    assert!(
        err.contains("does not fit") && err.contains("fitment"),
        "got: {err}"
    );
    let checklist = t
        .call(
            "get_checklist",
            json!({ "vehicle_id": v, "appointment_id": appt }),
        )
        .await;
    assert!(names(&checklist, "decide").contains(&"Rear rotors".to_owned()));

    let chosen = t
        .call(
            "update_need",
            json!({
                "vehicle_id": v, "need_id": rotors, "supplied_by": "self",
                "chosen_option_id": right, "decision_reason": "Matches the Performance Pack"
            }),
        )
        .await;
    assert_eq!(chosen["chosen_option_id"], right);
    assert_eq!(chosen["decision_reason"], "Matches the Performance Pack");

    let checklist = t
        .call(
            "get_checklist",
            json!({ "vehicle_id": v, "appointment_id": appt }),
        )
        .await;
    assert_eq!(names(&checklist, "order"), ["Rear rotors"]);
    assert_eq!(names(&checklist, "decide"), ["Rear pads"]);
    assert_eq!(names(&checklist, "done"), ["Spark plugs"]);
    assert_eq!(
        checklist["appointment"]["purpose"],
        "60k service + shore up loose ends"
    );

    let ordered = t
        .call(
            "mark_need_ordered",
            json!({ "vehicle_id": v, "need_id": rotors, "order_ref": "FCP-1001" }),
        )
        .await;
    assert_eq!(ordered["stage"], "ordered");
    assert_eq!(ordered["part"]["order_ref"], "FCP-1001");
    assert_eq!(ordered["part"]["part_number"], "08.B413.11");
    let checklist = t
        .call(
            "get_checklist",
            json!({ "vehicle_id": v, "appointment_id": appt }),
        )
        .await;
    assert_eq!(names(&checklist, "arriving"), ["Rear rotors"]);

    let received = t
        .call(
            "mark_need_received",
            json!({ "vehicle_id": v, "need_id": rotors }),
        )
        .await;
    assert_eq!(received["stage"], "received");

    let returned = t
        .call(
            "return_part",
            json!({ "vehicle_id": v, "need_id": rotors, "reason": "Box held 272 mm rotors" }),
        )
        .await;
    assert_eq!(returned["stage"], "to_order");
    assert_eq!(returned["returned_part"]["status"], "returned");
    let checklist = t
        .call(
            "get_checklist",
            json!({ "vehicle_id": v, "appointment_id": appt }),
        )
        .await;
    assert_eq!(names(&checklist, "order"), ["Rear rotors"]);
    let todos = names(&checklist, "todos");
    assert_eq!(todos.len(), 1, "got: {checklist}");
    assert!(todos[0].starts_with("Return "), "got: {todos:?}");
}

#[tokio::test]
async fn existing_part_links_without_reentry() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    let job = plan_job(&t, v, "Front brakes", None).await;
    let need = add_need(&t, v, job, "Front pads").await;
    let other = add_need(&t, v, job, "Front rotors").await;
    let part = id(&t
        .call(
            "record_part",
            json!({
                "vehicle_id": v, "name": "FCP front pads", "seller": "FCP Euro",
                "part_number": "1K0698151", "order_ref": "FCP-77"
            }),
        )
        .await);

    let linked = t
        .call(
            "link_part_to_need",
            json!({ "vehicle_id": v, "need_id": need, "part_id": part, "received_on": "2026-11-01" }),
        )
        .await;
    assert_eq!(linked["stage"], "received");
    assert_eq!(linked["part"]["received_on"], "2026-11-01");
    assert_eq!(linked["part"]["order_ref"], "FCP-77");

    // One part serves one need.
    let err = t
        .call_err(
            "link_part_to_need",
            json!({ "vehicle_id": v, "need_id": other, "part_id": part }),
        )
        .await;
    assert!(err.contains("already linked"), "got: {err}");

    // Clearing the arrival date reads as ordered again.
    let ordered = t
        .call(
            "mark_need_received",
            json!({ "vehicle_id": v, "need_id": need, "received": false }),
        )
        .await;
    assert_eq!(ordered["stage"], "ordered");
    assert_eq!(ordered["part"]["received_on"], Value::Null);

    // A null part id unlinks and keeps the part.
    let unlinked = t
        .call(
            "link_part_to_need",
            json!({ "vehicle_id": v, "need_id": need, "part_id": null }),
        )
        .await;
    assert_eq!(unlinked["part_id"], Value::Null);
    let parts = t.call("list_parts", json!({ "vehicle_id": v })).await;
    assert_eq!(parts.as_array().map(Vec::len), Some(1), "got: {parts}");

    // The key is required: leaving it out is not the same as unlinking.
    let err = t
        .call_err(
            "link_part_to_need",
            json!({ "vehicle_id": v, "need_id": need }),
        )
        .await;
    assert!(err.contains("part_id"), "got: {err}");

    // The job reads whole through get_checklist.
    let view = t
        .call(
            "get_checklist",
            json!({ "vehicle_id": v, "work_item_id": job }),
        )
        .await;
    assert_eq!(
        view["needs"].as_array().map(Vec::len),
        Some(2),
        "got: {view}"
    );
}

#[tokio::test]
async fn past_decisions_returns_reason_and_rejected() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    let job = plan_job(&t, v, "Brakes", None).await;
    let need = add_need(&t, v, job, "Rear rotors").await;
    let chosen = add_option(
        &t,
        v,
        need,
        json!({ "description": "310 mm rotor", "manufacturer": "Brembo", "part_number": "08.B413.11" }),
    )
    .await;
    let rejected = add_option(
        &t,
        v,
        need,
        json!({ "description": "Slotted rotor", "manufacturer": "Power Stop" }),
    )
    .await;
    t.call(
        "update_need_option",
        json!({
            "vehicle_id": v, "option_id": rejected,
            "rejected_reason": "Slots wear pads faster on a daily"
        }),
    )
    .await;
    t.call(
        "update_need",
        json!({
            "vehicle_id": v, "need_id": need, "supplied_by": "self",
            "chosen_option_id": chosen, "decision_reason": "OE-quality, quiet"
        }),
    )
    .await;

    let found = t.call("past_decisions", json!({ "query": "rotor" })).await;
    let first = &found[0];
    assert_eq!(first["vehicle_name"], "GTI", "got: {found}");
    assert_eq!(first["need"]["decision_reason"], "OE-quality, quiet");
    assert_eq!(first["chosen"]["manufacturer"], "Brembo");
    assert_eq!(
        first["rejected"][0]["rejected_reason"],
        "Slots wear pads faster on a daily"
    );

    let none = t.call("past_decisions", json!({ "query": "wipers" })).await;
    assert_eq!(none, json!([]));
    let err = t.call_err("past_decisions", json!({ "query": "  " })).await;
    assert!(err.contains("query"), "got: {err}");
}

#[tokio::test]
async fn record_service_with_appointment_reports_unfitted_needs() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    let appt = plan_appointment(&t, v, "Brake day").await;
    let job = plan_job(&t, v, "Rear brakes", Some(appt)).await;
    let need = add_need(&t, v, job, "Rear pads").await;
    let part = id(&t
        .call(
            "record_part",
            json!({ "vehicle_id": v, "name": "Rear pads" }),
        )
        .await);
    t.call(
        "link_part_to_need",
        json!({ "vehicle_id": v, "need_id": need, "part_id": part }),
    )
    .await;

    let filed = t
        .call(
            "record_service",
            json!({
                "vehicle_id": v, "service_date": "2026-11-10", "appointment_id": appt,
                "work_item_ids": [job]
            }),
        )
        .await;
    assert_eq!(filed["appointment_id"], appt);
    assert_eq!(filed["unfitted_needs"][0]["need_id"], need, "got: {filed}");
    assert_eq!(filed["unfitted_needs"][0]["part_id"], part);

    // Naming the appointment installed nothing.
    let parts = t
        .call(
            "list_parts",
            json!({ "vehicle_id": v, "status": "purchased" }),
        )
        .await;
    assert_eq!(parts.as_array().map(Vec::len), Some(1), "got: {parts}");
}

#[tokio::test]
async fn an_inline_part_fills_the_need_it_names() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    let job = plan_job(&t, v, "Plugs", None).await;
    let need = id(&t
        .call(
            "add_need",
            json!({ "vehicle_id": v, "work_item_id": job, "name": "Plugs", "supplied_by": "shop" }),
        )
        .await);

    t.call(
        "record_service",
        json!({
            "vehicle_id": v, "service_date": "2026-11-10", "work_item_ids": [job],
            "parts": [{
                "name": "Plug set", "supplied_by": "shop", "need_id": need, "order_ref": "SHOP-9"
            }]
        }),
    )
    .await;

    let view = t
        .call(
            "get_checklist",
            json!({ "vehicle_id": v, "work_item_id": job }),
        )
        .await;
    assert_eq!(view["needs"][0]["stage"], "installed", "got: {view}");
    assert_eq!(view["needs"][0]["part"]["order_ref"], "SHOP-9");
}

#[tokio::test]
async fn appointment_status_changes_report_the_jobs_they_release() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    let appt = plan_appointment(&t, v, "60k").await;
    let job = plan_job(&t, v, "Brakes", Some(appt)).await;

    let edited = t
        .call(
            "update_appointment",
            json!({
                "vehicle_id": v, "appointment_id": appt, "purpose": "60k service",
                "shop_name": "KW", "planned_date": "2026-12-01"
            }),
        )
        .await;
    assert_eq!(edited["appointment"]["purpose"], "60k service");
    assert_eq!(edited["appointment"]["booked"], true);
    assert_eq!(edited["appointment"]["jobs"], 1);
    assert_eq!(edited["unassigned_job_ids"], json!([]));

    // A null clears the date.
    let cleared = t
        .call(
            "update_appointment",
            json!({ "vehicle_id": v, "appointment_id": appt, "planned_date": null }),
        )
        .await;
    assert_eq!(cleared["appointment"]["booked"], false);

    let cancelled = t
        .call(
            "update_appointment",
            json!({ "vehicle_id": v, "appointment_id": appt, "status": "cancelled" }),
        )
        .await;
    assert_eq!(cancelled["appointment"]["status"], "cancelled");
    assert_eq!(cancelled["unassigned_job_ids"], json!([job]));

    let listed = t
        .call("list_appointments", json!({ "vehicle_id": v }))
        .await;
    assert_eq!(listed, json!([]), "a cancelled appointment is hidden");
    let all = t
        .call(
            "list_appointments",
            json!({ "vehicle_id": v, "include_closed": true }),
        )
        .await;
    assert_eq!(all.as_array().map(Vec::len), Some(1));

    let reopened = t
        .call(
            "update_appointment",
            json!({ "vehicle_id": v, "appointment_id": appt, "status": "planning" }),
        )
        .await;
    assert_eq!(reopened["appointment"]["status"], "planning");

    let err = t
        .call_err(
            "update_appointment",
            json!({ "vehicle_id": v, "appointment_id": appt, "status": "finished" }),
        )
        .await;
    assert!(
        err.contains("finished") && err.contains("planning") && err.contains("cancelled"),
        "got: {err}"
    );

    // Deleting an appointment keeps its jobs.
    let deleted = t
        .call(
            "delete_appointment",
            json!({ "vehicle_id": v, "appointment_id": appt }),
        )
        .await;
    assert_eq!(deleted["deleted"], "appointment");
    let jobs = t
        .call("list_planned_work", json!({ "vehicle_id": v }))
        .await;
    assert_eq!(jobs["items"].as_array().map(Vec::len), Some(1));
}

#[tokio::test]
async fn jobs_move_between_appointments_through_the_planning_tools() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    let a = plan_appointment(&t, v, "A").await;
    let b = plan_appointment(&t, v, "B").await;
    let job = plan_job(&t, v, "Brakes", Some(a)).await;
    assert_eq!(
        t.call(
            "update_planned_work",
            json!({ "vehicle_id": v, "work_item_id": job, "appointment_id": b })
        )
        .await["appointment_id"],
        b
    );
    assert_eq!(
        t.call(
            "update_planned_work",
            json!({ "vehicle_id": v, "work_item_id": job, "appointment_id": null })
        )
        .await["appointment_id"],
        Value::Null
    );
}

#[tokio::test]
async fn get_checklist_rejects_both_ids_and_reads_the_unscheduled_view() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    let job = plan_job(&t, v, "Wipers", None).await;
    add_need(&t, v, job, "Blades").await;
    let appt = plan_appointment(&t, v, "Visit").await;

    let err = t
        .call_err(
            "get_checklist",
            json!({ "vehicle_id": v, "appointment_id": appt, "work_item_id": job }),
        )
        .await;
    assert!(
        err.contains("appointment_id") && err.contains("work_item_id"),
        "got: {err}"
    );

    let unscheduled = t.call("get_checklist", json!({ "vehicle_id": v })).await;
    assert_eq!(names(&unscheduled, "decide"), ["Blades"]);
    assert_eq!(unscheduled["appointment"], Value::Null);
}

#[tokio::test]
async fn todos_tick_untick_move_and_delete() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    let appt = plan_appointment(&t, v, "Visit").await;
    let todo = id(&t
        .call(
            "add_todo",
            json!({ "vehicle_id": v, "title": "Ask about the differential", "appointment_id": appt }),
        )
        .await);

    let ticked = t
        .call(
            "update_todo",
            json!({ "vehicle_id": v, "todo_id": todo, "done": true }),
        )
        .await;
    assert!(ticked["done_on"].is_string(), "got: {ticked}");
    let unticked = t
        .call(
            "update_todo",
            json!({ "vehicle_id": v, "todo_id": todo, "done": false, "notes": "KW said Monday" }),
        )
        .await;
    assert_eq!(unticked["done_on"], Value::Null);
    assert_eq!(unticked["notes"], "KW said Monday");
    let moved = t
        .call(
            "update_todo",
            json!({ "vehicle_id": v, "todo_id": todo, "appointment_id": null }),
        )
        .await;
    assert_eq!(moved["appointment_id"], Value::Null);

    let deleted = t
        .call("delete_todo", json!({ "vehicle_id": v, "todo_id": todo }))
        .await;
    assert_eq!(deleted["deleted"], "to-do");
}

#[tokio::test]
async fn options_and_needs_can_be_edited_and_deleted() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    let job = plan_job(&t, v, "Brakes", None).await;
    let need = add_need(&t, v, job, "Pads").await;
    let option = add_option(&t, v, need, json!({ "description": "Pads" })).await;

    let edited = t
        .call(
            "update_need_option",
            json!({
                "vehicle_id": v, "option_id": option, "fitment": "confirmed",
                "price_cents": 9_000, "seller": "FCP Euro"
            }),
        )
        .await;
    assert_eq!(edited["fitment"], "confirmed");
    let cleared = t
        .call(
            "update_need_option",
            json!({ "vehicle_id": v, "option_id": option, "seller": null }),
        )
        .await;
    assert_eq!(cleared["seller"], Value::Null);
    assert_eq!(cleared["price_cents"], 9_000);

    t.call(
        "update_need",
        json!({ "vehicle_id": v, "need_id": need, "chosen_option_id": option }),
    )
    .await;
    let cleared_choice = t
        .call(
            "update_need",
            json!({ "vehicle_id": v, "need_id": need, "chosen_option_id": null }),
        )
        .await;
    assert_eq!(cleared_choice["chosen_option_id"], Value::Null);

    assert_eq!(
        t.call(
            "delete_need_option",
            json!({ "vehicle_id": v, "option_id": option })
        )
        .await["deleted"],
        "option"
    );
    assert_eq!(
        t.call("delete_need", json!({ "vehicle_id": v, "need_id": need }))
            .await["deleted"],
        "need"
    );
}

#[tokio::test]
async fn a_returned_part_is_not_a_settable_status() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    let part = id(&t
        .call("record_part", json!({ "vehicle_id": v, "name": "Pads" }))
        .await);
    let err = t
        .call_err(
            "record_part",
            json!({ "vehicle_id": v, "name": "Rotors", "status": "returned" }),
        )
        .await;
    assert!(err.contains("return_part"), "got: {err}");
    let err = t
        .call_err(
            "correct_part",
            json!({ "vehicle_id": v, "part_id": part, "status": "returned" }),
        )
        .await;
    assert!(err.contains("return_part"), "got: {err}");

    // The order number and arrival date are editable on a part with no need.
    let corrected = t
        .call(
            "correct_part",
            json!({ "vehicle_id": v, "part_id": part, "order_ref": "A-1", "received_on": "2026-11-02" }),
        )
        .await;
    assert_eq!(corrected["order_ref"], "A-1");
    assert_eq!(corrected["received_on"], "2026-11-02");
}

#[tokio::test]
async fn unknown_field_is_a_tool_error_naming_it() {
    let t = connect().await;
    let cases = [
        (
            "plan_appointment",
            json!({ "vehicle_id": 1, "purpose": "x" }),
        ),
        (
            "update_appointment",
            json!({ "vehicle_id": 1, "appointment_id": 1 }),
        ),
        ("list_appointments", json!({ "vehicle_id": 1 })),
        (
            "delete_appointment",
            json!({ "vehicle_id": 1, "appointment_id": 1 }),
        ),
        ("get_checklist", json!({ "vehicle_id": 1 })),
        (
            "add_need",
            json!({ "vehicle_id": 1, "work_item_id": 1, "name": "x" }),
        ),
        ("update_need", json!({ "vehicle_id": 1, "need_id": 1 })),
        ("delete_need", json!({ "vehicle_id": 1, "need_id": 1 })),
        (
            "add_need_option",
            json!({ "vehicle_id": 1, "need_id": 1, "description": "x" }),
        ),
        (
            "update_need_option",
            json!({ "vehicle_id": 1, "option_id": 1 }),
        ),
        (
            "delete_need_option",
            json!({ "vehicle_id": 1, "option_id": 1 }),
        ),
        (
            "mark_need_ordered",
            json!({ "vehicle_id": 1, "need_id": 1 }),
        ),
        (
            "link_part_to_need",
            json!({ "vehicle_id": 1, "need_id": 1, "part_id": null }),
        ),
        (
            "mark_need_received",
            json!({ "vehicle_id": 1, "need_id": 1 }),
        ),
        (
            "return_part",
            json!({ "vehicle_id": 1, "need_id": 1, "reason": "x" }),
        ),
        ("add_todo", json!({ "vehicle_id": 1, "title": "x" })),
        ("update_todo", json!({ "vehicle_id": 1, "todo_id": 1 })),
        ("delete_todo", json!({ "vehicle_id": 1, "todo_id": 1 })),
        ("past_decisions", json!({ "query": "x" })),
        (
            "record_service",
            json!({ "vehicle_id": 1, "service_date": "2026-11-10", "parts": [{ "name": "x" }] }),
        ),
    ];
    for (tool, args) in cases {
        let mut args = args;
        args["bogus"] = json!(true);
        assert_unknown_field_rejected(&t, tool, args, "bogus").await;
    }

    // A nested inline part is checked too.
    assert_unknown_field_rejected(
        &t,
        "record_service",
        json!({
            "vehicle_id": 1, "service_date": "2026-11-10",
            "parts": [{ "name": "x", "bogus": true }]
        }),
        "bogus",
    )
    .await;
}

#[tokio::test]
async fn wrong_vehicle_ids_read_as_missing() {
    let t = connect().await;
    let a = add_vehicle(&t, "Mine").await;
    let b = add_vehicle(&t, "Theirs").await;
    let appt = plan_appointment(&t, a, "Visit").await;
    let job = plan_job(&t, a, "Brakes", Some(appt)).await;
    let need = add_need(&t, a, job, "Pads").await;
    let option = add_option(&t, a, need, json!({ "description": "Pads" })).await;
    let todo = id(&t
        .call("add_todo", json!({ "vehicle_id": a, "title": "Call" }))
        .await);
    let part = id(&t
        .call("record_part", json!({ "vehicle_id": a, "name": "Pads" }))
        .await);
    // A need on the other vehicle, to try a part that is not its own.
    let their_job = plan_job(&t, b, "Brakes", None).await;
    let their_need = add_need(&t, b, their_job, "Pads").await;

    // Each case is a tool, its arguments with `ID` where the foreign id goes,
    // and the real id that belongs to the other vehicle.
    let cases: Vec<(&str, Value, i64)> = vec![
        (
            "update_appointment",
            json!({ "vehicle_id": b, "appointment_id": "ID", "purpose": "x" }),
            appt,
        ),
        (
            "delete_appointment",
            json!({ "vehicle_id": b, "appointment_id": "ID" }),
            appt,
        ),
        (
            "get_checklist",
            json!({ "vehicle_id": b, "appointment_id": "ID" }),
            appt,
        ),
        (
            "get_checklist",
            json!({ "vehicle_id": b, "work_item_id": "ID" }),
            job,
        ),
        (
            "plan_work",
            json!({ "vehicle_id": b, "title": "x", "appointment_id": "ID" }),
            appt,
        ),
        (
            "record_service",
            json!({ "vehicle_id": b, "service_date": "2026-11-10", "appointment_id": "ID" }),
            appt,
        ),
        (
            "add_need",
            json!({ "vehicle_id": b, "work_item_id": "ID", "name": "x" }),
            job,
        ),
        (
            "update_need",
            json!({ "vehicle_id": b, "need_id": "ID", "name": "x" }),
            need,
        ),
        (
            "delete_need",
            json!({ "vehicle_id": b, "need_id": "ID" }),
            need,
        ),
        (
            "add_need_option",
            json!({ "vehicle_id": b, "need_id": "ID", "description": "x" }),
            need,
        ),
        (
            "update_need_option",
            json!({ "vehicle_id": b, "option_id": "ID", "fitment": "confirmed" }),
            option,
        ),
        (
            "delete_need_option",
            json!({ "vehicle_id": b, "option_id": "ID" }),
            option,
        ),
        (
            "mark_need_ordered",
            json!({ "vehicle_id": b, "need_id": "ID" }),
            need,
        ),
        (
            "mark_need_received",
            json!({ "vehicle_id": b, "need_id": "ID" }),
            need,
        ),
        (
            "return_part",
            json!({ "vehicle_id": b, "need_id": "ID", "reason": "x" }),
            need,
        ),
        (
            "link_part_to_need",
            json!({ "vehicle_id": b, "need_id": "ID", "part_id": null }),
            need,
        ),
        (
            "link_part_to_need",
            json!({ "vehicle_id": b, "need_id": their_need, "part_id": "ID" }),
            part,
        ),
        (
            "update_todo",
            json!({ "vehicle_id": b, "todo_id": "ID", "done": true }),
            todo,
        ),
        (
            "delete_todo",
            json!({ "vehicle_id": b, "todo_id": "ID" }),
            todo,
        ),
        (
            "add_todo",
            json!({ "vehicle_id": b, "title": "x", "appointment_id": "ID" }),
            appt,
        ),
    ];
    for (tool, args, real) in cases {
        let with = |value: i64| -> Value {
            serde_json::from_str(&args.to_string().replace("\"ID\"", &value.to_string())).unwrap()
        };
        let missing = t.call_err(tool, with(99_999)).await;
        let wrong = t.call_err(tool, with(real)).await;
        assert!(wrong.contains("not found"), "{tool}: {wrong}");
        assert_eq!(
            wrong.replace(&real.to_string(), "N"),
            missing.replace("99999", "N"),
            "{tool} must not tell a wrong-vehicle id from a missing one"
        );
    }
}
