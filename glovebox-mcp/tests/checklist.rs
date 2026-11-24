// The job-checklist tools: appointments, needs, options, purchases, to-dos and
// past decisions, driven through tool calls.

mod common;

use common::{TestClient, add_vehicle, assert_unknown_field_rejected, connect, id};
use serde_json::{Value, json};

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

// The names of the rows in one next-steps group of a checklist.
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
#[expect(
    clippy::too_many_lines,
    reason = "one appointment told start to finish"
)]
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
    let rotor_row = &checklist["jobs"][0]["needs"][0]["need"];
    assert_eq!(rotor_row["name"], "Rear rotors", "got: {checklist}");
    assert_eq!(
        rotor_row["supplied_by"],
        Value::Null,
        "the refused call applied nothing"
    );
    assert_eq!(rotor_row["chosen_option_id"], Value::Null);

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
async fn the_calls_return_part_names_undo_a_return() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    let job = plan_job(&t, v, "Rear brakes", None).await;
    let rotors = add_need(&t, v, job, "Rear rotors").await;
    let option = add_option(&t, v, rotors, json!({ "description": "310 mm rotor" })).await;
    t.call(
        "update_need",
        json!({
            "vehicle_id": v, "need_id": rotors, "supplied_by": "self",
            "chosen_option_id": option, "decision_reason": "Matches the brakes"
        }),
    )
    .await;
    let ordered = t
        .call(
            "mark_need_ordered",
            json!({ "vehicle_id": v, "need_id": rotors, "purchase_date": "2026-10-30" }),
        )
        .await;
    let part = ordered["part"]["id"].as_i64().unwrap();
    let returned = t
        .call(
            "return_part",
            json!({
                "vehicle_id": v, "need_id": rotors, "reason": "Thought it was wrong",
                "option_was_wrong": true
            }),
        )
        .await;
    let todo = returned["todo"]["id"].as_i64().unwrap();

    t.call(
        "correct_part",
        json!({ "vehicle_id": v, "part_id": part, "status": "purchased" }),
    )
    .await;
    t.call(
        "link_part_to_need",
        json!({ "vehicle_id": v, "need_id": rotors, "part_id": part }),
    )
    .await;
    t.call("delete_todo", json!({ "vehicle_id": v, "todo_id": todo }))
        .await;
    t.call(
        "update_need_option",
        json!({ "vehicle_id": v, "option_id": option, "rejected_reason": null }),
    )
    .await;
    t.call(
        "update_need",
        json!({
            "vehicle_id": v, "need_id": rotors, "chosen_option_id": option,
            "decision_reason": "Matches the brakes"
        }),
    )
    .await;

    let checklist = t.call("get_checklist", json!({ "vehicle_id": v })).await;
    assert_eq!(
        names(&checklist, "arriving"),
        ["Rear rotors"],
        "got: {checklist}"
    );
    assert!(names(&checklist, "todos").is_empty(), "got: {checklist}");
    let row = &checklist["jobs"][0]["needs"][0];
    assert_eq!(row["need"]["chosen_option_id"], option);
    assert_eq!(row["need"]["decision_reason"], "Matches the brakes");
    assert_eq!(row["chosen"]["rejected_reason"], Value::Null);
    // Choosing the option again clears the history its rejection kept.
    assert_eq!(row["chosen"]["chosen_reason"], Value::Null, "got: {row}");
    assert_eq!(row["chosen"]["chosen_on"], Value::Null, "got: {row}");
}

