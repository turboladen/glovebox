mod common;

use common::{add_vehicle, assert_unknown_field_rejected, connect, id};
use serde_json::{Value, json};

fn series<'a>(coming: &'a Value, component: &str, axle: Option<&str>) -> &'a Value {
    coming["wear"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["component"] == component && o.get("axle").and_then(Value::as_str) == axle)
        .unwrap_or_else(|| panic!("no {component} {axle:?} series in {}", coming["wear"]))
}

// The readings of the owner's own invoices: brakes in millimeters and tires
// in 32nds, the front pads replaced in between.
#[tokio::test]
async fn invoice_readings_project_when_the_brakes_and_tires_reach_their_limits() {
    let t = connect().await;
    let vid = add_vehicle(&t, "GTI").await;
    let reading = |component: &str, axle: &str, value: f64, unit: &str| {
        json!({ "component": component, "axle": axle,
                "reading": { "value": value, "unit": unit } })
    };

    t.call(
        "record_service",
        json!({
            "vehicle_id": vid, "service_date": "2020-05-19", "mileage": 30_000,
            "wear_readings": [
                reading("brake_pads", "front", 5.0, "mm"),
                reading("brake_pads", "rear", 9.0, "mm"),
                reading("tire_tread", "front", 9.0, "in_32"),
                reading("tire_tread", "rear", 10.0, "in_32"),
            ],
        }),
    )
    .await;
    let brake_job = t
        .call(
            "record_service",
            json!({
                "vehicle_id": vid, "service_date": "2022-09-15", "mileage": 40_000,
                "parts": [{ "name": "Front brake pad set",
                            "wears_as": { "component": "brake_pads", "axle": "front" } }],
            }),
        )
        .await;
    assert_eq!(brake_job["parts"][0]["wears_component"], "brake_pads");
    assert_eq!(brake_job["parts"][0]["wears_axle"], "front");
    let last = t
        .call(
            "record_service",
            json!({
                "vehicle_id": vid, "service_date": "2025-10-21", "mileage": 57_151,
                "wear_readings": [
                    reading("brake_pads", "front", 8.0, "mm"),
                    { "component": "brake_pads", "axle": "rear",
                      "reading": { "value": 2, "unit": "mm" },
                      "limit": { "value": 3, "unit": "mm" }, "notes": "FAIL" },
                    reading("tire_tread", "front", 4.0, "in_32"),
                    reading("tire_tread", "rear", 5.0, "in_32"),
                ],
            }),
        )
        .await;
    assert_eq!(last["wear_readings"].as_array().unwrap().len(), 4);
    assert_eq!(last["wear_readings"][1]["reading"]["text"], "2 mm");
    assert_eq!(last["wear_readings"][1]["odometer"], 57_151);

    let coming = t.call("whats_coming", json!({ "vehicle_id": vid })).await;
    let rear = series(&coming, "brake_pads", Some("rear"));
    assert_eq!(rear["status"], "overdue");
    assert_eq!(rear["limit_source"], "shop");
    assert_eq!(coming["wear"][0], *rear, "the overdue series comes first");

    // The front pads count only from their install, so one reading is no rate.
    let front = series(&coming, "brake_pads", Some("front"));
    assert_eq!(front["series_starts_on"], "2022-09-15");
    assert_eq!(front["readings_in_series"], 1);
    assert_eq!(front["miles_left"], Value::Null);

    // The tires are one set, judged by the lowest reading of each visit: 9/32
    // to 4/32 over 27,151 miles.
    let tires = series(&coming, "tire_tread", None);
    assert_eq!(tires["latest"]["text"], "4/32 in");
    assert_eq!(tires["limit"]["text"], "2/32 in");
    assert_eq!(tires["limit_source"], "default");
    assert!(tires["miles_left"].as_i64().is_some(), "{tires}");

    let listed = t
        .call(
            "list_wear_readings",
            json!({ "vehicle_id": vid, "component": "brake_pads", "axle": "rear", "limit": 1 }),
        )
        .await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["notes"], "FAIL");
}

