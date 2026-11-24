mod common;

use common::{connect, new_vehicle};
use glovebox_shared::services::vehicle;
use serde_json::json;

#[tokio::test]
async fn record_part_round_trips_and_rejects_a_missing_vehicle() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();

    // The tool result payload is the created part record itself.
    let part = t
        .call(
            "record_part",
            json!({
                "vehicle_id": v.id,
                "name": "Sachs SRE clutch kit",
                "cost_cents": 89_900,
                "location": "Front brakes",
                "seller": "FCP Euro"
            }),
        )
        .await;
    assert_eq!(part["name"], "Sachs SRE clutch kit");
    assert_eq!(part["location"], "Front brakes");
    assert!(
        part["id"].is_i64(),
        "created part must carry an id; got: {part}"
    );
    assert_eq!(
        part["status"], "purchased",
        "status must default to purchased"
    );

    t.call_err(
        "record_part",
        json!({ "vehicle_id": 999, "name": "Oil filter" }),
    )
    .await;
}

#[tokio::test]
async fn list_correct_and_delete_a_part() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let other = vehicle::create(&t.db, new_vehicle("Other")).await.unwrap();
    let part = t
        .call(
            "record_part",
            json!({
                "vehicle_id": v.id,
                "name": "Clutch kit",
                "cost_cents": 89_900,
                "seller": "FCP Euro"
            }),
        )
        .await;
    let pid = part["id"].clone();

    let listed = t.call("list_parts", json!({ "vehicle_id": v.id })).await;
    assert_eq!(listed.as_array().map(Vec::len), Some(1));
    let installed_only = t
        .call(
            "list_parts",
            json!({ "vehicle_id": v.id, "status": "installed" }),
        )
        .await;
    assert_eq!(installed_only, json!([]));

    let corrected = t
        .call(
            "correct_part",
            json!({
                "vehicle_id": v.id,
                "part_id": pid,
                "cost_cents": 79_900,
                "seller": null
            }),
        )
        .await;
    assert_eq!(corrected["cost_cents"], 79_900);
    assert!(
        corrected["seller"].is_null(),
        "null clears seller: {corrected}"
    );
    assert_eq!(corrected["name"], "Clutch kit", "absent keys stay put");

    for tool in ["correct_part", "delete_part"] {
        let wrong = t
            .call_err(tool, json!({ "vehicle_id": other.id, "part_id": pid }))
            .await;
        let missing = t
            .call_err(tool, json!({ "vehicle_id": other.id, "part_id": 99999 }))
            .await;
        assert_eq!(
            wrong.replace(&pid.to_string(), "N"),
            missing.replace("99999", "N"),
            "{tool}: a wrong-vehicle id must read like a nonexistent one"
        );
    }

    t.call("delete_part", json!({ "vehicle_id": v.id, "part_id": pid }))
        .await;
    let after = t.call("list_parts", json!({ "vehicle_id": v.id })).await;
    assert_eq!(after, json!([]));
}

#[tokio::test]
async fn correcting_an_installed_part_to_purchased_clears_its_install_details() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let service = t
        .call(
            "record_service",
            json!({ "vehicle_id": v.id, "service_date": "2026-11-01", "mileage": 40_000 }),
        )
        .await;
    let part = t
        .call(
            "record_part",
            json!({
                "vehicle_id": v.id,
                "name": "Brake pads",
                "status": "installed",
                "installed_service_id": service["id"],
                "installed_date": "2026-11-01",
                "installed_odometer": 40_000
            }),
        )
        .await;
    assert_eq!(part["installed_service_id"], service["id"], "{part}");

    let corrected = t
        .call(
            "correct_part",
            json!({ "vehicle_id": v.id, "part_id": part["id"], "status": "purchased" }),
        )
        .await;
    assert_eq!(corrected["status"], "purchased");
    for field in [
        "installed_service_id",
        "installed_date",
        "installed_odometer",
    ] {
        assert!(
            corrected[field].is_null(),
            "{field} is cleared: {corrected}"
        );
    }
}

#[tokio::test]
async fn parts_tools_reject_unknown_arguments() {
    let t = connect().await;
    common::assert_unknown_field_rejected(
        &t,
        "record_part",
        json!({ "vehicle_id": 1, "name": "Pads", "brand": "x" }),
        "brand",
    )
    .await;
    common::assert_unknown_field_rejected(
        &t,
        "record_part",
        json!({ "vehicle_id": 1, "name": "Kit", "contents": [{ "name": "Filter", "sku": "x" }] }),
        "sku",
    )
    .await;
    // `correct_part` adds and removes items by id; a whole list is not an
    // argument it takes.
    common::assert_unknown_field_rejected(
        &t,
        "correct_part",
        json!({ "vehicle_id": 1, "part_id": 1, "contents": [{ "name": "Filter" }] }),
        "contents",
    )
    .await;
}