#[tokio::test]
async fn a_reason_with_no_chosen_option_is_kept_only_for_a_shop_need() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    let job = plan_job(&t, v, "Spark plugs", None).await;
    let need = add_need(&t, v, job, "Spark plugs").await;

    let err = t
        .call_err(
            "update_need",
            json!({ "vehicle_id": v, "need_id": need, "decision_reason": "OEM only" }),
        )
        .await;
    assert!(
        err.contains("decision_reason: needs a chosen option unless the shop supplies the need"),
        "got: {err}"
    );

    let shop = t
        .call(
            "update_need",
            json!({
                "vehicle_id": v, "need_id": need, "supplied_by": "shop",
                "decision_reason": "OEM only"
            }),
        )
        .await;
    assert_eq!(shop["decision_reason"], "OEM only");

    // A shop need takes a reason together with a cleared choice.
    let cleared = t
        .call(
            "update_need",
            json!({
                "vehicle_id": v, "need_id": need, "chosen_option_id": null,
                "decision_reason": "The shop stocks OEM plugs"
            }),
        )
        .await;
    assert_eq!(cleared["decision_reason"], "The shop stocks OEM plugs");

    // Clearing a choice the need does not have leaves its reason alone.
    let unchanged = t
        .call(
            "update_need",
            json!({ "vehicle_id": v, "need_id": need, "chosen_option_id": null }),
        )
        .await;
    assert_eq!(unchanged["decision_reason"], "The shop stocks OEM plugs");

    // Moving it to the owner with no chosen option clears the reason.
    let moved = t
        .call(
            "update_need",
            json!({ "vehicle_id": v, "need_id": need, "supplied_by": "self" }),
        )
        .await;
    assert_eq!(moved["decision_reason"], Value::Null, "got: {moved}");
}

#[tokio::test]
async fn a_replaced_part_reads_installed_and_cannot_be_returned_or_redated() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    let job = plan_job(&t, v, "Rear brakes", None).await;
    let rotors = add_need(&t, v, job, "Rear rotors").await;
    let option = add_option(&t, v, rotors, json!({ "description": "310 mm rotor" })).await;
    t.call(
        "update_need",
        json!({
            "vehicle_id": v, "need_id": rotors, "supplied_by": "self",
            "chosen_option_id": option
        }),
    )
    .await;
    let ordered = t
        .call(
            "mark_need_ordered",
            json!({ "vehicle_id": v, "need_id": rotors, "purchase_date": "2026-10-30" }),
        )
        .await;
    let part = ordered["part"]["id"].as_i64().unwrap();
    t.call(
        "mark_need_received",
        json!({ "vehicle_id": v, "need_id": rotors, "received_on": "2026-11-01" }),
    )
    .await;
    t.call(
        "correct_part",
        json!({ "vehicle_id": v, "part_id": part, "status": "replaced" }),
    )
    .await;

    let checklist = t.call("get_checklist", json!({ "vehicle_id": v })).await;
    assert_eq!(
        checklist["jobs"][0]["needs"][0]["stage"], "installed",
        "got: {checklist}"
    );
    let err = t
        .call_err(
            "return_part",
            json!({ "vehicle_id": v, "need_id": rotors, "reason": "Wrong size" }),
        )
        .await;
    assert_eq!(
        err,
        format!("Part {part} is replaced; only an ordered or received part can be returned")
    );

    // The arrival date of a part on the car stays as it is.
    let err = t
        .call_err(
            "mark_need_received",
            json!({ "vehicle_id": v, "need_id": rotors, "received_on": "2026-11-03" }),
        )
        .await;
    assert_eq!(
        err,
        format!("Part {part} is replaced; only an ordered or received part can be marked received")
    );
    let err = t
        .call_err(
            "mark_need_received",
            json!({ "vehicle_id": v, "need_id": rotors, "received": false }),
        )
        .await;
    assert_eq!(
        err,
        format!(
            "Part {part} is replaced; only an ordered or received part can have its arrival date \
             cleared"
        )
    );
    let err = t
        .call_err(
            "link_part_to_need",
            json!({
                "vehicle_id": v, "need_id": rotors, "part_id": part, "received_on": "2026-11-03"
            }),
        )
        .await;
    assert_eq!(
        err,
        format!("Part {part} is replaced; only an ordered or received part can be marked received")
    );
    let parts = t.call("list_parts", json!({ "vehicle_id": v })).await;
    assert_eq!(parts[0]["received_on"], "2026-11-01", "got: {parts}");
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one part followed through every tool that names it"
)]
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

    // A second arrival with no date keeps the stored one; a named date
    // replaces it.
    let err = t
        .call_err(
            "mark_need_received",
            json!({ "vehicle_id": v, "need_id": need }),
        )
        .await;
    assert_eq!(
        err,
        format!("Need {need}'s part was already received on 2026-11-01")
    );
    let redated = t
        .call(
            "mark_need_received",
            json!({ "vehicle_id": v, "need_id": need, "received_on": "2026-11-02" }),
        )
        .await;
    assert_eq!(redated["part"]["received_on"], "2026-11-02");

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

    // A date has no meaning when unlinking.
    let err = t
        .call_err(
            "link_part_to_need",
            json!({ "vehicle_id": v, "need_id": need, "part_id": null, "received_on": "2026-11-02" }),
        )
        .await;
    assert_eq!(err, "received_on: cannot be given when unlinking a part");

    // The key is required: leaving it out is not the same as unlinking.
    let err = t
        .call_err(
            "link_part_to_need",
            json!({ "vehicle_id": v, "need_id": need }),
        )
        .await;
    assert_eq!(
        err,
        "part_id: is required: name a part to link, or unlink the need's current part"
    );

    // A date has no meaning when clearing the arrival date.
    let err = t
        .call_err(
            "mark_need_received",
            json!({
                "vehicle_id": v, "need_id": need, "received": false, "received_on": "2026-11-02"
            }),
        )
        .await;
    assert_eq!(
        err,
        "received_on: cannot be given when clearing the arrival date"
    );

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
async fn a_fitted_part_goes_back_to_purchased_only_when_the_owner_holds_it() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    // The job stays planned (the service does not name it), so each need's
    // stage is read on an open job.
    let job = plan_job(&t, v, "Plugs and pads", None).await;
    let mut needs = Vec::new();
    for (name, supplier) in [("Plugs", "shop"), ("Pads", "self")] {
        needs.push(id(&t
            .call(
                "add_need",
                json!({
                    "vehicle_id": v, "work_item_id": job, "name": name, "supplied_by": supplier
                }),
            )
            .await));
    }
    t.call(
        "record_service",
        json!({
            "vehicle_id": v, "service_date": "2026-11-10",
            "parts": [
                { "name": "Plug set", "supplied_by": "shop", "need_id": needs[0] },
                { "name": "Pad set", "supplied_by": "self", "need_id": needs[1] }
            ]
        }),
    )
    .await;
    let view = t
        .call(
            "get_checklist",
            json!({ "vehicle_id": v, "work_item_id": job }),
        )
        .await;
    let part = |i: usize| view["needs"][i]["part"]["id"].clone();
    let (plugs, pads) = (part(0), part(1));

    let err = t
        .call_err(
            "correct_part",
            json!({ "vehicle_id": v, "part_id": plugs, "status": "purchased" }),
        )
        .await;
    assert!(err.contains("supplied by the shop"), "got: {err}");

    t.call(
        "correct_part",
        json!({ "vehicle_id": v, "part_id": pads, "status": "purchased" }),
    )
    .await;
    let view = t
        .call(
            "get_checklist",
            json!({ "vehicle_id": v, "work_item_id": job }),
        )
        .await;
    assert_eq!(view["needs"][0]["stage"], "installed", "got: {view}");
    assert_eq!(view["needs"][1]["stage"], "received", "got: {view}");
}

