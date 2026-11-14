// A new owner's first session, driven entirely through tool calls: add the
// car from a template, plan some work, file one realistic invoice, capture a
// shop advisory, ask what is coming, and prepare the next shop visit.

mod common;

use common::{connect, reminder};
use serde_json::{Value, json};

fn titles(items: &Value) -> Vec<&str> {
    items
        .as_array()
        .expect("array of items")
        .iter()
        .map(|i| i["title"].as_str().expect("title"))
        .collect()
}

#[tokio::test]
async fn a_new_owner_can_set_up_file_an_invoice_and_ask_whats_coming() {
    let t = connect().await;

    // 1. Add the car from the seeded GTI template.
    let templates = t.call("list_model_templates", json!({})).await;
    let gti = templates
        .as_array()
        .expect("template list")
        .iter()
        .find(|tpl| tpl["model"] == "Golf GTI")
        .unwrap_or_else(|| panic!("the GTI template must be seeded; got: {templates}"));
    let added = t
        .call(
            "add_vehicle",
            json!({
                "name": "GTI",
                "model_template_id": gti["id"],
                "year": 2017,
                "make": "Volkswagen",
                "model": "Golf GTI",
                "purchase_date": "2017-03-05",
                "purchase_mileage": 40
            }),
        )
        .await;
    let vehicle_id = added["id"].as_i64().expect("vehicle id");

    // 2. Plan the pads before the shop visit.
    let pads = t
        .call(
            "plan_work",
            json!({ "vehicle_id": vehicle_id, "title": "Replace front pads" }),
        )
        .await;

    // The seeded 60k spark plug item is overdue once the car passes 60k.
    t.call(
        "log_mileage",
        json!({ "vehicle_id": vehicle_id, "mileage": 64_000 }),
    )
    .await;
    let before = t
        .call("whats_coming", json!({ "vehicle_id": vehicle_id }))
        .await;
    let plugs = reminder(&before, "Spark Plugs");
    assert_eq!(plugs["status"], "overdue", "got: {plugs}");
    let plugs_id = plugs["schedule_item"]["id"].clone();

    // 3. File the invoice in one call.
    let recorded = t
        .call(
            "record_service",
            json!({
                "vehicle_id": vehicle_id,
                "service_date": "2026-11-10",
                "end_date": "2026-11-12",
                "performed_by": "shop",
                "shop_name": "Autohaus",
                "mileage": 64_000,
                "invoice_ref": "RO-2291",
                "description": "60k service and front brakes",
                "total_cost_cents": 98_000,
                "line_items": [
                    { "description": "Spark plugs", "work_kind": "maintenance", "cost_cents": 30_000 },
                    { "description": "Front pads and rotors", "work_kind": "repair", "cost_cents": 50_000 },
                    { "description": "ECU flash", "work_kind": "mod", "cost_cents": 18_000 }
                ],
                "parts": [
                    { "name": "Front brake pads", "seller": "FCP Euro", "supplied_by": "self", "cost_cents": 9_000 },
                    { "name": "Spark plug set", "supplied_by": "shop", "cost_cents": 6_000 }
                ],
                "work_item_ids": [pads["id"]],
                "schedule_item_ids": [plugs_id]
            }),
        )
        .await;
    assert!(
        recorded.get("already_recorded").is_none(),
        "got: {recorded}"
    );
    let service_id = recorded["id"].as_i64().expect("service id");

    // 4. Capture the advisory the shop wrote on the invoice.
    let weeping = t
        .call(
            "log_incident",
            json!({
                "vehicle_id": vehicle_id,
                "title": "Water pump weeping",
                "category": "shop_observation",
                "noticed_during_service_id": service_id
            }),
        )
        .await;
    t.call(
        "plan_work",
        json!({
            "vehicle_id": vehicle_id,
            "title": "Replace water pump",
            "incident_id": weeping["id"],
            "revisit_by_miles": 70_000
        }),
    )
    .await;

    // 5. Ask what is coming.
    let coming = t
        .call("whats_coming", json!({ "vehicle_id": vehicle_id }))
        .await;
    assert_eq!(reminder(&coming, "Spark Plugs")["status"], "ok");
    assert_eq!(titles(&coming["planned_work"]), vec!["Replace water pump"]);
    assert_eq!(coming["planned_work"][0]["revisit_status"], "ok");
    assert_eq!(coming["open_incidents"][0]["title"], "Water pump weeping");

    // 6. The service reads back whole.
    let service = t
        .call(
            "get_service",
            json!({ "vehicle_id": vehicle_id, "service_id": service_id }),
        )
        .await;
    assert_eq!(
        service["noticed_incidents"][0]["title"],
        "Water pump weeping"
    );
    let mut suppliers: Vec<&str> = service["parts"]
        .as_array()
        .expect("parts")
        .iter()
        .map(|p| p["supplied_by"].as_str().expect("supplied_by"))
        .collect();
    suppliers.sort_unstable();
    assert_eq!(suppliers, vec!["self", "shop"]);
    let mut kinds: Vec<&str> = service["line_items"]
        .as_array()
        .expect("line items")
        .iter()
        .map(|l| l["work_kind"].as_str().expect("work_kind"))
        .collect();
    kinds.sort_unstable();
    assert_eq!(kinds, vec!["maintenance", "mod", "repair"]);

    // 7. Filing the same invoice again returns the existing record.
    let again = t
        .call(
            "record_service",
            json!({
                "vehicle_id": vehicle_id,
                "service_date": "2026-11-10",
                "invoice_ref": "RO-2291"
            }),
        )
        .await;
    assert_eq!(again["already_recorded"], true);
    assert_eq!(again["id"].as_i64(), Some(service_id));

    // 8. Prepare the next visit: an appointment, a job on it, a need with two
    // researched options, and the owner's choice.
    let appointment = t
        .call(
            "plan_appointment",
            json!({ "vehicle_id": vehicle_id, "purpose": "Rear brakes", "shop_name": "Autohaus" }),
        )
        .await;
    let job = t
        .call(
            "plan_work",
            json!({
                "vehicle_id": vehicle_id,
                "title": "Replace rear pads and rotors",
                "appointment_id": appointment["id"]
            }),
        )
        .await;
    let need = t
        .call(
            "add_need",
            json!({ "vehicle_id": vehicle_id, "work_item_id": job["id"], "name": "Rear pads" }),
        )
        .await;
    let fits = t
        .call(
            "add_need_option",
            json!({
                "vehicle_id": vehicle_id, "need_id": need["id"], "description": "OE-spec pads",
                "manufacturer": "Textar", "part_number": "2530101", "fitment": "confirmed"
            }),
        )
        .await;
    t.call(
        "add_need_option",
        json!({
            "vehicle_id": vehicle_id, "need_id": need["id"], "description": "Track pads",
            "fitment": "does_not_fit", "fitment_note": "Wrong shape for the single-piston caliper"
        }),
    )
    .await;
    t.call(
        "update_need",
        json!({
            "vehicle_id": vehicle_id, "need_id": need["id"], "supplied_by": "self",
            "chosen_option_id": fits["id"], "decision_reason": "Quiet on the street"
        }),
    )
    .await;
    let checklist = t
        .call(
            "get_checklist",
            json!({ "vehicle_id": vehicle_id, "appointment_id": appointment["id"] }),
        )
        .await;
    assert_eq!(checklist["next_steps"]["order"][0]["name"], "Rear pads");
    let coming = t
        .call("whats_coming", json!({ "vehicle_id": vehicle_id }))
        .await;
    assert_eq!(
        coming["appointments"][0]["purpose"], "Rear brakes",
        "got: {coming}"
    );
}
