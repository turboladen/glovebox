use super::*;
use crate::{
    error::DomainError,
    test_support::{date, seed_service, seed_vehicle, test_db},
};

const MM: LengthUnit = LengthUnit::Millimeter;
const IN32: LengthUnit = LengthUnit::ThirtySecondInch;

fn row(
    id: i32,
    component: WearComponent,
    axle: Axle,
    value: f64,
    unit: LengthUnit,
    day: &str,
    odometer: i32,
) -> wear_reading::Model {
    wear_reading::Model {
        id,
        vehicle_id: 1,
        service_id: None,
        component,
        axle,
        value,
        unit,
        limit_value: None,
        limit_unit: None,
        measured_on: date(day),
        odometer: Some(odometer),
        notes: None,
        created_at: crate::services::now(),
        updated_at: crate::services::now(),
    }
}

fn pads(id: i32, axle: Axle, mm: f64, day: &str, odometer: i32) -> wear_reading::Model {
    row(id, WearComponent::BrakePads, axle, mm, MM, day, odometer)
}

fn tread(id: i32, axle: Axle, n: f64, day: &str, odometer: i32) -> wear_reading::Model {
    row(id, WearComponent::TireTread, axle, n, IN32, day, odometer)
}

fn with_limit(mut r: wear_reading::Model, value: f64, unit: LengthUnit) -> wear_reading::Model {
    r.limit_value = Some(value);
    r.limit_unit = Some(unit);
    r
}

// The car drives 20 miles a day and reads 50,000 on the day of the outlook.
fn judge(
    readings: Vec<wear_reading::Model>,
    installs: &BTreeMap<WearTag, Install>,
) -> Vec<WearOutlook> {
    outlook_from(readings, installs, &[], 50_000, 20.0, date("2026-11-26"))
}

#[test]
fn a_falling_series_is_projected_to_its_limit() {
    // 10 mm at 30,000 and 6 mm at 40,000: 4 mm over 10,000 miles, so 3 mm
    // left above the 3 mm default lasts 7,500 miles from 40,000, which is
    // 47,500: 2,500 miles behind the estimate.
    let out = judge(
        vec![
            pads(1, Axle::Front, 10.0, "2024-01-01", 30_000),
            pads(2, Axle::Front, 6.0, "2025-01-01", 40_000),
        ],
        &BTreeMap::new(),
    );
    assert_eq!(out.len(), 1);
    let o = &out[0];
    assert_eq!(o.axle, Some(Axle::Front));
    assert_eq!(o.limit, Some(Measure::new(3.0, MM)));
    assert_eq!(o.limit_source, Some(LimitSource::Default));
    assert_eq!(o.wear_per_10k_miles, Some(Measure::new(4.0, MM)));
    assert_eq!(o.miles_left, Some(-2_500));
    // At 20 miles a day the limit was reached 125 days ago.
    assert_eq!(
        o.due_on,
        Some(date::<NaiveDate>("2026-11-26") - chrono::Duration::days(125))
    );
    assert_eq!(o.status, DueStatus::Overdue);
    assert_eq!(o.note, None);
}

#[test]
fn a_projection_ahead_reads_its_day_and_due_soon_inside_the_warning() {
    // 10 mm at 40,000 and 8 mm at 50,000: 5 mm above 3 mm lasts 25,000
    // miles, 1,250 days at 20 a day.
    let far = judge(
        vec![
            pads(1, Axle::Rear, 10.0, "2025-01-01", 40_000),
            pads(2, Axle::Rear, 8.0, "2026-11-26", 50_000),
        ],
        &BTreeMap::new(),
    );
    assert_eq!(far[0].miles_left, Some(25_000));
    assert_eq!(
        far[0].due_on,
        Some(date::<NaiveDate>("2026-11-26") + chrono::Duration::days(1_250))
    );
    assert_eq!(far[0].status, DueStatus::NotDue);

    // 4 mm at 40,000 and 3.04 mm at 50,000 wear 0.96 mm over 10,000 miles,
    // so the 0.04 mm left lasts about 417 miles, inside the 1,000-mile
    // warning.
    let near = judge(
        vec![
            pads(1, Axle::Rear, 4.0, "2025-01-01", 40_000),
            pads(2, Axle::Rear, 3.04, "2026-11-26", 50_000),
        ],
        &BTreeMap::new(),
    );
    assert_eq!(near[0].miles_left, Some(417));
    assert_eq!(near[0].status, DueStatus::DueSoon);
}