#[tokio::test]
async fn appointment_status_changes_report_the_jobs_they_release() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    let appt = plan_appointment(&t, v, "60k").await;
    let job = plan_job(&t, v, "Brakes", Some(appt)).await;
    let kw = id(&t.call("add_shop", json!({ "name": "KW Automotive" })).await);

    let err = t
        .call_err(
            "update_appointment",
            json!({ "vehicle_id": v, "appointment_id": appt, "shop_id": kw, "shop_name": "KW" }),
        )
        .await;
    assert_eq!(err, "Name the shop by shop_id or by shop_name, not both");
    let err = t
        .call_err(
            "update_appointment",
            json!({ "vehicle_id": v, "appointment_id": appt, "shop_name": "KW" }),
        )
        .await;
    assert!(
        err.starts_with("No shop in the directory is named KW"),
        "{err}"
    );

    let edited = t
        .call(
            "update_appointment",
            json!({
                "vehicle_id": v, "appointment_id": appt, "purpose": "60k service",
                "shop_name": "kw automotive", "planned_date": "2026-12-01"
            }),
        )
        .await;
    assert_eq!(edited["appointment"]["shop_id"], kw);
    assert_eq!(edited["appointment"]["shop_name"], "KW Automotive");
    assert_eq!(edited["appointment"]["purpose"], "60k service");
    assert_eq!(edited["appointment"]["planned_date"], "2026-12-01");
    assert_eq!(edited["appointment"]["jobs"], 1);
    assert_eq!(edited["unassigned_job_ids"], json!([]));

    // A null clears the date.
    let cleared = t
        .call(
            "update_appointment",
            json!({ "vehicle_id": v, "appointment_id": appt, "planned_date": null }),
        )
        .await;
    assert_eq!(cleared["appointment"]["planned_date"], Value::Null);
    assert_eq!(cleared["appointment"]["phase"], "planning");

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
    assert_eq!(
        err,
        "appointment_id: pass either appointment_id or work_item_id, not both"
    );

    let unscheduled = t.call("get_checklist", json!({ "vehicle_id": v })).await;
    assert_eq!(names(&unscheduled, "decide"), ["Blades"]);
    assert_eq!(unscheduled["appointment"], Value::Null);
}