#[tokio::test]
async fn a_reading_is_recorded_corrected_and_deleted() {
    let t = connect().await;
    let vid = add_vehicle(&t, "GTI").await;
    let service = id(&t
        .call(
            "record_service",
            json!({ "vehicle_id": vid, "service_date": "2025-10-21", "mileage": 57_151 }),
        )
        .await);
    let recorded = t
        .call(
            "record_wear_reading",
            json!({
                "vehicle_id": vid, "service_id": service, "component": "brake_pads",
                "axle": "front", "reading": { "value": 4, "unit": "mm" },
                "limit": { "value": 3, "unit": "mm" },
            }),
        )
        .await;
    let rid = id(&recorded);
    assert_eq!(recorded["measured_on"], "2025-10-21");
    assert_eq!(recorded["odometer"], 57_151);

    // A reading copied from a reply goes back as it is, `text` and all.
    let corrected = t
        .call(
            "correct_wear_reading",
            json!({
                "vehicle_id": vid, "wear_reading_id": rid,
                "reading": recorded["limit"], "limit": null, "notes": "caution",
            }),
        )
        .await;
    assert_eq!(corrected["reading"]["text"], "3 mm");
    assert_eq!(corrected["limit"], Value::Null);
    assert_eq!(corrected["notes"], "caution");

    let got = t
        .call(
            "get_service",
            json!({ "vehicle_id": vid, "service_id": service }),
        )
        .await;
    assert_eq!(got["wear_readings"][0]["id"], rid);

    let deleted = t
        .call(
            "delete_wear_reading",
            json!({ "vehicle_id": vid, "wear_reading_id": rid }),
        )
        .await;
    assert_eq!(deleted, json!({ "deleted": "wear_reading", "id": rid }));
    let err = t
        .call_err(
            "delete_wear_reading",
            json!({ "vehicle_id": vid, "wear_reading_id": rid }),
        )
        .await;
    assert_eq!(err, format!("Wear reading {rid} not found"));
}

#[tokio::test]
async fn bad_wear_arguments_are_refused_by_name() {
    let t = connect().await;
    let vid = add_vehicle(&t, "GTI").await;
    let err = t
        .call_err(
            "record_wear_reading",
            json!({ "vehicle_id": vid, "component": "brake_pads", "axle": "front",
                    "reading": { "value": 4, "unit": "mm" } }),
        )
        .await;
    assert_eq!(err, "measured_on: is needed for a reading no service took");

    let err = t
        .call_err(
            "record_wear_reading",
            json!({ "vehicle_id": vid, "component": "brake_pads", "axle": "front",
                    "measured_on": "2025-01-01", "reading": { "value": 50, "unit": "percent" } }),
        )
        .await;
    assert!(err.contains("reading.unit"), "{err}");

    let err = t
        .call_err(
            "record_part",
            json!({ "vehicle_id": vid, "name": "Tires",
                    "wears_as": { "component": "tire_tread", "axle": "front" } }),
        )
        .await;
    assert_eq!(
        err,
        "wears_as: the tires are tracked as one set, so a tire part names no axle"
    );

    assert_unknown_field_rejected(
        &t,
        "record_wear_reading",
        json!({ "vehicle_id": vid, "component": "brake_pads", "axle": "front",
                "measured_on": "2025-01-01", "reading": { "value": 4, "unit": "mm", "mm": 4 } }),
        "mm",
    )
    .await;
}

#[tokio::test]
async fn a_part_tag_is_set_and_cleared_with_correct_part() {
    let t = connect().await;
    let vid = add_vehicle(&t, "GTI").await;
    let part = id(&t
        .call(
            "record_part",
            json!({ "vehicle_id": vid, "name": "Rotors" }),
        )
        .await);
    let tagged = t
        .call(
            "correct_part",
            json!({ "vehicle_id": vid, "part_id": part,
                    "wears_as": { "component": "brake_rotors", "axle": "rear" } }),
        )
        .await;
    assert_eq!(tagged["wears_component"], "brake_rotors");
    assert_eq!(tagged["wears_axle"], "rear");
    let cleared = t
        .call(
            "correct_part",
            json!({ "vehicle_id": vid, "part_id": part, "wears_as": null }),
        )
        .await;
    assert_eq!(cleared["wears_component"], Value::Null);
    assert_eq!(cleared["wears_axle"], Value::Null);
}