#[test]
fn readings_in_32nds_and_millimeters_share_one_series() {
    // 8/32 in is 6.35 mm. The second reading is 4.7625 mm, so the series wears
    // 1.5875 mm over 10,000 miles, which is 2/32 in.
    let out = judge(
        vec![
            row(
                1,
                WearComponent::TireTread,
                Axle::Front,
                6.35,
                MM,
                "2025-01-01",
                40_000,
            ),
            tread(2, Axle::Front, 6.0, "2026-01-01", 50_000),
        ],
        &BTreeMap::new(),
    );
    assert_eq!(out[0].wear_per_10k_miles, Some(Measure::new(2.0, IN32)));
    assert_eq!(out[0].latest, Measure::new(6.0, IN32));
    assert_eq!(out[0].limit, Some(Measure::new(2.0, IN32)));
}

#[test]
fn the_tires_are_one_series_judged_by_the_lowest_reading_of_each_visit() {
    let out = judge(
        vec![
            tread(1, Axle::Front, 8.0, "2025-01-01", 40_000),
            with_limit(tread(2, Axle::Rear, 9.0, "2025-01-01", 40_000), 3.0, IN32),
            tread(3, Axle::Front, 7.0, "2026-01-01", 45_000),
            tread(4, Axle::Rear, 5.0, "2026-01-01", 45_000),
        ],
        &BTreeMap::new(),
    );
    assert_eq!(out.len(), 1);
    let o = &out[0];
    assert_eq!(o.axle, None);
    assert_eq!(o.readings_in_series, 2);
    assert_eq!(o.latest, Measure::new(5.0, IN32));
    // The rear's stated limit wins over the default.
    assert_eq!(o.limit, Some(Measure::new(3.0, IN32)));
    assert_eq!(o.limit_source, Some(LimitSource::Shop));
    assert_eq!(o.wear_per_10k_miles, Some(Measure::new(6.0, IN32)));
}

#[test]
fn a_tagged_install_starts_a_new_series_and_axles_stay_apart() {
    let mut installs = BTreeMap::new();
    installs.insert(
        WearTag::BrakePads(Axle::Front),
        Install {
            day: date("2025-01-01"),
            service_id: None,
        },
    );
    let out = judge(
        vec![
            pads(1, Axle::Front, 10.0, "2023-01-01", 20_000),
            pads(2, Axle::Front, 3.0, "2024-06-01", 35_000),
            pads(3, Axle::Rear, 9.0, "2024-06-01", 35_000),
            pads(4, Axle::Front, 11.0, "2025-06-01", 42_000),
        ],
        &installs,
    );
    let front = out.iter().find(|o| o.axle == Some(Axle::Front)).unwrap();
    assert_eq!(front.readings_in_series, 1);
    assert_eq!(front.series_starts_on, Some(date("2025-01-01")));
    assert_eq!(front.latest, Measure::new(11.0, MM));
    assert_eq!(front.miles_left, None);
    assert_eq!(
        front.note.as_deref(),
        Some("Only one reading since the part installed on 2025-01-01")
    );
    let rear = out.iter().find(|o| o.axle == Some(Axle::Rear)).unwrap();
    assert_eq!(rear.series_starts_on, None);
    assert_eq!(
        rear.note.as_deref(),
        Some("One reading is not enough to tell a wear rate")
    );
}

#[test]
fn a_rising_series_gives_no_projection() {
    let out = judge(
        vec![
            tread(1, Axle::Front, 3.0, "2021-09-15", 40_000),
            tread(2, Axle::Front, 7.0, "2022-06-30", 45_000),
        ],
        &BTreeMap::new(),
    );
    assert_eq!(out[0].miles_left, None);
    assert_eq!(out[0].due_on, None);
    assert_eq!(out[0].status, DueStatus::NotDue);
    assert_eq!(
        out[0].note.as_deref(),
        Some("The readings do not show wear; a replacement may be missing from the parts")
    );
}

#[test]
fn a_reading_at_its_limit_is_overdue_without_a_rate() {
    let out = judge(
        vec![with_limit(
            pads(1, Axle::Rear, 2.0, "2025-10-21", 57_151),
            3.0,
            MM,
        )],
        &BTreeMap::new(),
    );
    assert_eq!(out[0].status, DueStatus::Overdue);
}

#[test]
fn a_rotor_with_no_stated_limit_is_shown_but_not_judged() {
    let out = judge(
        vec![
            row(
                1,
                WearComponent::BrakeRotors,
                Axle::Front,
                26.0,
                MM,
                "2025-01-01",
                40_000,
            ),
            row(
                2,
                WearComponent::BrakeRotors,
                Axle::Front,
                25.0,
                MM,
                "2026-01-01",
                50_000,
            ),
        ],
        &BTreeMap::new(),
    );
    assert_eq!(out[0].limit, None);
    assert_eq!(out[0].limit_source, None);
    assert_eq!(out[0].miles_left, None);
    assert!(
        out[0]
            .note
            .as_deref()
            .unwrap()
            .starts_with("No limit is known")
    );
}