#[tokio::test]
async fn a_closed_or_cancelled_appointment_takes_no_new_todos_until_reopened() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    let closed = plan_appointment(&t, v, "Brakes").await;
    let cancelled = plan_appointment(&t, v, "Tires").await;
    for (appt, status) in [(closed, "done"), (cancelled, "cancelled")] {
        t.call(
            "update_appointment",
            json!({ "vehicle_id": v, "appointment_id": appt, "status": status }),
        )
        .await;
    }
    let loose = id(&t
        .call("add_todo", json!({ "vehicle_id": v, "title": "Loose" }))
        .await);

    for (appt, ending) in [(closed, "closed"), (cancelled, "cancelled")] {
        let expected = format!("Appointment {appt} is {ending}, so it takes no new work");
        let err = t
            .call_err(
                "add_todo",
                json!({ "vehicle_id": v, "title": "Late", "appointment_id": appt }),
            )
            .await;
        assert!(err.contains(&expected), "got: {err}");
        let err = t
            .call_err(
                "update_todo",
                json!({ "vehicle_id": v, "todo_id": loose, "appointment_id": appt }),
            )
            .await;
        assert!(err.contains(&expected), "got: {err}");
    }

    t.call(
        "update_appointment",
        json!({ "vehicle_id": v, "appointment_id": closed, "status": "planning" }),
    )
    .await;
    let added = t
        .call(
            "add_todo",
            json!({ "vehicle_id": v, "title": "Late", "appointment_id": closed }),
        )
        .await;
    assert_eq!(added["appointment_id"], closed);
}

#[tokio::test]
async fn closing_waits_for_open_work_and_un_ticking_reopens_the_appointment() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    let appt = plan_appointment(&t, v, "Brakes").await;
    let todo = id(&t
        .call(
            "add_todo",
            json!({ "vehicle_id": v, "title": "Bring the key", "appointment_id": appt }),
        )
        .await);
    let close = json!({ "vehicle_id": v, "appointment_id": appt, "status": "done" });

    let err = t.call_err("update_appointment", close.clone()).await;
    assert!(
        err.contains(&format!("to-do {todo} \"Bring the key\"")),
        "got: {err}"
    );
    t.call(
        "update_todo",
        json!({ "vehicle_id": v, "todo_id": todo, "done": true }),
    )
    .await;
    let closed = t.call("update_appointment", close).await;
    assert_eq!(closed["appointment"]["status"], "done");

    let back = t
        .call(
            "update_todo",
            json!({ "vehicle_id": v, "todo_id": todo, "done": false }),
        )
        .await;
    assert_eq!(back["appointment_id"], appt);
    assert_eq!(back["reopened_appointment_id"], appt);
    assert!(back.get("left_appointment_id").is_none(), "got: {back}");
    let listed = t
        .call("list_appointments", json!({ "vehicle_id": v }))
        .await;
    assert_eq!(listed[0]["status"], "planning", "got: {listed}");

    let cancelled = t
        .call(
            "update_appointment",
            json!({ "vehicle_id": v, "appointment_id": appt, "status": "cancelled" }),
        )
        .await;
    assert_eq!(cancelled["unassigned_todo_ids"], json!([todo]));
}