#[tokio::test]
async fn a_kits_contents_are_recorded_listed_added_to_and_removed_by_id() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let kit = t
        .call(
            "record_part",
            json!({
                "vehicle_id": v.id,
                "name": "Oil change kit",
                "part_number": "06K115562LM",
                "contents": [
                    { "name": "Oil filter", "manufacturer": "Mann", "part_number": "HU 7020 z" },
                    { "name": "Engine oil, 1 L", "quantity": 5 }
                ]
            }),
        )
        .await;
    let names = |part: &serde_json::Value| -> Vec<String> {
        part["contents"]
            .as_array()
            .unwrap_or_else(|| panic!("no contents in {part}"))
            .iter()
            .map(|c| c["name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(names(&kit), ["Oil filter", "Engine oil, 1 L"]);
    assert_eq!(kit["contents"][1]["quantity"], 5);

    let listed = t.call("list_parts", json!({ "vehicle_id": v.id })).await;
    assert_eq!(names(&listed[0]), ["Oil filter", "Engine oil, 1 L"]);

    let untouched = t
        .call(
            "correct_part",
            json!({ "vehicle_id": v.id, "part_id": kit["id"], "seller": "FCP Euro" }),
        )
        .await;
    assert_eq!(names(&untouched), ["Oil filter", "Engine oil, 1 L"]);

    let ids = |part: &serde_json::Value| -> Vec<i64> {
        part["contents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_i64().unwrap())
            .collect()
    };
    let filter = ids(&untouched)[0];
    let edited = t
        .call(
            "correct_part",
            json!({
                "vehicle_id": v.id,
                "part_id": kit["id"],
                "add_contents": [{ "name": "Drain plug" }],
                "remove_contents": [filter]
            }),
        )
        .await;
    assert_eq!(names(&edited), ["Engine oil, 1 L", "Drain plug"]);

    // An item copied from a reply, id and all, is added as a new item.
    let copied = edited["contents"][1].clone();
    let readded = t
        .call(
            "correct_part",
            json!({ "vehicle_id": v.id, "part_id": kit["id"], "add_contents": [copied] }),
        )
        .await;
    assert_eq!(
        names(&readded),
        ["Engine oil, 1 L", "Drain plug", "Drain plug"]
    );
    assert_ne!(readded["contents"][2]["id"], edited["contents"][1]["id"]);
    let edited = t
        .call(
            "correct_part",
            json!({ "vehicle_id": v.id, "part_id": kit["id"], "remove_contents": [readded["contents"][2]["id"]] }),
        )
        .await;

    let refused = t
        .call_err(
            "correct_part",
            json!({ "vehicle_id": v.id, "part_id": kit["id"], "add_contents": [{ "name": " " }] }),
        )
        .await;
    assert_eq!(refused, "add_contents: item 1 needs a name");
    let gone = t
        .call_err(
            "correct_part",
            json!({ "vehicle_id": v.id, "part_id": kit["id"], "remove_contents": [filter] }),
        )
        .await;
    assert_eq!(gone, format!("Kit item {filter} not found"));

    let emptied = t
        .call(
            "correct_part",
            json!({ "vehicle_id": v.id, "part_id": kit["id"], "remove_contents": ids(&edited) }),
        )
        .await;
    assert_eq!(emptied["contents"], json!([]));
}

#[tokio::test]
async fn a_status_takes_only_the_details_it_has() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();

    // Install details on a part that stays purchased are refused, on a new
    // part and on a stored one.
    let err = t
        .call_err(
            "record_part",
            json!({ "vehicle_id": v.id, "name": "Pads", "installed_date": "2026-11-01" }),
        )
        .await;
    assert!(
        err.contains("A purchased part takes no install details"),
        "{err}"
    );
    let part = t
        .call("record_part", json!({ "vehicle_id": v.id, "name": "Pads" }))
        .await;
    let pid = part["id"].clone();
    let err = t
        .call_err(
            "correct_part",
            json!({ "vehicle_id": v.id, "part_id": pid, "installed_odometer": 40_000 }),
        )
        .await;
    assert!(
        err.contains("is purchased, so it takes no install or replacement details"),
        "{err}"
    );

    // An installed part takes replacement details only as replaced.
    t.call(
        "correct_part",
        json!({ "vehicle_id": v.id, "part_id": pid, "status": "installed",
                "installed_odometer": 40_000 }),
    )
    .await;
    let err = t
        .call_err(
            "correct_part",
            json!({ "vehicle_id": v.id, "part_id": pid, "replaced_date": "2026-11-10" }),
        )
        .await;
    assert!(
        err.contains("is installed, so it takes no replacement details"),
        "{err}"
    );
    let replaced = t
        .call(
            "correct_part",
            json!({ "vehicle_id": v.id, "part_id": pid, "status": "replaced",
                    "replaced_date": "2026-11-10" }),
        )
        .await;
    assert_eq!(replaced["replaced_date"], "2026-11-10", "{replaced}");
    assert_eq!(replaced["installed_odometer"], 40_000, "{replaced}");

    // Setting it back to installed clears the replacement in the same call.
    let err = t
        .call_err(
            "correct_part",
            json!({ "vehicle_id": v.id, "part_id": pid, "status": "installed" }),
        )
        .await;
    assert!(
        err.contains("clear its replacement date and odometer"),
        "{err}"
    );
    let back = t
        .call(
            "correct_part",
            json!({ "vehicle_id": v.id, "part_id": pid, "status": "installed",
                    "replaced_date": null }),
        )
        .await;
    assert_eq!(back["status"], "installed", "{back}");
    assert!(back["replaced_date"].is_null(), "{back}");
}

#[tokio::test]
async fn a_part_is_never_received_before_it_was_bought() {
    let t = connect().await;
    let v = vehicle::create(&t.db, new_vehicle("Daily")).await.unwrap();
    let part = t
        .call(
            "record_part",
            json!({ "vehicle_id": v.id, "name": "Pads", "purchase_date": "2026-11-05" }),
        )
        .await;
    let err = t
        .call_err(
            "correct_part",
            json!({ "vehicle_id": v.id, "part_id": part["id"], "received_on": "2026-11-04" }),
        )
        .await;
    assert!(
        err.contains("received_on: is before the purchase date 2026-11-05"),
        "{err}"
    );
}

#[tokio::test]
async fn list_parts_reads_each_warranty_and_filters_on_it() {
    let t = connect().await;
    let v = t
        .call(
            "add_vehicle",
            json!({ "name": "Daily", "purchase_mileage": 50_000 }),
        )
        .await["id"]
        .clone();
    for (name, on, miles) in [
        ("Battery", Some("2099-01-01"), Some(60_000)),
        ("Wheel bearing", Some("2099-01-01"), Some(40_000)),
        ("Alternator", Some("2020-01-01"), Some(60_000)),
        ("Wipers", None, None),
    ] {
        t.call(
            "record_part",
            json!({
                "vehicle_id": v,
                "name": name,
                "warranty_expires_on": on,
                "warranty_expires_miles": miles
            }),
        )
        .await;
    }
    let names = |listed: &serde_json::Value| -> Vec<String> {
        let mut names: Vec<String> = listed
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap().to_owned())
            .collect();
        names.sort();
        names
    };

    let all = t.call("list_parts", json!({ "vehicle_id": v })).await;
    assert_eq!(all.as_array().map(Vec::len), Some(4));
    let by_name = |name: &str| {
        all.as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap()
            .clone()
    };
    let battery = by_name("Battery");
    assert_eq!(battery["warranty"]["in_force"], true);
    assert_eq!(battery["warranty"]["miles_left"], 10_000);
    assert_eq!(battery["warranty"]["date_passed"], false);
    // A warranty ends at the first bound passed, here the mileage.
    let bearing = by_name("Wheel bearing");
    assert_eq!(bearing["warranty"]["in_force"], false);
    assert_eq!(bearing["warranty"]["mileage_passed"], true);
    assert_eq!(bearing["warranty"]["date_passed"], false);
    let alternator = by_name("Alternator");
    assert_eq!(alternator["warranty"]["in_force"], false);
    assert_eq!(alternator["warranty"]["days_left"], 0);
    assert!(by_name("Wipers").get("warranty").is_none());

    let covered = t
        .call(
            "list_parts",
            json!({ "vehicle_id": v, "under_warranty": true }),
        )
        .await;
    assert_eq!(names(&covered), ["Battery"]);
    let rest = t
        .call(
            "list_parts",
            json!({ "vehicle_id": v, "under_warranty": false }),
        )
        .await;
    assert_eq!(names(&rest), ["Alternator", "Wheel bearing", "Wipers"]);

    // A part reply outside the list carries no warranty reading.
    let recorded = t
        .call(
            "correct_part",
            json!({ "vehicle_id": v, "part_id": battery["id"], "notes": "AGM" }),
        )
        .await;
    assert!(recorded.get("warranty").is_none(), "{recorded}");

    common::assert_unknown_field_rejected(
        &t,
        "list_parts",
        json!({ "vehicle_id": v, "in_warranty": true }),
        "in_warranty",
    )
    .await;
}