#[test]
fn the_most_urgent_series_comes_first() {
    let out = judge(
        vec![
            pads(1, Axle::Front, 9.0, "2026-01-01", 45_000),
            with_limit(pads(2, Axle::Rear, 2.0, "2026-01-01", 45_000), 3.0, MM),
        ],
        &BTreeMap::new(),
    );
    assert_eq!(out[0].axle, Some(Axle::Rear));
    assert_eq!(out[0].status, DueStatus::Overdue);
}

fn new_reading(service_id: Option<i32>) -> NewWearReading {
    NewWearReading {
        component: WearComponent::BrakePads,
        axle: Axle::Front,
        reading: Measure::new(4.0, MM),
        limit: None,
        measured_on: None,
        odometer: None,
        service_id,
        notes: None,
    }
}

#[tokio::test]
async fn a_reading_with_a_service_takes_its_day_and_odometer() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let sid = seed_service(&db, vid).await;
    let created = create(&db, vid, new_reading(Some(sid))).await.unwrap();
    assert_eq!(created.measured_on, date("2024-01-01"));
    assert_eq!(created.service_id, Some(sid));

    let listed = list(&db, vid, WearFilter::default()).await.unwrap();
    assert_eq!(listed, vec![created.clone()]);
    let by = by_service(&db, [sid]).await.unwrap();
    assert_eq!(by.get(&sid), Some(&vec![created]));
}

#[tokio::test]
async fn a_reading_with_no_day_and_no_service_is_refused() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let err = create(&db, vid, new_reading(None)).await.unwrap_err();
    assert_eq!(
        err.to_string(),
        "measured_on: is needed for a reading no service took"
    );
    let mut negative = new_reading(None);
    negative.measured_on = Some(date("2024-01-01"));
    negative.reading = Measure::new(-1.0, MM);
    let err = create(&db, vid, negative).await.unwrap_err();
    assert_eq!(err.to_string(), "reading: must be a length of zero or more");
}

