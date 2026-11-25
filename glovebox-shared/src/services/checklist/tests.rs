use super::*;
use crate::{
    entities::part::PartStatus,
    inputs::{
        appointment::NewAppointment,
        need::{NewNeed, NewOption, OrderDetails, ReturnInput, UpdateNeed, UpdateOption},
        part::{NewPart, NewPartContent},
        todo::NewTodo,
    },
    services::{need, part as part_svc},
    test_support::{VehicleFixture, add_job, date, set_job_status, test_db},
};

async fn new_need(db: &DatabaseConnection, vehicle_id: i32, job_id: i32, name: &str) -> i32 {
    need::add(
        db,
        vehicle_id,
        job_id,
        NewNeed {
            name: name.into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id
}

async fn new_option(
    db: &DatabaseConnection,
    vehicle_id: i32,
    need_id: i32,
    description: &str,
    part_number: Option<&str>,
) -> i32 {
    need::add_option(
        db,
        vehicle_id,
        need_id,
        NewOption {
            description: description.into(),
            part_number: part_number.map(Into::into),
            seller: Some("FCP Euro".into()),
            price_cents: Some(8_499),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id
}

// A need the owner will buy: supplied by the owner with an option chosen.
async fn to_order_need(
    db: &DatabaseConnection,
    vehicle_id: i32,
    job_id: i32,
    name: &str,
    description: &str,
) -> i32 {
    let id = new_need(db, vehicle_id, job_id, name).await;
    let option_id = new_option(db, vehicle_id, id, description, None).await;
    set_supplier(db, vehicle_id, id, Supplier::Owner).await;
    need::decide(db, vehicle_id, id, Some(option_id), Some("fits".into()))
        .await
        .unwrap();
    id
}

// A to-order need whose only option is `option`.
async fn to_order_need_with(
    db: &DatabaseConnection,
    vehicle_id: i32,
    job_id: i32,
    name: &str,
    option: NewOption,
) -> i32 {
    let id = new_need(db, vehicle_id, job_id, name).await;
    let option_id = need::add_option(db, vehicle_id, id, option)
        .await
        .unwrap()
        .id;
    set_supplier(db, vehicle_id, id, Supplier::Owner).await;
    need::decide(db, vehicle_id, id, Some(option_id), None)
        .await
        .unwrap();
    id
}

async fn set_supplier(db: &DatabaseConnection, vehicle_id: i32, need_id: i32, who: Supplier) {
    need::update(
        db,
        vehicle_id,
        need_id,
        UpdateNeed {
            supplied_by: Some(Some(who)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

async fn order(db: &DatabaseConnection, vehicle_id: i32, need_id: i32, on: &str) {
    need::mark_ordered(
        db,
        vehicle_id,
        need_id,
        OrderDetails {
            purchase_date: Some(date(on)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

async fn appointment_id(db: &DatabaseConnection, vehicle_id: i32) -> i32 {
    appointment::create(
        db,
        vehicle_id,
        NewAppointment {
            purpose: "60k service".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id
}

fn part_with(
    seller: Option<&str>,
    date: Option<&str>,
    cost: Option<i32>,
    order_ref: Option<&str>,
) -> part::Model {
    part::Model {
        id: 1,
        vehicle_id: 1,
        name: "Part".into(),
        manufacturer: None,
        part_number: None,
        oe_part_number_replaced: None,
        seller: seller.map(Into::into),
        purchase_date: date.map(crate::test_support::date),
        cost_cents: cost,
        cost_currency: None,
        status: PartStatus::Purchased,
        installed_date: None,
        installed_odometer: None,
        installed_service_id: None,
        replaced_date: None,
        replaced_odometer: None,
        notes: None,
        created_at: chrono::DateTime::default(),
        updated_at: chrono::DateTime::default(),
        manufacturer_url: None,
        retailer_url: None,
        location: None,
        warranty_expires_on: None,
        warranty_expires_miles: None,
        supplied_by: Supplier::Owner,
        received_on: None,
        returned_on: None,
        return_reason: None,
        order_ref: order_ref.map(Into::into),
        returned_from_need_id: None,
    }
}

#[test]
fn purchase_summary_reads_as_a_sentence() {
    for (part, expected) in [
        (
            part_with(
                Some("FCP Euro"),
                Some("2026-11-13"),
                Some(16_200),
                Some("FCP-10458"),
            ),
            Some("Bought from FCP Euro on 2026-11-13 for $162.00, order FCP-10458"),
        ),
        (
            part_with(Some("FCP Euro"), None, Some(2_150), None),
            Some("Bought from FCP Euro for $21.50"),
        ),
        (
            part_with(None, Some("2026-11-13"), None, None),
            Some("Bought on 2026-11-13"),
        ),
        (
            part_with(None, None, None, Some("A-1")),
            Some("Bought, order A-1"),
        ),
        (part_with(None, None, None, None), None),
    ] {
        assert_eq!(purchase_summary(&part).as_deref(), expected);
    }
}

#[tokio::test]
async fn step_detail_is_the_summary_without_the_leading_name() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let job = add_job(&db, v, "Oil change", None).await;
    new_need(&db, v, job, "Oil: 5W-30").await;

    let list = for_unscheduled(&db, v).await.unwrap();

    let row = &list.next_steps.decide[0];
    assert_eq!(row.summary, "Oil: 5W-30: no part chosen");
    assert_eq!(row.detail, "no part chosen");
}

#[tokio::test]
async fn checks_name_the_need_and_job_even_when_the_name_has_a_colon() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let job = add_job(&db, v, "Oil change", None).await;
    let n = new_need(&db, v, job, "Oil: 5W-30").await;
    let unsure = need::add_option(
        &db,
        v,
        n,
        NewOption {
            description: "Any oil".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    set_supplier(&db, v, n, Supplier::Owner).await;
    need::decide(&db, v, n, Some(unsure), None).await.unwrap();

    let list = for_unscheduled(&db, v).await.unwrap();

    assert_eq!(list.checks.len(), 1);
    let check = &list.checks[0];
    assert_eq!(check.need_id, n);
    assert_eq!(check.need_name, "Oil: 5W-30");
    assert_eq!(check.job_title, "Oil change");
    assert_eq!(check.note, "Fitment of the chosen option is not confirmed.");
    assert_eq!(
        list.warnings(),
        ["Oil: 5W-30: Fitment of the chosen option is not confirmed."]
    );
}

fn rows(group: &[StepRow]) -> Vec<(&str, &str)> {
    group
        .iter()
        .map(|r| (r.name.as_str(), r.summary.as_str()))
        .collect()
}

#[tokio::test]
async fn next_steps_group_by_stage_in_order() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let a = appointment_id(&db, v).await;
    let job = add_job(&db, v, "60k service", Some(a)).await;
    new_need(&db, v, job, "Rear pads").await;
    to_order_need_with(
        &db,
        v,
        job,
        "Rear rotors",
        NewOption {
            description: "Rear rotor, solid disc".into(),
            manufacturer: Some("OE".into()),
            part_number: Some("5Q0615601E".into()),
            seller: Some("FCP Euro".into()),
            ..Default::default()
        },
    )
    .await;
    let front_pads = to_order_need(&db, v, job, "Front pads", "Textar pads").await;
    order(&db, v, front_pads, "2026-11-14").await;
    let front_rotors = to_order_need(&db, v, job, "Front rotors", "Zimmermann rotors").await;
    order(&db, v, front_rotors, "2026-11-06").await;
    need::mark_received(&db, v, front_rotors, Some(date("2026-11-09")))
        .await
        .unwrap();
    crate::services::todo::add(
        &db,
        v,
        NewTodo {
            title: "Bring wheel locks".into(),
            appointment_id: Some(a),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let list = for_appointment(&db, v, a, crate::services::today())
        .await
        .unwrap();
    assert_eq!(list.appointment.as_ref().unwrap().appointment.id, a);
    let steps = &list.next_steps;
    assert_eq!(
        rows(&steps.decide),
        [("Rear pads", "Rear pads: no part chosen")]
    );
    assert_eq!(
        rows(&steps.order),
        [(
            "Rear rotors",
            "Rear rotors: order OE 5Q0615601E from FCP Euro"
        )]
    );
    assert_eq!(
        rows(&steps.arriving),
        [(
            "Front pads",
            "Front pads: ordered from FCP Euro on 2026-11-14"
        )]
    );
    assert_eq!(steps.done.len(), 1);
    assert_eq!(steps.done[0].name, "Front rotors");
    assert_eq!(steps.done[0].stage, Stage::Received);
    assert_eq!(steps.todos.len(), 1);
    assert_eq!(steps.decide[0].job_title, "60k service");
    assert_eq!(list.jobs.len(), 1);
    assert_eq!(list.jobs[0].needs.len(), 4);
}

#[tokio::test]
async fn undecided_summary_says_what_is_missing() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let a = appointment_id(&db, v).await;
    let job = add_job(&db, v, "Brakes", Some(a)).await;
    let n = new_need(&db, v, job, "Rear pads").await;
    let option_id = new_option(&db, v, n, "Textar pads", None).await;
    need::decide(&db, v, n, Some(option_id), None)
        .await
        .unwrap();
    let list = for_appointment(&db, v, a, crate::services::today())
        .await
        .unwrap();
    assert_eq!(
        rows(&list.next_steps.decide),
        [("Rear pads", "Rear pads: who supplies it is not decided")]
    );
}

#[tokio::test]
async fn shop_supplied_need_is_done_not_open() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let a = appointment_id(&db, v).await;
    let job = add_job(&db, v, "Brakes", Some(a)).await;
    let n = new_need(&db, v, job, "Rear pads").await;
    set_supplier(&db, v, n, Supplier::Shop).await;
    let list = for_appointment(&db, v, a, crate::services::today())
        .await
        .unwrap();
    let steps = &list.next_steps;
    assert!(steps.decide.is_empty() && steps.order.is_empty() && steps.arriving.is_empty());
    assert_eq!(steps.done.len(), 1);
    assert_eq!(steps.done[0].stage, Stage::ShopSupplies);
    assert_eq!(list.appointment.unwrap().needs_open, 0);
}

#[tokio::test]
async fn dropped_job_needs_appear_under_the_job_only() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let a = appointment_id(&db, v).await;
    let job = add_job(&db, v, "Brakes", Some(a)).await;
    new_need(&db, v, job, "Rear pads").await;
    set_job_status(&db, v, job, JobStatus::Dropped).await;
    let list = for_appointment(&db, v, a, crate::services::today())
        .await
        .unwrap();
    let steps = &list.next_steps;
    assert!(
        steps.decide.is_empty()
            && steps.order.is_empty()
            && steps.arriving.is_empty()
            && steps.done.is_empty()
    );
    assert_eq!(list.jobs[0].needs.len(), 1);
    assert_eq!(list.appointment.unwrap().needs_open, 0);
}

#[tokio::test]
async fn done_job_needs_go_to_done_even_when_undecided() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let a = appointment_id(&db, v).await;
    let job = add_job(&db, v, "Brakes", Some(a)).await;
    new_need(&db, v, job, "Rear pads").await;
    to_order_need(&db, v, job, "Rear rotors", "OE rotors").await;
    set_job_status(&db, v, job, JobStatus::Done).await;
    let list = for_appointment(&db, v, a, crate::services::today())
        .await
        .unwrap();
    let steps = &list.next_steps;
    assert!(steps.decide.is_empty() && steps.order.is_empty() && steps.arriving.is_empty());
    assert_eq!(steps.done.len(), 2);
    assert_eq!(steps.done[0].stage, Stage::Undecided);
    assert_eq!(list.appointment.unwrap().needs_open, 0);
}

#[tokio::test]
async fn order_summary_names_the_option_by_part_number_or_description() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let a = appointment_id(&db, v).await;
    let job = add_job(&db, v, "Brakes", Some(a)).await;
    let option = |description: &str, maker: Option<&str>, number: Option<&str>, seller| NewOption {
        description: description.into(),
        manufacturer: maker.map(Into::into),
        part_number: number.map(Into::into),
        seller,
        ..Default::default()
    };
    let fcp = || Some("FCP Euro".to_string());
    to_order_need_with(
        &db,
        v,
        job,
        "Rear rotors",
        option("Rear rotor", Some("Zimmermann"), Some("600.3241.20"), fcp()),
    )
    .await;
    to_order_need_with(
        &db,
        v,
        job,
        "Wiper blades",
        option("Aero blades", None, Some("A123"), fcp()),
    )
    .await;
    to_order_need_with(
        &db,
        v,
        job,
        "Cabin filter",
        option("Mann cabin filter", Some("Mann"), None, fcp()),
    )
    .await;
    to_order_need_with(
        &db,
        v,
        job,
        "Oil",
        option("5W-40 oil", Some("Liqui Moly"), Some("2332"), None),
    )
    .await;
    let list = for_appointment(&db, v, a, crate::services::today())
        .await
        .unwrap();
    assert_eq!(
        rows(&list.next_steps.order),
        [
            (
                "Rear rotors",
                "Rear rotors: order Zimmermann 600.3241.20 from FCP Euro"
            ),
            ("Wiper blades", "Wiper blades: order A123 from FCP Euro"),
            (
                "Cabin filter",
                "Cabin filter: order Mann cabin filter from FCP Euro"
            ),
            ("Oil", "Oil: order Liqui Moly 2332"),
        ]
    );
}

#[tokio::test]
async fn order_summary_counts_a_need_for_more_than_one() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let a = appointment_id(&db, v).await;
    let job = add_job(&db, v, "Brakes", Some(a)).await;
    let n = to_order_need(&db, v, job, "Rear rotors", "Rear rotor").await;
    need::update(
        &db,
        v,
        n,
        UpdateNeed {
            quantity: Some(2),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let list = for_appointment(&db, v, a, crate::services::today())
        .await
        .unwrap();

    assert_eq!(
        rows(&list.next_steps.order),
        [(
            "Rear rotors",
            "Rear rotors: order 2 \u{d7} Rear rotor from FCP Euro"
        )]
    );
}

#[tokio::test]
async fn a_shop_part_with_no_decision_is_not_noted() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let job = add_job(&db, v, "Brakes", None).await;
    let n = new_need(&db, v, job, "Brake fluid").await;
    set_supplier(&db, v, n, Supplier::Shop).await;
    let fluid = part_svc::create(
        &db,
        v,
        NewPart {
            name: "DOT4".into(),
            supplied_by: Some(Supplier::Shop),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    need::link_part(&db, v, n, fluid.id, None).await.unwrap();

    let view = for_job(&db, v, job).await.unwrap();

    crate::test_support::assert_empty(&view.needs[0].notes);
}

#[tokio::test]
async fn open_needs_counts_only_a_planned_jobs_open_needs() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let job = add_job(&db, v, "Brakes", None).await;
    new_need(&db, v, job, "Pads").await;
    to_order_need(&db, v, job, "Rotors", "Rotor").await;
    let shop = new_need(&db, v, job, "Fluid").await;
    set_supplier(&db, v, shop, Supplier::Shop).await;

    assert_eq!(for_job(&db, v, job).await.unwrap().open_needs(), 2);
    set_job_status(&db, v, job, JobStatus::Done).await;
    assert_eq!(for_job(&db, v, job).await.unwrap().open_needs(), 0);
}

#[tokio::test]
async fn options_note_counts_options_and_rejections() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let job = add_job(&db, v, "Brakes", None).await;
    let n = new_need(&db, v, job, "Pads").await;
    let note = || async { for_job(&db, v, job).await.unwrap().needs[0].options_note() };
    assert_eq!(note().await, "0 options researched");
    let first = new_option(&db, v, n, "Pads A", None).await;
    assert_eq!(note().await, "1 option researched");
    new_option(&db, v, n, "Pads B", None).await;
    need::update_option(
        &db,
        v,
        first,
        UpdateOption {
            rejected_reason: Some(Some("noisy".into())),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(note().await, "2 options researched, 1 rejected");
}

#[tokio::test]
async fn a_whitespace_manufacturer_leaves_no_leading_space_in_the_label() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let a = appointment_id(&db, v).await;
    let job = add_job(&db, v, "Brakes", Some(a)).await;
    to_order_need_with(
        &db,
        v,
        job,
        "Wiper blades",
        NewOption {
            description: "Aero blades".into(),
            manufacturer: Some("   ".into()),
            part_number: Some("A123".into()),
            ..Default::default()
        },
    )
    .await;
    let list = for_appointment(&db, v, a, crate::services::today())
        .await
        .unwrap();
    assert_eq!(list.next_steps.order[0].summary, "Wiper blades: order A123");
}

#[tokio::test]
async fn done_group_sentences_are_exact() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let a = appointment_id(&db, v).await;
    let job = add_job(&db, v, "Brakes", Some(a)).await;
    let shop = new_need(&db, v, job, "Rear pads").await;
    set_supplier(&db, v, shop, Supplier::Shop).await;
    let received = to_order_need(&db, v, job, "Front pads", "Textar pads").await;
    order(&db, v, received, "2026-11-06").await;
    need::mark_received(&db, v, received, Some(date("2026-11-09")))
        .await
        .unwrap();
    let installed = to_order_need(&db, v, job, "Front rotors", "Zimmermann rotors").await;
    order(&db, v, installed, "2026-11-06").await;
    let linked = need::require(&db, v, installed).await.unwrap();
    let mut part: part::ActiveModel = part::Entity::find_by_id(linked.part_id.unwrap())
        .one(&db)
        .await
        .unwrap()
        .unwrap()
        .into();
    part.status = Set(PartStatus::Installed);
    part.update(&db).await.unwrap();
    let list = for_appointment(&db, v, a, crate::services::today())
        .await
        .unwrap();
    assert_eq!(
        rows(&list.next_steps.done),
        [
            ("Rear pads", "Rear pads: the shop supplies it"),
            ("Front pads", "Front pads: received on 2026-11-09"),
            ("Front rotors", "Front rotors: installed"),
        ]
    );

    let open = new_need(&db, v, job, "Wipers").await;
    let _ = open;
    set_job_status(&db, v, job, JobStatus::Done).await;
    let list = for_appointment(&db, v, a, crate::services::today())
        .await
        .unwrap();
    let wipers = list.next_steps.done.last().unwrap();
    assert_eq!(wipers.summary, "Wipers: the job is done");
}

const UNCONFIRMED: &str = "Fitment of the chosen option is not confirmed.";
const NO_DECISION: &str = NO_DECISION_NOTE;

#[tokio::test]
async fn unconfirmed_fitment_and_mismatched_part_number_warn() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let a = appointment_id(&db, v).await;
    let job = add_job(&db, v, "Brakes", Some(a)).await;
    let n = new_need(&db, v, job, "Rear rotors").await;
    let option_id = new_option(&db, v, n, "OE rotors", Some("5Q0615601E")).await;
    set_supplier(&db, v, n, Supplier::Owner).await;
    need::decide(&db, v, n, Some(option_id), None)
        .await
        .unwrap();
    let bought = part_svc::create(
        &db,
        v,
        NewPart {
            name: "Rotors".into(),
            part_number: Some("5Q0615601F".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    need::link_part(&db, v, n, bought.id, None).await.unwrap();

    let list = for_appointment(&db, v, a, crate::services::today())
        .await
        .unwrap();
    let mismatch =
        "The linked part's number (5Q0615601F) differs from the chosen option's (5Q0615601E).";
    assert_eq!(list.jobs[0].needs[0].notes, [UNCONFIRMED, mismatch]);
    assert_eq!(
        list.warnings(),
        [
            format!("Rear rotors: {UNCONFIRMED}"),
            format!("Rear rotors: {mismatch}")
        ]
    );
}

#[tokio::test]
async fn confirmed_fitment_and_matching_numbers_do_not_warn() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let a = appointment_id(&db, v).await;
    let job = add_job(&db, v, "Brakes", Some(a)).await;
    let n = new_need(&db, v, job, "Rear rotors").await;
    let option_id = new_option(&db, v, n, "OE rotors", Some("5q0615601e")).await;
    need::update_option(
        &db,
        v,
        option_id,
        UpdateOption {
            fitment: Some(Fitment::Confirmed),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    set_supplier(&db, v, n, Supplier::Owner).await;
    need::decide(&db, v, n, Some(option_id), None)
        .await
        .unwrap();
    let bought = part_svc::create(
        &db,
        v,
        NewPart {
            name: "Rotors".into(),
            part_number: Some("5Q0615601E".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    need::link_part(&db, v, n, bought.id, None).await.unwrap();
    let list = for_appointment(&db, v, a, crate::services::today())
        .await
        .unwrap();
    assert!(list.warnings().is_empty(), "{:?}", list.warnings());
}

#[tokio::test]
async fn linked_part_without_decision_is_noted() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let a = appointment_id(&db, v).await;
    let job = add_job(&db, v, "Brakes", Some(a)).await;
    let n = new_need(&db, v, job, "Rear pads").await;
    let bought = part_svc::create(
        &db,
        v,
        NewPart {
            name: "Pads".into(),
            seller: Some("FCP Euro".into()),
            purchase_date: Some(date("2026-11-06")),
            cost_cents: Some(8_499),
            order_ref: Some("12345".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    need::link_part(&db, v, n, bought.id, None).await.unwrap();
    let view = for_job(&db, v, job).await.unwrap();
    let need_view = &view.needs[0];
    assert_eq!(need_view.notes, [NO_DECISION]);
    assert_eq!(need_view.stage, Stage::Ordered);
    assert_eq!(
        need_view.purchase_summary.as_deref(),
        Some("Bought from FCP Euro on 2026-11-06 for $84.99, order 12345")
    );
}

#[tokio::test]
async fn a_linked_kit_carries_its_contents() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let a = appointment_id(&db, v).await;
    let job = add_job(&db, v, "Oil change", Some(a)).await;
    let n = new_need(&db, v, job, "Oil and filter").await;
    let plain = new_need(&db, v, job, "Wipers").await;
    let kit = part_svc::create(
        &db,
        v,
        NewPart {
            name: "Oil change kit".into(),
            contents: vec![NewPartContent {
                name: "Oil filter".into(),
                part_number: Some("HU 7020 z".into()),
                ..Default::default()
            }],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let wipers = part_svc::create(
        &db,
        v,
        NewPart {
            name: "Wipers".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    need::link_part(&db, v, n, kit.id, None).await.unwrap();
    need::link_part(&db, v, plain, wipers.id, None)
        .await
        .unwrap();

    let list = for_appointment(&db, v, a, crate::services::today())
        .await
        .unwrap();

    let needs = &list.jobs[0].needs;
    let kit_view = needs[0].part.as_ref().unwrap();
    assert_eq!(
        kit_view.contents[0].part_number.as_deref(),
        Some("HU 7020 z")
    );
    crate::test_support::assert_empty(&needs[1].part.as_ref().unwrap().contents);
}

#[tokio::test]
async fn purchase_summary_uses_only_the_parts_present() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let job = add_job(&db, v, "Brakes", None).await;
    let n = new_need(&db, v, job, "Rear pads").await;
    let bought = part_svc::create(
        &db,
        v,
        NewPart {
            name: "Pads".into(),
            cost_cents: Some(5),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    need::link_part(&db, v, n, bought.id, None).await.unwrap();
    let view = for_job(&db, v, job).await.unwrap();
    assert_eq!(
        view.needs[0].purchase_summary.as_deref(),
        Some("Bought for $0.05")
    );
}

#[tokio::test]
async fn unscheduled_lists_only_planned_jobs_with_needs() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let a = appointment_id(&db, v).await;
    let with_needs = add_job(&db, v, "Wipers", None).await;
    new_need(&db, v, with_needs, "Blades").await;
    add_job(&db, v, "Wash", None).await;
    let scheduled = add_job(&db, v, "Brakes", Some(a)).await;
    new_need(&db, v, scheduled, "Pads").await;
    let done = add_job(&db, v, "Oil", None).await;
    new_need(&db, v, done, "Filter").await;
    set_job_status(&db, v, done, JobStatus::Done).await;
    crate::services::todo::add(
        &db,
        v,
        NewTodo {
            title: "Buy wiper fluid".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let list = for_unscheduled(&db, v).await.unwrap();
    assert!(list.appointment.is_none());
    let titles: Vec<&str> = list.jobs.iter().map(|j| j.job.title.as_str()).collect();
    assert_eq!(titles, ["Wipers"]);
    assert_eq!(
        rows(&list.next_steps.decide),
        [("Blades", "Blades: no part chosen")]
    );
    assert_eq!(list.todos.len(), 1);
    assert_eq!(list.next_steps.todos.len(), 1);
}

#[tokio::test]
async fn wrong_vehicle_ids_read_as_missing() {
    let db = test_db().await;
    let v1 = VehicleFixture::new().insert_id(&db).await;
    let v2 = VehicleFixture::new().name("Other").insert_id(&db).await;
    let a = appointment_id(&db, v1).await;
    let job = add_job(&db, v1, "Brakes", Some(a)).await;
    let missing_appointment = for_appointment(&db, v1, 99_999, crate::services::today())
        .await
        .unwrap_err();
    let wrong_appointment = for_appointment(&db, v2, a, crate::services::today())
        .await
        .unwrap_err();
    assert!(matches!(wrong_appointment, DomainError::NotFound(_)));
    assert_eq!(
        wrong_appointment.to_string().replace(&a.to_string(), "N"),
        missing_appointment.to_string().replace("99999", "N")
    );
    let missing_job = for_job(&db, v1, 99_999).await.unwrap_err();
    let wrong_job = for_job(&db, v2, job).await.unwrap_err();
    assert!(matches!(wrong_job, DomainError::NotFound(_)));
    assert_eq!(
        wrong_job.to_string().replace(&job.to_string(), "N"),
        missing_job.to_string().replace("99999", "N")
    );
}

async fn set_decided_on(db: &DatabaseConnection, need_id: i32, on: &str) {
    let found = job_need::Entity::find_by_id(need_id)
        .one(db)
        .await
        .unwrap()
        .unwrap();
    let mut active: job_need::ActiveModel = found.into();
    active.decided_on = Set(Some(date(on)));
    active.update(db).await.unwrap();
}

#[tokio::test]
async fn past_decisions_finds_by_need_name_and_part_number_across_vehicles() {
    let db = test_db().await;
    let gti = VehicleFixture::new().name("GTI").insert_id(&db).await;
    let alltrack = VehicleFixture::new().name("Alltrack").insert_id(&db).await;
    let gti_job = add_job(&db, gti, "Brakes", None).await;
    let older = to_order_need(&db, gti, gti_job, "Brake pads", "Textar pads").await;
    set_decided_on(&db, older, "2026-10-01").await;
    let alltrack_job = add_job(&db, alltrack, "Rotors", None).await;
    let newer = new_need(&db, alltrack, alltrack_job, "Rear rotors").await;
    let option_id = new_option(&db, alltrack, newer, "OE", Some("5Q0615601E")).await;
    need::decide(
        &db,
        alltrack,
        newer,
        Some(option_id),
        Some("OE fits".into()),
    )
    .await
    .unwrap();
    set_decided_on(&db, newer, "2026-11-01").await;

    let by_name = past_decisions(&db, "BRAKE").await.unwrap();
    assert_eq!(by_name.len(), 1);
    assert_eq!(by_name[0].vehicle_name, "GTI");
    assert_eq!(by_name[0].job_title, "Brakes");
    assert_eq!(
        by_name[0].chosen.as_ref().unwrap().description,
        "Textar pads"
    );

    let by_number = past_decisions(&db, "5q0615601e").await.unwrap();
    assert_eq!(by_number.len(), 1);
    assert_eq!(by_number[0].vehicle_name, "Alltrack");
    assert_eq!(
        by_number[0].need.decision_reason.as_deref(),
        Some("OE fits")
    );

    let both = past_decisions(&db, "r").await.unwrap();
    let ids: Vec<i32> = both.iter().map(|d| d.need.id).collect();
    assert_eq!(ids, [newer, older]);

    // Wildcard characters in the query match themselves only.
    assert!(past_decisions(&db, "%").await.unwrap().is_empty());
    assert!(past_decisions(&db, "_").await.unwrap().is_empty());
    assert!(
        past_decisions(&db, "no such part")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn past_decisions_includes_rejected_options_and_returned_parts() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let job = add_job(&db, v, "Brakes", None).await;
    let n = new_need(&db, v, job, "Rear pads").await;
    let wrong = new_option(&db, v, n, "Cheap pads", None).await;
    let right = new_option(&db, v, n, "Textar pads", None).await;
    need::update_option(
        &db,
        v,
        wrong,
        UpdateOption {
            rejected_reason: Some(Some("glazes quickly".into())),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    set_supplier(&db, v, n, Supplier::Owner).await;
    need::decide(&db, v, n, Some(right), Some("OE-grade".into()))
        .await
        .unwrap();
    order(&db, v, n, "2026-11-06").await;
    need::return_part(
        &db,
        v,
        n,
        ReturnInput {
            reason: "wrong box".into(),
            option_was_wrong: false,
        },
    )
    .await
    .unwrap();

    let found = past_decisions(&db, "rear pads").await.unwrap();
    assert_eq!(found.len(), 1);
    let d = &found[0];
    assert_eq!(d.rejected.len(), 1);
    assert_eq!(d.rejected[0].id, wrong);
    assert_eq!(
        d.rejected[0].rejected_reason.as_deref(),
        Some("glazes quickly")
    );
    assert_eq!(d.returned_parts.len(), 1);
    assert_eq!(
        d.returned_parts[0].part.return_reason.as_deref(),
        Some("wrong box")
    );
    assert_eq!(d.chosen.as_ref().unwrap().id, right);
}

#[tokio::test]
async fn past_decisions_keeps_the_reason_for_a_choice_that_proved_wrong() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let job = add_job(&db, v, "Brakes", None).await;
    let n = new_need(&db, v, job, "Rear rotors").await;
    let first = new_option(&db, v, n, "310 mm rotors", None).await;
    set_supplier(&db, v, n, Supplier::Owner).await;
    need::decide(&db, v, n, Some(first), Some("cheapest OE size".into()))
        .await
        .unwrap();
    order(&db, v, n, "2026-11-06").await;
    need::return_part(
        &db,
        v,
        n,
        ReturnInput {
            reason: "car has the 340 mm brakes".into(),
            option_was_wrong: true,
        },
    )
    .await
    .unwrap();

    let found = past_decisions(&db, "rear rotors").await.unwrap();
    let d = &found[0];
    assert!(d.chosen.is_none());
    assert_eq!(d.rejected.len(), 1);
    assert_eq!(
        d.rejected[0].chosen_reason.as_deref(),
        Some("cheapest OE size")
    );
    assert_eq!(
        d.rejected[0].rejected_reason.as_deref(),
        Some("car has the 340 mm brakes")
    );
    assert!(d.rejected[0].chosen_on.is_some());
}

#[tokio::test]
async fn past_decisions_keeps_the_reason_for_a_choice_that_was_ruled_out() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let job = add_job(&db, v, "Brakes", None).await;
    let n = new_need(&db, v, job, "Rear rotors").await;
    let first = new_option(&db, v, n, "272 mm rotors", None).await;
    need::decide(&db, v, n, Some(first), Some("cheapest OE size".into()))
        .await
        .unwrap();
    need::update_option(
        &db,
        v,
        first,
        UpdateOption {
            fitment: Some(Fitment::DoesNotFit),
            fitment_note: Some(Some("The car has the 310 mm brakes.".into())),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let found = past_decisions(&db, "rear rotors").await.unwrap();
    let d = &found[0];
    assert!(d.chosen.is_none());
    assert_eq!(d.rejected.len(), 1, "{d:?}");
    assert_eq!(d.rejected[0].id, first);
    assert_eq!(d.rejected[0].fitment, Fitment::DoesNotFit);
    assert_eq!(
        d.rejected[0].chosen_reason.as_deref(),
        Some("cheapest OE size")
    );
    assert!(d.rejected[0].chosen_on.is_some());
}

#[tokio::test]
async fn blank_query_is_invalid() {
    let db = test_db().await;
    let err = past_decisions(&db, "   ").await.unwrap_err();
    assert!(matches!(err, DomainError::Invalid { .. }), "{err:?}");
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one fixture across two vehicles, then its checks"
)]
async fn home_view_groups_by_vehicle() {
    let db = test_db().await;
    let golf = VehicleFixture::new().name("Golf").insert_id(&db).await;
    let alltrack = VehicleFixture::new().name("Alltrack").insert_id(&db).await;
    let old = VehicleFixture::new().name("Old").insert_id(&db).await;
    crate::services::vehicle::archive(&db, old, crate::inputs::vehicle::VehicleSale::default())
        .await
        .unwrap();

    let shop = crate::test_support::add_shop(&db, "Smith Auto").await;
    let booked = appointment::create(
        &db,
        golf,
        NewAppointment {
            purpose: "60k service".into(),
            planned_date: Some(date("2026-12-01")),
            shop: Some(crate::inputs::shop::ShopChoice::Id(shop)),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let unbooked = appointment::create(
        &db,
        golf,
        NewAppointment {
            purpose: "Tire swap".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let job = add_job(&db, golf, "Brakes", Some(booked)).await;
    new_need(&db, golf, job, "Rear pads").await;
    new_need(&db, golf, job, "Rear rotors").await;
    let todo_input = |title: &str, appointment_id| NewTodo {
        title: title.into(),
        appointment_id,
        ..Default::default()
    };
    crate::services::todo::add(&db, golf, todo_input("Bring wheel locks", Some(booked)))
        .await
        .unwrap();
    crate::services::todo::add(&db, golf, todo_input("Wash the car", None))
        .await
        .unwrap();
    let done = crate::services::todo::add(&db, golf, todo_input("Buy wipers", None))
        .await
        .unwrap();
    crate::services::todo::complete(&db, golf, done.id, None)
        .await
        .unwrap();
    let loose = add_job(&db, golf, "Wiper blades", None).await;
    new_need(&db, golf, loose, "Blades").await;
    add_job(&db, golf, "Needless job", None).await;
    crate::services::todo::add(&db, alltrack, todo_input("Check tire pressure", None))
        .await
        .unwrap();

    let home = home(&db, crate::test_support::date("2026-11-18"))
        .await
        .unwrap();

    let names: Vec<&str> = home.iter().map(|h| h.vehicle.name.as_str()).collect();
    assert_eq!(
        names,
        ["Golf", "Alltrack"],
        "archived vehicles are left out"
    );
    let g = &home[0];
    let shown: Vec<(i32, &str, &str, bool, usize, usize)> = g
        .appointments
        .iter()
        .map(|a| {
            (
                a.id,
                a.purpose.as_str(),
                a.when.as_str(),
                a.dated,
                a.needs_open,
                a.todos_open,
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            (booked, "60k service", "2026-12-01", true, 2, 1),
            (unbooked, "Tire swap", "not booked", false, 0, 0),
        ]
    );
    let shops: Vec<_> = g
        .appointments
        .iter()
        .map(|a| a.shop.as_ref().map(|s| (s.id, s.name.as_str())))
        .collect();
    assert_eq!(shops, [Some((shop, "Smith Auto")), None]);
    let state: Vec<_> = g.appointments.iter().map(|a| (a.phase, a.jobs)).collect();
    assert_eq!(
        state,
        [
            (super::appointment::AppointmentPhase::Booked, 1),
            (super::appointment::AppointmentPhase::Planning, 0)
        ]
    );
    let jobs: Vec<(&str, usize)> = g
        .waiting
        .jobs
        .iter()
        .map(|j| (j.title.as_str(), j.needs_open))
        .collect();
    assert_eq!(
        jobs,
        [("Wiper blades", 1), ("Needless job", 0)],
        "a job with no needs waits too, and a booked job does not"
    );
    let todos: Vec<&str> = g.todos.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(
        todos,
        ["Wash the car"],
        "done and appointment to-dos are left out"
    );
    let a = &home[1];
    assert!(a.appointments.is_empty() && a.waiting.jobs.is_empty());
    assert_eq!(a.todos.len(), 1);
}

#[test]
fn scope_reads_at_most_one_id() {
    assert_eq!(
        scope(Some(3), None).unwrap(),
        ChecklistScope::Appointment(3)
    );
    assert_eq!(scope(None, Some(4)).unwrap(), ChecklistScope::Job(4));
    assert_eq!(scope(None, None).unwrap(), ChecklistScope::Unscheduled);
    let err = scope(Some(3), Some(4)).unwrap_err();
    assert_eq!(
        err.to_string(),
        "appointment_id: pass either appointment_id or work_item_id, not both"
    );
    assert!(matches!(err, DomainError::Invalid { .. }), "{err:?}");
}

fn json(value: &impl Serialize) -> serde_json::Value {
    serde_json::to_value(value).unwrap()
}

#[tokio::test]
async fn get_reads_the_same_as_the_function_for_its_scope() {
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let a = appointment_id(&db, v).await;
    let on_visit = add_job(&db, v, "Brakes", Some(a)).await;
    new_need(&db, v, on_visit, "Pads").await;
    let loose = add_job(&db, v, "Wipers", None).await;
    new_need(&db, v, loose, "Blades").await;

    let got = get(
        &db,
        v,
        ChecklistScope::Appointment(a),
        crate::services::today(),
    )
    .await
    .unwrap();
    let want = for_appointment(&db, v, a, crate::services::today())
        .await
        .unwrap();
    assert_eq!(json(&got), json(&want));
    let got = get(&db, v, ChecklistScope::Job(loose), crate::services::today())
        .await
        .unwrap();
    let want = for_job(&db, v, loose).await.unwrap();
    assert_eq!(json(&got), json(&want));
    let got = get(
        &db,
        v,
        ChecklistScope::Unscheduled,
        crate::services::today(),
    )
    .await
    .unwrap();
    let want = for_unscheduled(&db, v).await.unwrap();
    assert_eq!(json(&got), json(&want));
    assert_eq!(json(&got)["jobs"][0]["job"]["id"], loose);

    let other = VehicleFixture::new().name("Other").insert_id(&db).await;
    for (scope, missing, id) in [
        (
            ChecklistScope::Appointment(a),
            ChecklistScope::Appointment(99_999),
            a,
        ),
        (
            ChecklistScope::Job(loose),
            ChecklistScope::Job(99_999),
            loose,
        ),
    ] {
        let wrong = get(&db, other, scope, crate::services::today())
            .await
            .unwrap_err()
            .to_string();
        let absent = get(&db, v, missing, crate::services::today())
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(
            wrong.replace(&id.to_string(), "N"),
            absent.replace("99999", "N"),
            "{scope:?}"
        );
    }
}

// Two jobs one service completed both name it, and a planned job names none.
#[tokio::test]
async fn a_job_view_names_the_service_that_completed_it() {
    use crate::{inputs::service_record::NewServiceRecord, services::service_record};
    let db = test_db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let pads = add_job(&db, v, "Rear pads", None).await;
    let fluid = add_job(&db, v, "Brake fluid", None).await;
    let planned = add_job(&db, v, "Tires", None).await;
    let service = service_record::create(
        &db,
        v,
        NewServiceRecord {
            service_date: date("2026-10-01"),
            work_item_ids: Some(vec![pads, fluid]),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .record
    .id;

    for job in [pads, fluid] {
        let by = for_job(&db, v, job).await.unwrap().completed_by.unwrap();
        assert_eq!((by.id, by.service_date), (service, date("2026-10-01")));
    }
    let view = for_job(&db, v, planned).await.unwrap();
    assert!(view.completed_by.is_none(), "{:?}", view.completed_by);
}