#[tokio::test]
async fn an_appointment_reads_its_phase_from_its_date() {
    let t = connect().await;
    let v = add_vehicle(&t, "GTI").await;
    for (purpose, planned_date) in [
        ("Past", json!("2000-01-01")),
        ("Future", json!("2099-01-01")),
        ("Undated", Value::Null),
    ] {
        t.call(
            "plan_appointment",
            json!({ "vehicle_id": v, "purpose": purpose, "planned_date": planned_date }),
        )
        .await;
    }

    let listed = t
        .call("list_appointments", json!({ "vehicle_id": v }))
        .await;
    let phases: Vec<(&str, &str, &str)> = listed
        .as_array()
        .expect("a list")
        .iter()
        .map(|a| {
            (
                a["purpose"].as_str().unwrap(),
                a["status"].as_str().unwrap(),
                a["phase"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        phases,
        [
            ("Past", "planning", "in_shop"),
            ("Future", "planning", "booked"),
            ("Undated", "planning", "planning"),
        ]
    );
    let past = listed[0]["id"].as_i64().unwrap();
    let sheet = t
        .call(
            "get_checklist",
            json!({ "vehicle_id": v, "appointment_id": past }),
        )
        .await;
    assert_eq!(sheet["appointment"]["phase"], "in_shop", "got: {sheet}");
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
    // Ticking a to-do that is already done keeps its first date.
    let again = t
        .call(
            "update_todo",
            json!({ "vehicle_id": v, "todo_id": todo, "done": true }),
        )
        .await;
    assert_eq!(again["done_on"], ticked["done_on"]);
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

    // A reason cannot be stored while the choice is being cleared.
    let err = t
        .call_err(
            "update_need",
            json!({ "vehicle_id": v, "need_id": need, "chosen_option_id": null, "decision_reason": "why" }),
        )
        .await;
    assert!(
        err.contains("decision_reason: needs a chosen option unless the shop supplies the need"),
        "got: {err}"
    );

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

    // A value outside a list is refused with the argument's name.
    for (tool, args, argument, listed) in [
        (
            "record_part",
            json!({ "vehicle_id": v, "name": "Rotors", "status": "lost" }),
            "status",
            "`purchased`",
        ),
        (
            "correct_part",
            json!({ "vehicle_id": v, "part_id": part, "supplied_by": "dealer" }),
            "supplied_by",
            "`self`",
        ),
        (
            "list_parts",
            json!({ "vehicle_id": v, "status": "lost" }),
            "status",
            "`returned`",
        ),
        (
            "update_planned_work",
            json!({ "vehicle_id": v, "work_item_id": 1, "status": "scheduled" }),
            "status",
            "`dropped`",
        ),
        (
            "add_need",
            json!({ "vehicle_id": v, "work_item_id": 1, "name": "Pads", "approach": "rebuild" }),
            "approach",
            "`upgrade`",
        ),
        (
            "add_need_option",
            json!({ "vehicle_id": v, "need_id": 1, "description": "Pads", "fitment": "maybe" }),
            "fitment",
            "`does_not_fit`",
        ),
        // An argument that also takes `null`.
        (
            "update_need",
            json!({ "vehicle_id": v, "need_id": 1, "supplied_by": "dealer" }),
            "supplied_by",
            "`shop`",
        ),
        // An argument inside a list.
        (
            "record_service",
            json!({
                "vehicle_id": v,
                "service_date": "2026-11-01",
                "parts": [{ "name": "Pads", "supplied_by": "dealer" }],
            }),
            "parts[0].supplied_by",
            "`shop`",
        ),
    ] {
        let err = t.call_err(tool, args).await;
        assert!(
            err.contains(&format!("{argument}: unknown variant")) && err.contains(listed),
            "{tool}: {err}"
        );
    }

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
            "get_work_sheet",
            json!({ "vehicle_id": 1, "appointment_id": 1 }),
        ),
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
    // So is a kit item on an order.
    assert_unknown_field_rejected(
        &t,
        "mark_need_ordered",
        json!({ "vehicle_id": 1, "need_id": 1, "contents": [{ "name": "x", "bogus": true }] }),
        "bogus",
    )
    .await;
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one wrong-vehicle case per checklist tool"
)]
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
    let their_service = id(&t
        .call(
            "record_service",
            json!({ "vehicle_id": b, "service_date": "2026-11-01" }),
        )
        .await);
    let their_todo = id(&t
        .call("add_todo", json!({ "vehicle_id": b, "title": "Call" }))
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
        (
            "correct_service",
            json!({ "vehicle_id": b, "service_id": their_service, "appointment_id": "ID" }),
            appt,
        ),
        (
            "update_planned_work",
            json!({ "vehicle_id": b, "work_item_id": their_job, "appointment_id": "ID" }),
            appt,
        ),
        (
            "update_todo",
            json!({ "vehicle_id": b, "todo_id": their_todo, "appointment_id": "ID" }),
            appt,
        ),
        (
            "record_service",
            json!({ "vehicle_id": b, "service_date": "2026-11-10", "parts": [{ "name": "x", "need_id": "ID" }] }),
            need,
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

// An appointment with one job: "Front pads" chosen and ordered by the owner
// (Akebono ACT1633), "Brake fluid" supplied by the shop, and "Wiper blades"
// undecided. Returns (vehicle, appointment).
async fn work_sheet_fixture(t: &TestClient) -> (i64, i64) {
    let v = add_vehicle(t, "Golf GTI").await;
    let appt = plan_appointment(t, v, "Brakes").await;
    let job = plan_job(t, v, "Front brakes", Some(appt)).await;
    let pads = add_need(t, v, job, "Front pads").await;
    let option = add_option(
        t,
        v,
        pads,
        json!({ "description": "Ceramic pads", "manufacturer": "Akebono", "part_number": "ACT1633" }),
    )
    .await;
    t.call(
        "update_need",
        json!({ "vehicle_id": v, "need_id": pads, "supplied_by": "self", "chosen_option_id": option }),
    )
    .await;
    t.call(
        "mark_need_ordered",
        json!({ "vehicle_id": v, "need_id": pads }),
    )
    .await;
    let fluid = add_need(t, v, job, "Brake fluid").await;
    t.call(
        "update_need",
        json!({ "vehicle_id": v, "need_id": fluid, "supplied_by": "shop" }),
    )
    .await;
    add_need(t, v, job, "Wiper blades").await;
    (v, appt)
}

#[tokio::test]
async fn work_sheet_groups_parts_by_supplier() {
    let t = connect().await;
    let (v, appt) = work_sheet_fixture(&t).await;

    let sheet = t
        .call(
            "get_work_sheet",
            json!({ "vehicle_id": v, "appointment_id": appt }),
        )
        .await;

    let job = &sheet["jobs"][0];
    assert_eq!(job["title"], "Front brakes", "got: {sheet}");
    // An owner line names the part that was bought, not the need.
    assert_eq!(job["owner_supplies"][0]["name"], "Ceramic pads");
    assert_eq!(job["owner_supplies"][0]["label"], "Akebono ACT1633");
    assert_eq!(job["owner_supplies"][0]["contents"], json!([]));
    assert_eq!(job["shop_supplies"][0]["name"], "Brake fluid");
    assert_eq!(job["shop_supplies"][0]["label"], Value::Null);
    assert_eq!(sheet["vehicle"], "Golf GTI");
}

#[tokio::test]
async fn work_sheet_carries_job_and_part_notes() {
    let t = connect().await;
    let (v, appt) = work_sheet_fixture(&t).await;
    let parts = t.call("list_parts", json!({ "vehicle_id": v })).await;
    t.call(
        "correct_part",
        json!({ "vehicle_id": v, "part_id": parts[0]["id"], "notes": "Torque to 20 Nm." }),
    )
    .await;
    let sheet = t
        .call(
            "get_work_sheet",
            json!({ "vehicle_id": v, "appointment_id": appt }),
        )
        .await;
    let job_id = sheet["jobs"][0]["job_id"].clone();
    t.call(
        "update_planned_work",
        json!({ "vehicle_id": v, "work_item_id": job_id, "notes": "Ask about the sensor." }),
    )
    .await;

    let sheet = t
        .call(
            "get_work_sheet",
            json!({ "vehicle_id": v, "appointment_id": appt }),
        )
        .await;

    let job = &sheet["jobs"][0];
    assert_eq!(job["notes"], "Ask about the sensor.", "got: {sheet}");
    assert_eq!(job["owner_supplies"][0]["notes"], "Torque to 20 Nm.");
    assert_eq!(job["shop_supplies"][0]["notes"], Value::Null);
}

#[tokio::test]
async fn work_sheet_names_the_car_and_the_symptom() {
    use glovebox_shared::{inputs::vehicle::NewVehicle, services::vehicle};
    let t = connect().await;
    let car = vehicle::create(
        &t.db,
        NewVehicle {
            name: "Greta".into(),
            year: Some(2017),
            make: Some("Volkswagen".into()),
            model: Some("Golf GTI".into()),
            vin: Some("3VW447AU9HM012345".into()),
            license_plate: Some("ABC 1234".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let v = i64::from(car.id);
    t.call(
        "log_mileage",
        json!({ "vehicle_id": v, "mileage": 61_000, "recorded_at": "2026-11-01" }),
    )
    .await;
    let appt = plan_appointment(&t, v, "Diagnosis").await;
    let incident = t
        .call(
            "log_incident",
            json!({
                "vehicle_id": v,
                "title": "Rattle from the front left",
                "description": "Over bumps below 20 mph."
            }),
        )
        .await["id"]
        .clone();
    t.call(
        "plan_work",
        json!({
            "vehicle_id": v,
            "title": "Diagnose the rattle",
            "incident_id": incident,
            "appointment_id": appt
        }),
    )
    .await;
    plan_job(&t, v, "Oil change", Some(appt)).await;

    let sheet = t
        .call(
            "get_work_sheet",
            json!({ "vehicle_id": v, "appointment_id": appt }),
        )
        .await;

    assert_eq!(
        sheet["vehicle_description"], "2017 Volkswagen Golf GTI",
        "got: {sheet}"
    );
    assert_eq!(sheet["vin"], "3VW447AU9HM012345");
    assert_eq!(sheet["license_plate"], "ABC 1234");
    assert_eq!(sheet["odometer"], 61_000);
    assert_eq!(sheet["odometer_read_on"], "2026-11-01");
    let filed = &sheet["jobs"][0];
    assert_eq!(filed["incident_id"], incident);
    assert_eq!(filed["symptom"], "Rattle from the front left");
    assert_eq!(filed["symptom_description"], "Over bumps below 20 mph.");
    let plain = &sheet["jobs"][1];
    assert_eq!(plain["incident_id"], Value::Null);
    assert_eq!(plain["symptom"], Value::Null);
    assert_eq!(plain["symptom_description"], Value::Null);
}

// A vehicle recorded with a name only has none of the car's details, and each
// key is present and null.
#[tokio::test]
async fn work_sheet_of_a_bare_vehicle_has_null_car_details() {
    let t = connect().await;
    let (v, appt) = work_sheet_fixture(&t).await;

    let sheet = t
        .call(
            "get_work_sheet",
            json!({ "vehicle_id": v, "appointment_id": appt }),
        )
        .await;

    let details = sheet.as_object().unwrap();
    for key in [
        "vehicle_description",
        "vin",
        "license_plate",
        "odometer",
        "odometer_read_on",
    ] {
        assert_eq!(details.get(key), Some(&Value::Null), "{key}: {sheet}");
    }
}

#[tokio::test]
async fn work_sheet_lists_a_kits_contents() {
    let t = connect().await;
    let (v, appt) = work_sheet_fixture(&t).await;
    let parts = t.call("list_parts", json!({ "vehicle_id": v })).await;
    let pads = parts[0]["id"].clone();
    let kit = t.call(
        "correct_part",
        json!({
            "vehicle_id": v,
            "part_id": pads,
            "add_contents": [
                { "name": "Pads", "manufacturer": "Akebono", "part_number": "ACT1633A", "quantity": 4 },
                { "name": "Shims" }
            ]
        }),
    )
    .await;

    let sheet = t
        .call(
            "get_work_sheet",
            json!({ "vehicle_id": v, "appointment_id": appt }),
        )
        .await;

    assert_eq!(
        sheet["jobs"][0]["owner_supplies"][0]["contents"],
        json!([
            { "id": kit["contents"][0]["id"], "name": "Pads", "quantity": 4, "label": "Akebono ACT1633A" },
            { "id": kit["contents"][1]["id"], "name": "Shims", "quantity": 1, "label": null }
        ]),
        "got: {sheet}"
    );
}

#[tokio::test]
async fn work_sheet_lists_unsettled_needs() {
    let t = connect().await;
    let (v, appt) = work_sheet_fixture(&t).await;

    let sheet = t
        .call(
            "get_work_sheet",
            json!({ "vehicle_id": v, "appointment_id": appt }),
        )
        .await;

    assert_eq!(
        sheet["unsettled"][0]["name"], "Wiper blades",
        "got: {sheet}"
    );
    assert_eq!(sheet["unsettled"][0]["stage"], "undecided");
    assert_eq!(sheet["unsettled"][0]["job_title"], "Front brakes");
}

#[tokio::test]
async fn work_sheet_of_another_vehicles_appointment_is_a_tool_error() {
    let t = connect().await;
    let (_, appt) = work_sheet_fixture(&t).await;
    let other = add_vehicle(&t, "Other").await;

    let wrong = t
        .call_err(
            "get_work_sheet",
            json!({ "vehicle_id": other, "appointment_id": appt }),
        )
        .await;
    let missing = t
        .call_err(
            "get_work_sheet",
            json!({ "vehicle_id": other, "appointment_id": 9999 }),
        )
        .await;

    assert_eq!(wrong, missing.replace("9999", &appt.to_string()));
}

// A kit's items ride along wherever its need returns the part, so a caller
// reading any of these replies sees the ids it would name to remove one.
#[tokio::test]
async fn every_need_reply_shows_the_linked_kits_contents() {
    let t = connect().await;
    let (v, appt) = work_sheet_fixture(&t).await;
    let parts = t.call("list_parts", json!({ "vehicle_id": v })).await;
    t.call(
        "correct_part",
        json!({
            "vehicle_id": v,
            "part_id": parts[0]["id"],
            "add_contents": [{ "name": "Pads", "part_number": "ACT1633A", "quantity": 4 }]
        }),
    )
    .await;
    let item = |part: &Value| part["contents"][0]["part_number"].clone();

    let list = t
        .call(
            "get_checklist",
            json!({ "vehicle_id": v, "appointment_id": appt }),
        )
        .await;
    let need = &list["jobs"][0]["needs"][0];
    assert_eq!(need["part"]["part_number"], "ACT1633", "got: {list}");
    assert_eq!(item(&need["part"]), "ACT1633A");
    assert!(need["part"]["contents"][0]["id"].is_i64());
    let need_id = need["need"]["id"].clone();

    let relinked = t
        .call(
            "link_part_to_need",
            json!({ "vehicle_id": v, "need_id": need_id, "part_id": parts[0]["id"] }),
        )
        .await;
    // Linking the part the need already holds changes nothing.
    assert_eq!(item(&relinked["part"]), "ACT1633A", "got: {relinked}");

    let received = t
        .call(
            "mark_need_received",
            json!({ "vehicle_id": v, "need_id": need_id }),
        )
        .await;
    assert_eq!(item(&received["part"]), "ACT1633A", "got: {received}");

    let returned = t
        .call(
            "return_part",
            json!({ "vehicle_id": v, "need_id": need_id, "reason": "Wrong axle" }),
        )
        .await;
    assert_eq!(
        item(&returned["returned_part"]),
        "ACT1633A",
        "got: {returned}"
    );

    let past = t.call("past_decisions", json!({ "query": "pads" })).await;
    assert_eq!(
        item(&past[0]["returned_parts"][0]),
        "ACT1633A",
        "got: {past}"
    );
}

#[tokio::test]
async fn mark_need_ordered_records_a_kits_contents() {
    let t = connect().await;
    let v = add_vehicle(&t, "Golf GTI").await;
    let job = plan_job(&t, v, "Oil change", None).await;
    let need = add_need(&t, v, job, "Oil and filter").await;
    let option = add_option(
        &t,
        v,
        need,
        json!({ "description": "Oil change kit", "manufacturer": "Liqui Moly", "part_number": "06K115562LM" }),
    )
    .await;
    t.call(
        "update_need",
        json!({ "vehicle_id": v, "need_id": need, "supplied_by": "self", "chosen_option_id": option }),
    )
    .await;

    let ordered = t
        .call(
            "mark_need_ordered",
            json!({
                "vehicle_id": v,
                "need_id": need,
                "contents": [{ "name": "Oil filter", "manufacturer": "Mann", "part_number": "HU 7020 z" }]
            }),
        )
        .await;

    assert_eq!(ordered["stage"], "ordered", "got: {ordered}");
    assert_eq!(ordered["part"]["part_number"], "06K115562LM");
    assert_eq!(ordered["part"]["contents"][0]["part_number"], "HU 7020 z");
}