#[tokio::test]
async fn another_vehicles_service_and_reading_read_as_missing() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let other = seed_vehicle(&db).await;
    let theirs = seed_service(&db, other).await;
    let missing = create(&db, vid, new_reading(Some(999_999)))
        .await
        .unwrap_err();
    let wrong = create(&db, vid, new_reading(Some(theirs)))
        .await
        .unwrap_err();
    assert!(matches!(wrong, DomainError::NotFound(_)));
    assert_eq!(
        wrong.to_string(),
        missing.to_string().replace("999999", &theirs.to_string())
    );

    let reading = create(&db, other, new_reading(Some(theirs))).await.unwrap();
    let wrong = delete(&db, vid, reading.id).await.unwrap_err();
    let missing = delete(&db, vid, 999_999).await.unwrap_err();
    assert_eq!(
        wrong.to_string(),
        missing
            .to_string()
            .replace("999999", &reading.id.to_string())
    );
    let wrong = update(&db, vid, reading.id, UpdateWearReading::default())
        .await
        .unwrap_err();
    assert!(matches!(wrong, DomainError::NotFound(_)));
    assert_eq!(
        list(&db, other, WearFilter::default()).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn a_correction_changes_only_what_it_sends() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let sid = seed_service(&db, vid).await;
    let mut input = new_reading(Some(sid));
    input.limit = Some(Measure::new(2.0, MM));
    input.notes = Some("caution".into());
    let created = create(&db, vid, input).await.unwrap();
    let updated = update(
        &db,
        vid,
        created.id,
        UpdateWearReading {
            reading: Some(Measure::new(3.5, MM)),
            limit: Some(None),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.reading, Measure::new(3.5, MM));
    assert_eq!(updated.limit, None);
    assert_eq!(updated.notes.as_deref(), Some("caution"));
    assert_eq!(updated.service_id, Some(sid));
}

#[tokio::test]
async fn outlook_counts_from_a_tagged_part_install() {
    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    for (day, mm, odo) in [("2024-01-01", 9.0, 30_000), ("2024-06-01", 4.0, 35_000)] {
        let mut r = new_reading(None);
        r.measured_on = Some(date(day));
        r.reading = Measure::new(mm, MM);
        r.odometer = Some(odo);
        create(&db, vid, r).await.unwrap();
    }
    crate::services::part::create(
        &db,
        vid,
        crate::inputs::part::NewPart {
            name: "Front pads".into(),
            fitting: crate::inputs::part::NewFitting::Installed(crate::inputs::part::Install {
                date: Some(date("2024-07-01")),
                ..Default::default()
            }),
            wears_as: Some(WearTag::BrakePads(Axle::Front)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let today = date("2026-11-26");
    let reminders = crate::services::reminders::calculate_reminders_on(&db, vid, today)
        .await
        .unwrap();
    // Both readings predate the install: they measured the pads that came off.
    crate::test_support::assert_empty(&outlook(&db, vid, &reminders, today).await.unwrap());

    let mut since = new_reading(None);
    since.measured_on = Some(date("2025-01-01"));
    since.reading = Measure::new(11.0, MM);
    since.odometer = Some(40_000);
    create(&db, vid, since).await.unwrap();
    let out = outlook(&db, vid, &reminders, today).await.unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].series_starts_on, Some(date("2024-07-01")));
    assert_eq!(out[0].readings_in_series, 1);
    assert_eq!(out[0].latest, Measure::new(11.0, MM));
}

fn service_with_readings(
    readings: Vec<NewWearReading>,
) -> crate::inputs::service_record::NewServiceRecord {
    crate::inputs::service_record::NewServiceRecord {
        service_date: date("2025-10-21"),
        mileage: Some(57_151),
        wear_readings: Some(readings),
        ..Default::default()
    }
}

#[tokio::test]
async fn record_service_writes_its_readings_and_delete_asks_what_happened() {
    use crate::services::service_record::{self, CompletedWork};

    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let mut rear = new_reading(None);
    rear.axle = Axle::Rear;
    rear.reading = Measure::new(2.0, MM);
    rear.limit = Some(Measure::new(3.0, MM));
    let created = service_record::create(
        &db,
        vid,
        service_with_readings(vec![new_reading(None), rear]),
    )
    .await
    .unwrap();
    assert_eq!(created.wear_readings.len(), 2);
    assert!(
        created
            .wear_readings
            .iter()
            .all(|r| r.odometer == Some(57_151)
                && r.measured_on == date::<NaiveDate>("2025-10-21")
                && r.service_id == Some(created.record.id))
    );
    let fetched = service_record::get(&db, vid, created.record.id)
        .await
        .unwrap();
    assert_eq!(fetched.wear_readings, created.wear_readings);
    let listed = service_record::list_filtered(
        &db,
        vid,
        crate::inputs::service_record::ServiceFilter::default(),
    )
    .await
    .unwrap();
    assert_eq!(listed[0].wear_readings, created.wear_readings);

    // A service with nothing recorded as done goes with its readings.
    let ids: Vec<i32> = created.wear_readings.iter().map(|r| r.id).collect();
    let deleted = service_record::delete(&db, vid, created.record.id, None)
        .await
        .unwrap();
    assert_eq!(deleted.wear_reading_ids, ids);
    crate::test_support::assert_empty(&list(&db, vid, WearFilter::default()).await.unwrap());

    // Work done another way keeps the readings, with no service.
    let kept = service_record::create(&db, vid, service_with_readings(vec![new_reading(None)]))
        .await
        .unwrap();
    let deleted = service_record::delete(
        &db,
        vid,
        kept.record.id,
        Some(CompletedWork::DoneAnotherWay),
    )
    .await
    .unwrap();
    assert_eq!(deleted.wear_reading_ids, vec![kept.wear_readings[0].id]);
    let left = list(&db, vid, WearFilter::default()).await.unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].service_id, None);
    assert_eq!(left[0].measured_on, date::<NaiveDate>("2025-10-21"));
}

#[tokio::test]
async fn a_bad_reading_on_record_service_writes_nothing() {
    use crate::services::service_record;

    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let mut bad = new_reading(None);
    bad.reading = Measure::new(f64::NAN, MM);
    let err = service_record::create(
        &db,
        vid,
        service_with_readings(vec![new_reading(None), bad]),
    )
    .await
    .unwrap_err();
    assert_eq!(err.to_string(), "reading: must be a length of zero or more");
    crate::test_support::assert_empty(
        &service_record::list_filtered(
            &db,
            vid,
            crate::inputs::service_record::ServiceFilter::default(),
        )
        .await
        .unwrap(),
    );
    crate::test_support::assert_empty(&list(&db, vid, WearFilter::default()).await.unwrap());
}

#[test]
fn readings_with_no_odometer_and_no_log_say_so() {
    let mut a = pads(1, Axle::Front, 9.0, "2025-01-01", 0);
    let mut b = pads(2, Axle::Front, 7.0, "2026-01-01", 0);
    a.odometer = None;
    b.odometer = None;
    let out = judge(vec![a, b], &BTreeMap::new());
    assert_eq!(out[0].readings_in_series, 2);
    assert_eq!(
        out[0].note.as_deref(),
        Some("The readings carry no odometer and none can be estimated from the mileage log")
    );
}

#[test]
fn the_installing_services_own_readings_measured_the_old_part() {
    let mut installs = BTreeMap::new();
    installs.insert(
        WearTag::BrakePads(Axle::Front),
        Install {
            day: date("2026-11-20"),
            service_id: Some(7),
        },
    );
    let mut worn = pads(1, Axle::Front, 2.0, "2026-11-20", 49_000);
    worn.service_id = Some(7);
    let mut fresh = pads(2, Axle::Front, 12.0, "2026-11-20", 49_000);
    fresh.service_id = Some(8);
    let out = judge(vec![worn, fresh], &installs);
    assert_eq!(out[0].latest, Measure::new(12.0, MM));
    assert_eq!(out[0].readings_in_series, 1);

    let mut only_worn = pads(1, Axle::Front, 2.0, "2026-11-20", 49_000);
    only_worn.service_id = Some(7);
    crate::test_support::assert_empty(&judge(vec![only_worn], &installs));
}

#[test]
fn the_estimate_is_never_below_the_newest_reading() {
    // The log puts the car at 0, but a reading was taken at 50,000: 9 mm at
    // 40,000 and 3.5 mm at 50,000 leave 0.5 mm, about 909 miles.
    let out = outlook_from(
        vec![
            pads(1, Axle::Front, 9.0, "2025-01-01", 40_000),
            pads(2, Axle::Front, 3.5, "2026-01-01", 50_000),
        ],
        &BTreeMap::new(),
        &[],
        0,
        0.0,
        date("2026-11-26"),
    );
    assert_eq!(out[0].miles_left, Some(909));
    assert_eq!(out[0].status, DueStatus::DueSoon);
}

#[test]
fn a_stated_odometer_wins_over_an_estimate_on_the_same_day() {
    // The log says 45,000 that day; the rear reading says 50,000.
    let mut front = tread(1, Axle::Front, 6.0, "2026-01-01", 0);
    front.odometer = None;
    let out = outlook_from(
        vec![
            tread(0, Axle::Front, 10.0, "2025-01-01", 40_000),
            front,
            tread(2, Axle::Rear, 7.0, "2026-01-01", 50_000),
        ],
        &BTreeMap::new(),
        &[(date("2026-01-01"), 45_000)],
        50_000,
        20.0,
        date("2026-11-26"),
    );
    // 10/32 to 6/32 over 10,000 miles is 4/32 per 10,000.
    assert_eq!(out[0].wear_per_10k_miles, Some(Measure::new(4.0, IN32)));
}

#[tokio::test]
async fn a_corrected_service_moves_the_readings_that_took_its_day_and_odometer() {
    use crate::{inputs::service_record::UpdateServiceRecord, services::service_record};

    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let mut own = new_reading(None);
    own.odometer = Some(57_000);
    let created = service_record::create(
        &db,
        vid,
        service_with_readings(vec![new_reading(None), own]),
    )
    .await
    .unwrap();
    service_record::update(
        &db,
        vid,
        created.record.id,
        UpdateServiceRecord {
            service_date: Some(date("2025-10-30")),
            mileage: Some(Some(57_153)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let readings = list(&db, vid, WearFilter::default()).await.unwrap();
    let by_id = |id: i32| readings.iter().find(|r| r.id == id).unwrap();
    let taken = by_id(created.wear_readings[0].id);
    assert_eq!(taken.measured_on, date::<NaiveDate>("2025-10-30"));
    assert_eq!(taken.odometer, Some(57_153));
    let kept = by_id(created.wear_readings[1].id);
    assert_eq!(kept.measured_on, date::<NaiveDate>("2025-10-30"));
    assert_eq!(kept.odometer, Some(57_000));
}

#[tokio::test]
async fn a_future_dated_service_keeps_its_readings() {
    use crate::services::service_record;

    let db = test_db().await;
    let vid = seed_vehicle(&db).await;
    let mut input = service_with_readings(vec![new_reading(None)]);
    input.service_date = crate::services::today() + chrono::Duration::days(1);
    let created = service_record::create(&db, vid, input).await.unwrap();
    assert_eq!(created.wear_readings.len(), 1);
}
