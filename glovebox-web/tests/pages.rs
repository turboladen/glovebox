mod common;

use axum::{body::Body, http::StatusCode};
use common::{app, app_with_file, db, get, page, request, send, send_bytes};
use glovebox_shared::{
    db::DatabaseConnection,
    entities::incident::IncidentCategory,
    inputs::{
        appointment::NewAppointment, incident::NewIncident, mileage::NewMileageEntry,
        need::NewNeed, todo::NewTodo, work_item::NewWorkItem,
    },
    services::{
        appointment, incident, mileage, model_template, need,
        research::{self, NewFiledFinding},
        todo, waiting, work_item,
    },
    test_support::{VehicleFixture, date},
};

const CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; font-src 'self'; form-action \
                   'self'; base-uri 'none'; frame-ancestors 'none'";

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one page's fixture, then a check per row"
)]
async fn home_lists_open_appointments_and_jobs_not_scheduled() {
    let db = db().await;
    let v = VehicleFixture::new().name("Golf GTI").insert_id(&db).await;
    let a = appointment::create(
        &db,
        v,
        NewAppointment {
            purpose: "60k service".into(),
            planned_date: Some(date("2026-12-01")),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    appointment::create(
        &db,
        v,
        NewAppointment {
            purpose: "Winter tires".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let booked_job = work_item::create(
        &db,
        v,
        NewWorkItem {
            title: "Brake job".into(),
            appointment_id: Some(a),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    for name in ["Front pads", "Front rotors"] {
        need::add(
            &db,
            v,
            booked_job,
            NewNeed {
                name: name.into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    let loose_job = work_item::create(
        &db,
        v,
        NewWorkItem {
            title: "Replace wiper blades".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    need::add(
        &db,
        v,
        loose_job,
        NewNeed {
            name: "Blades".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    for (title, appointment_id) in [("Bring wheel locks", Some(a)), ("Wash the car", None)] {
        todo::add(
            &db,
            v,
            NewTodo {
                title: title.into(),
                appointment_id,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    let (status, body) = page(app(db, "live"), get("/")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Golf GTI"), "{body}");
    assert!(body.contains("60k service"), "{body}");
    assert!(body.contains("2026-12-01"), "{body}");
    assert!(
        body.contains(
            "<span>Appointment</span><span>Status</span><span>Date</span><span>Shop</span><span \
             class=\"num\">Jobs</span><span class=\"num\">Needs open</span><span \
             class=\"num\">To-dos open</span>"
        ),
        "{body}"
    );
    assert!(
        body.contains(
            r#"<span data-l="Status"><span class="vh">Status </span><span class="pill pill--wait">Booked</span></span>"#
        ),
        "the dated appointment reads booked: {body}"
    );
    assert!(
        body.contains(
            r#"<span data-l="Status"><span class="vh">Status </span><span class="pill pill--plain">Planning</span></span>"#
        ),
        "the undated one reads planning: {body}"
    );
    assert!(
        body.contains(r#"<span class="num" data-l="Jobs"><span class="vh">Jobs </span>1</span>"#),
        "{body}"
    );
    assert!(
        body.contains(
            r#"<span class="num" data-l="Needs open"><span class="vh">Needs open </span><span class="n n--todo">2</span></span>"#
        ),
        "{body}"
    );
    assert!(
        body.contains(
            r#"<span class="num" data-l="To-dos open"><span class="vh">To-dos open </span><span class="n n--todo">1</span></span>"#
        ),
        "{body}"
    );
    // An appointment with nothing open shows plain zeros.
    assert!(
        body.contains(r#"<span class="num" data-l="Needs open"><span class="vh">Needs open </span><span class="n">0</span></span>"#),
        "{body}"
    );
    assert!(body.contains("Winter tires"), "{body}");
    assert!(
        body.contains(r#"<span data-l="Date"><span class="vh">Date </span><span class="empty cap">not booked</span></span>"#),
        "{body}"
    );
    assert!(body.contains("Jobs not scheduled"), "{body}");
    assert!(body.contains("Replace wiper blades"), "{body}");
    assert!(body.contains("<span>Job</span>"), "{body}");
    assert!(
        body.contains(
            r#"<span class="at-job num" data-l="Needs open"><span class="vh">Needs open </span><span class="n n--todo">1</span></span>"#
        ),
        "{body}"
    );
    assert!(body.contains("Wash the car"), "{body}");
    assert!(
        body.contains(r#"<span data-l="Date"><span class="vh">Date </span>2026-12-01</span>"#),
        "{body}"
    );
    assert_eq!(body.matches(">Add a to-do<").count(), 2, "{body}");
    assert!(!body.contains(">Add to-do<"), "{body}");
    assert!(
        !body.contains("Brake job"),
        "a job on an appointment is not unscheduled: {body}"
    );
}

// A GTI on the seeded schedule at 64,000 miles, where the 60k spark plugs are
// overdue.
async fn gti_at_64k(db: &DatabaseConnection) -> i32 {
    gti(db, "2017-03-05", 64_000).await
}

// A GTI on the seeded schedule, bought on `purchased` with 40 miles and read
// at `mileage` on the test clock's day.
async fn gti(db: &DatabaseConnection, purchased: &str, mileage: i32) -> i32 {
    let template = model_template::list(db)
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.model.as_deref() == Some("Golf GTI"))
        .expect("seeded GTI template");
    let v = VehicleFixture::new()
        .name("Golf GTI")
        .model_template_id(template.id)
        .purchase_date(purchased)
        .purchase_mileage(40)
        .insert_id(db)
        .await;
    mileage::create(
        db,
        v,
        NewMileageEntry {
            mileage,
            recorded_at: Some(common::today().into()),
            source: None,
            notes: None,
        },
    )
    .await
    .unwrap();
    v
}

// The waiting lists judge by the page's clock, not the machine's: a GTI
// bought 2024-11-23 has its 24-month air filter due 2026-11-23, ten days
// after the test clock's 2026-11-13.
#[tokio::test]
async fn home_judges_what_is_due_by_the_page_clock() {
    let db = db().await;
    gti(&db, "2024-11-23", 1_000).await;

    let (_, body) = page(app(db, "live"), get("/")).await;

    let block = waiting_block(&body);
    assert!(block.contains("Air Filter"), "{block}");
    assert!(block.contains("in 10 days"), "{block}");
}

// The part of the home page from the first waiting sheet up to Appointments.
fn waiting_block(body: &str) -> &str {
    let start = body.find("Maintenance due").expect("the waiting sheets");
    let end = body.find("<h3>Appointments</h3>").expect("appointments");
    &body[start..end]
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one page's fixture, then a check per row"
)]
async fn home_shows_what_is_waiting_before_the_appointments() {
    let db = db().await;
    let v = gti_at_64k(&db).await;
    let plugs = waiting::waiting(&db, v, common::today())
        .await
        .unwrap()
        .maintenance
        .into_iter()
        .find(|m| m.name == "Spark Plugs")
        .unwrap()
        .schedule_item_id;
    let plug_job = work_item::create(
        &db,
        v,
        NewWorkItem {
            title: "Do the plugs".into(),
            schedule_item_id: Some(plugs),
            est_cost_cents: Some(18000),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let wiper = work_item::create(
        &db,
        v,
        NewWorkItem {
            title: "Rear wiper".into(),
            revisit_by_miles: Some(63000),
            est_cost_cents: Some(2500),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    research::file_finding(
        &db,
        v,
        NewFiledFinding {
            category: "recall".into(),
            title: "Fuel pump".into(),
            description: None,
            source_url: Some("https://www.nhtsa.gov/recalls?nhtsaId=24V123".into()),
            severity: None,
        },
    )
    .await
    .unwrap();
    incident::create(
        &db,
        v,
        NewIncident {
            category: IncidentCategory::ShopObservation,
            title: "Seeping valve cover".into(),
            occurred_at: Some(date("2026-10-02 09:30:00")),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let (status, body) = page(app(db, "live"), get("/")).await;

    assert_eq!(status, StatusCode::OK);
    let block = waiting_block(&body);
    assert!(!block.contains("<form"), "the waiting sheets only show");
    assert!(block.contains("Judged at 64,000 mi"), "{block}");
    assert!(block.contains("Spark Plugs"), "{block}");
    assert!(
        block.contains(r#"<span class="pill pill--bad">Overdue</span>"#),
        "{block}"
    );
    assert!(block.contains("60,040 mi"), "{block}");
    assert!(block.contains("3,960 mi over"), "{block}");
    assert!(
        block.contains(&format!(
            r#"<a href="/vehicles/{v}/jobs/{plug_job}">Job planned</a>"#
        )),
        "{block}"
    );
    let sheet = |from: &str, to: &str| {
        let start = block.find(from).unwrap();
        &block[start..start + block[start..].find(to).unwrap()]
    };
    let due = sheet("Maintenance due", "Jobs not scheduled");
    assert!(
        due.contains("Not planned"),
        "another due item has no job: {due}"
    );
    assert!(!due.contains("on an appointment"), "{due}");
    // A job with no needs is listed, with its estimate and its revisit status.
    let jobs = sheet("Jobs not scheduled", "Open recalls");
    assert!(
        jobs.contains("$25.00") && jobs.contains("$180.00"),
        "{jobs}"
    );
    assert!(
        jobs.contains(r#"<span class="pill pill--bad">Overdue</span>"#),
        "{jobs}"
    );
    assert!(
        block.contains(&format!(
            r#"<a href="/vehicles/{v}/jobs/{wiper}">Rear wiper</a>"#
        )),
        "{block}"
    );
    assert!(
        block.contains("Fuel pump") && block.contains("24V123"),
        "{block}"
    );
    assert!(block.contains("Seeping valve cover"), "{block}");
    assert!(block.contains("Shop observation"), "{block}");
    assert!(block.contains("2026-10-02"), "{block}");
    assert!(
        !block.contains("09:30"),
        "a page date is YYYY-MM-DD: {block}"
    );
}

#[tokio::test]
async fn home_says_so_when_nothing_is_waiting() {
    let db = db().await;
    VehicleFixture::new().name("Golf GTI").insert_id(&db).await;

    let (_, body) = page(app(db, "live"), get("/")).await;

    let block = waiting_block(&body);
    for line in [
        "No maintenance is due.",
        "No jobs waiting for an appointment.",
        "No open recalls.",
        "No open incidents.",
    ] {
        assert!(block.contains(line), "{line}: {block}");
    }
}

#[tokio::test]
async fn home_leaves_out_an_archived_car() {
    let db = db().await;
    VehicleFixture::new().name("Golf GTI").insert_id(&db).await;
    let sold = VehicleFixture::new().name("Old Jetta").insert_id(&db).await;
    glovebox_shared::services::vehicle::archive(
        &db,
        sold,
        glovebox_shared::inputs::vehicle::VehicleSale::default(),
    )
    .await
    .unwrap();

    let (_, body) = page(app(db, "live"), get("/")).await;

    assert!(body.contains("Golf GTI"), "{body}");
    assert!(!body.contains("Old Jetta"), "{body}");
}

#[tokio::test]
async fn home_says_how_many_due_items_are_already_on_an_appointment() {
    let db = db().await;
    let v = gti_at_64k(&db).await;
    let due = waiting::waiting(&db, v, common::today())
        .await
        .unwrap()
        .maintenance;
    let appt = appointment::create(
        &db,
        v,
        NewAppointment {
            purpose: "Everything due".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    for item in &due {
        work_item::create(
            &db,
            v,
            NewWorkItem {
                title: item.name.clone(),
                schedule_item_id: Some(item.schedule_item_id),
                appointment_id: Some(appt),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    let (_, body) = page(app(db, "live"), get("/")).await;

    let block = waiting_block(&body);
    assert!(
        block.contains(&format!("plus {} on an appointment", due.len())),
        "{block}"
    );
    assert!(
        block.contains("None waiting for an appointment."),
        "{block}"
    );
    assert!(!block.contains("No maintenance is due."), "{block}");
}

// The page edits no research facts, so every page says where they are changed.
#[tokio::test]
async fn every_page_says_research_facts_are_changed_through_claude() {
    const HINT: &str = "To add or change options, prices, part numbers or fitment, ask Claude.";
    let (_, home) = page(app(db().await, "live"), get("/")).await;
    let (_, missing) = page(app(db().await, "live"), get("/vehicles/99/jobs/99")).await;

    assert!(home.contains(HINT), "{home}");
    assert!(missing.contains(HINT), "{missing}");
}

#[tokio::test]
async fn every_page_names_its_environment_and_an_overriding_file() {
    let (_, test_body) = page(app(db().await, "test"), get("/")).await;
    let (_, live_body) = page(app(db().await, "live"), get("/")).await;
    let (_, file_body) = page(app_with_file(db().await, "test", "glovebox.db"), get("/")).await;

    assert!(
        test_body.contains(r#"<span class="pill pill--env""#),
        "{test_body}"
    );
    assert!(test_body.contains(">test</span>"), "{test_body}");
    assert!(live_body.contains(">live</span>"), "{live_body}");
    assert!(
        file_body.contains(">test \u{b7} glovebox.db</span>"),
        "{file_body}"
    );
}

#[tokio::test]
async fn foreign_host_is_403() {
    let on_host = |host: &str| {
        axum::http::Request::get("/")
            .header("host", host)
            .body(Body::empty())
            .unwrap()
    };

    for host in [
        "evil.example",
        "evil.example:4828",
        "127.0.0.1.evil.example:4828",
    ] {
        let (status, _) = page(app(db().await, "live"), on_host(host)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{host}");
    }
    for host in ["127.0.0.1:4828", "localhost:4828"] {
        let (status, _) = page(app(db().await, "live"), on_host(host)).await;
        assert_eq!(status, StatusCode::OK, "{host}");
    }
}

#[tokio::test]
async fn cross_origin_post_is_403() {
    let post = |origin: Option<&str>| {
        let mut req = request("POST", "/");
        if let Some(origin) = origin {
            req = req.header("origin", origin);
        }
        req.body(Body::empty()).unwrap()
    };

    let (status, _) = page(app(db().await, "live"), post(Some("http://evil.example"))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = page(app(db().await, "live"), post(Some("null"))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // An origin on the same host passes the guard and reaches the router,
    // which has no POST route for `/`.
    let (status, _) = page(app(db().await, "live"), post(Some("http://127.0.0.1:4828"))).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);

    let (status, _) = page(app(db().await, "live"), post(None)).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn every_response_carries_the_csp() {
    let foreign = axum::http::Request::get("/")
        .header("host", "evil.example")
        .body(Body::empty())
        .unwrap();
    let wrong_method = request("POST", "/").body(Body::empty()).unwrap();
    for req in [get("/"), get("/nowhere"), foreign, wrong_method] {
        let (res, _) = send(app(db().await, "live"), req).await;
        let status = res.status();
        assert_eq!(
            res.headers()
                .get("content-security-policy")
                .map(|v| v.to_str().unwrap()),
            Some(CSP),
            "status {status}"
        );
    }
}

#[tokio::test]
async fn unknown_path_is_404_page() {
    let (status, body) = page(app(db().await, "live"), get("/nowhere")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("<html"), "{body}");
    assert!(body.contains("Page not found"), "{body}");
    assert!(body.contains(r#"href="/""#), "{body}");
}

#[tokio::test]
async fn cross_site_fetch_metadata_post_is_403() {
    for site in ["cross-site", "same-site"] {
        let req = request("POST", "/")
            .header("sec-fetch-site", site)
            .body(Body::empty())
            .unwrap();
        let (status, _) = page(app(db().await, "live"), req).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{site}");
    }
    for site in ["same-origin", "none"] {
        let req = request("POST", "/")
            .header("sec-fetch-site", site)
            .body(Body::empty())
            .unwrap();
        let (status, _) = page(app(db().await, "live"), req).await;
        assert_ne!(status, StatusCode::FORBIDDEN, "{site}");
    }
}

#[tokio::test]
async fn font_files_are_served_as_woff2() {
    for path in ["/assets/plex-sans-400.woff2", "/assets/plex-sans-600.woff2"] {
        let (res, body) = send_bytes(app(db().await, "live"), get(path)).await;
        assert_eq!(res.status(), StatusCode::OK, "{path}");
        assert_eq!(res.headers()["content-type"], "font/woff2", "{path}");
        assert_eq!(
            res.headers()["cache-control"],
            "public, max-age=604800",
            "{path}"
        );
        assert!(body.starts_with(b"wOF2"), "{path}");

        // A request that names the font's tag gets 304 and no body.
        let etag = res.headers()["etag"].to_str().unwrap().to_owned();
        assert!(etag.starts_with('"') && etag.len() == 18, "{etag}");
        let again = request("GET", path)
            .header("if-none-match", &etag)
            .body(Body::empty())
            .unwrap();
        let (res, body) = send_bytes(app(db().await, "live"), again).await;
        assert_eq!(res.status(), StatusCode::NOT_MODIFIED, "{path}");
        assert!(body.is_empty(), "{path}");
        // A tag a cache marked weak still matches.
        let weak = request("GET", path)
            .header("if-none-match", format!("W/{etag}"))
            .body(Body::empty())
            .unwrap();
        let (res, _) = send_bytes(app(db().await, "live"), weak).await;
        assert_eq!(res.status(), StatusCode::NOT_MODIFIED, "{path}");
    }
}

#[tokio::test]
async fn every_page_loads_the_bundled_typeface_and_labels_the_database() {
    let (_, body) = page(app(db().await, "live"), get("/")).await;
    assert!(
        body.contains(r#"url("/assets/plex-sans-400.woff2")"#),
        "{body}"
    );
    assert!(
        body.contains(r#"url("/assets/plex-sans-600.woff2")"#),
        "{body}"
    );
    assert!(
        body.contains(r#"<span class="l">Database </span><span class="pill pill--env""#),
        "{body}"
    );
    assert!(body.contains(r#"<span class="l">Today </span>"#), "{body}");
}

// A vehicle with one appointment holding: "Front brakes" with an owner part
// bought (rotors × 2, ATE 13.0460) and a shop need (brake fluid); "Cabin
// filter" whose only need is installed; and a done job "Old job". Returns
// (vehicle, appointment, brakes job).
async fn sheet_fixture(db: &DatabaseConnection) -> (i32, i32, i32) {
    use glovebox_shared::{
        entities::{part::Supplier, work_item::JobStatus},
        inputs::{need::UpdateNeed, part::NewPart},
        services::part,
        test_support::{add_job, set_job_status},
    };
    let v = VehicleFixture::new().name("Golf GTI").insert_id(db).await;
    let a = appointment::create(
        db,
        v,
        NewAppointment {
            purpose: "Brakes".into(),
            shop: Some(glovebox_shared::inputs::shop::ShopChoice::Id(
                glovebox_shared::test_support::add_shop(db, "Smith Auto").await,
            )),
            planned_date: Some(date("2026-11-20")),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let brakes = add_job(db, v, "Front brakes", Some(a)).await;
    let cabin = add_job(db, v, "Cabin filter", Some(a)).await;
    let old = add_job(db, v, "Old job", Some(a)).await;
    let add = |job_id: i32, name: &'static str, quantity: i32| {
        need::add(
            db,
            v,
            job_id,
            NewNeed {
                name: name.into(),
                quantity: Some(quantity),
                ..Default::default()
            },
        )
    };
    let rotors = add(brakes, "Rotors", 2).await.unwrap().id;
    let fluid = add(brakes, "Brake fluid", 1).await.unwrap().id;
    let filter = add(cabin, "Cabin filter", 1).await.unwrap().id;
    let bought = part::create(
        db,
        v,
        NewPart {
            name: "Rotors".into(),
            manufacturer: Some("ATE".into()),
            part_number: Some("13.0460".into()),
            supplied_by: Some(Supplier::Owner),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    need::link_part(db, v, rotors, bought.id, Some(date("2026-11-12")))
        .await
        .unwrap();
    need::update(
        db,
        v,
        fluid,
        UpdateNeed {
            supplied_by: Some(Some(Supplier::Shop)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let fitted = part::create(
        db,
        v,
        NewPart {
            name: "Cabin filter".into(),
            fitting: glovebox_shared::inputs::part::NewFitting::Installed(
                glovebox_shared::inputs::part::Install {
                    date: Some(date("2026-11-01")),
                    ..Default::default()
                },
            ),
            supplied_by: Some(Supplier::Owner),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    need::link_part(db, v, filter, fitted.id, None)
        .await
        .unwrap();
    set_job_status(db, v, old, JobStatus::Done).await;
    (v, a, brakes)
}

fn sheet_path(v: i32, a: i32) -> String {
    format!("/vehicles/{v}/appointments/{a}/print")
}

#[tokio::test]
async fn print_sheet_lists_jobs_and_both_part_lists() {
    let db = db().await;
    let (v, a, _) = sheet_fixture(&db).await;

    let (status, body) = page(app(db, "test"), get(&sheet_path(v, a))).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains("Golf GTI — Smith Auto — 2026-11-20"),
        "{body}"
    );
    assert!(body.contains("Front brakes"), "{body}");
    assert!(body.contains(r#"class="ws-box""#), "{body}");
    assert!(body.contains("I'm supplying"), "{body}");
    assert!(body.contains("Rotors × 2"), "{body}");
    assert!(
        body.contains(r#"<span class="pn">ATE 13.0460</span>"#),
        "{body}"
    );
    assert!(body.contains("Please supply"), "{body}");
    assert!(body.contains("Brake fluid"), "{body}");
    assert!(!body.contains("Old job"), "{body}");
}

#[tokio::test]
async fn print_sheet_prints_job_and_part_notes_only_when_present() {
    use glovebox_shared::{
        inputs::{
            part::{PartFilter, UpdatePart},
            work_item::UpdateWorkItem,
        },
        services::{part, work_item},
    };
    let db = db().await;
    let (v, a, brakes) = sheet_fixture(&db).await;

    let (_, plain) = page(app(db.clone(), "test"), get(&sheet_path(v, a))).await;
    assert!(!plain.contains(r#"class="ws-note""#), "{plain}");

    work_item::update(
        &db,
        v,
        brakes,
        UpdateWorkItem {
            notes: Some(Some("Don't re-use the <old> bolts.".into())),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let rotors = part::list(&db, v, PartFilter::default())
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.part.name == "Rotors")
        .unwrap()
        .part
        .id;
    part::update(
        &db,
        v,
        rotors,
        UpdatePart {
            notes: Some(Some("Torque to 20 Nm.".into())),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let (status, body) = page(app(db, "test"), get(&sheet_path(v, a))).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains(r#"<p class="ws-note">Don&#39;t re-use the &#60;old&#62; bolts.</p>"#),
        "{body}"
    );
    assert!(
        body.contains(r#"<span class="ws-note">Torque to 20 Nm.</span>"#),
        "{body}"
    );
}

#[tokio::test]
async fn print_sheet_prints_a_linked_shop_parts_notes_under_please_supply() {
    use glovebox_shared::{
        entities::part::Supplier,
        inputs::{need::NewNeed, part::NewPart},
        services::{need, part},
    };
    let db = db().await;
    let (v, a, brakes) = sheet_fixture(&db).await;
    let seal = need::add(
        &db,
        v,
        brakes,
        NewNeed {
            name: "Caliper seal".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let bought = part::create(
        &db,
        v,
        NewPart {
            name: "Caliper seal".into(),
            notes: Some("Use the red grease.".into()),
            supplied_by: Some(Supplier::Shop),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    need::link_part(&db, v, seal, bought.id, None)
        .await
        .unwrap();

    let (status, body) = page(app(db, "test"), get(&sheet_path(v, a))).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let please = body.split("Please supply").nth(1).expect("a shop list");
    assert!(
        please.contains(r#"Caliper seal<span class="ws-note">Use the red grease.</span></li>"#),
        "{body}"
    );
}

#[tokio::test]
async fn print_sheet_boxes_each_owner_part_and_lists_a_kits_contents() {
    use glovebox_shared::{
        inputs::part::{NewPartContent, PartFilter, UpdatePart},
        services::part,
    };
    let db = db().await;
    let (v, a, _) = sheet_fixture(&db).await;
    let rotors = part::list(&db, v, PartFilter::default())
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.part.name == "Rotors")
        .unwrap()
        .part
        .id;
    part::update(
        &db,
        v,
        rotors,
        UpdatePart {
            add_contents: vec![
                NewPartContent {
                    name: "Rotor bolts".into(),
                    manufacturer: Some("Febi".into()),
                    part_number: Some("N 911 568 01".into()),
                    quantity: Some(4),
                },
                NewPartContent {
                    name: "Anti-squeal paste".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let (status, body) = page(app(db, "test"), get(&sheet_path(v, a))).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("Rotor bolts × 4"), "{body}");
    assert!(
        body.contains(r#"<span class="pn">Febi N 911 568 01</span>"#),
        "{body}"
    );
    assert!(body.contains("Anti-squeal paste"), "{body}");
    // The sheet has a box for each of the two jobs, one for the rotors and one
    // for each of the two items in their kit, which makes five.
    assert_eq!(body.matches(r#"class="ws-box""#).count(), 5, "{body}");
}

#[tokio::test]
async fn print_sheet_omits_empty_groups() {
    let db = db().await;
    let (v, a, brakes) = sheet_fixture(&db).await;
    work_item::delete(&db, v, brakes).await.unwrap();

    let (_, body) = page(app(db, "test"), get(&sheet_path(v, a))).await;

    assert!(body.contains("Cabin filter"), "{body}");
    assert!(!body.contains("Please supply"), "{body}");
    assert!(!body.contains("I'm supplying"), "{body}");
}

#[tokio::test]
async fn print_sheet_warns_about_unsettled_needs() {
    let db = db().await;
    let (v, a, brakes) = sheet_fixture(&db).await;
    let pads = need::add(
        &db,
        v,
        brakes,
        NewNeed {
            name: "Brake pads".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;

    let (_, body) = page(app(db, "test"), get(&sheet_path(v, a))).await;

    assert!(body.contains("Before you print"), "{body}");
    assert!(body.contains("Brake pads (Front brakes)"), "{body}");
    assert!(
        body.contains(&format!(r#"action="/vehicles/{v}/needs/{pads}/supplier""#)),
        "{body}"
    );
    assert!(
        body.contains(r#"name="supplied_by" value="shop""#),
        "{body}"
    );
    assert!(
        body.contains(&format!(r#"name="back" value="{}""#, sheet_path(v, a))),
        "{body}"
    );
    assert!(
        body.contains(&format!(
            r#"href="/vehicles/{v}/jobs/{brakes}?open=need-{pads}#need-{pads}""#
        )),
        "{body}"
    );
    assert!(!body.contains("Everything is settled."), "{body}");
}

#[tokio::test]
async fn print_sheet_says_everything_is_settled() {
    let db = db().await;
    let (v, a, _) = sheet_fixture(&db).await;

    let (_, body) = page(app(db, "test"), get(&sheet_path(v, a))).await;

    assert!(body.contains("Everything is settled."), "{body}");
    assert!(!body.contains("Before you print"), "{body}");
}

#[tokio::test]
async fn print_sheet_with_no_planned_jobs_says_so() {
    let db = db().await;
    let v = VehicleFixture::new().name("Golf GTI").insert_id(&db).await;
    let a = appointment::create(
        &db,
        v,
        NewAppointment {
            purpose: "Brakes".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;

    let (_, body) = page(app(db, "test"), get(&sheet_path(v, a))).await;

    assert!(
        body.contains("No planned jobs on this appointment."),
        "{body}"
    );
    assert!(!body.contains("Everything is settled."), "{body}");
}

// An appointment with no jobs for a vehicle recorded with `details`.
async fn sheet_for_car(
    db: &DatabaseConnection,
    details: glovebox_shared::inputs::vehicle::NewVehicle,
) -> (i32, i32) {
    let v = glovebox_shared::services::vehicle::create(db, details)
        .await
        .unwrap()
        .id;
    let a = appointment::create(
        db,
        v,
        NewAppointment {
            purpose: "Brakes".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    (v, a)
}

#[tokio::test]
async fn print_sheet_heading_names_the_car() {
    use glovebox_shared::inputs::vehicle::NewVehicle;
    let db = db().await;
    let (v, a) = sheet_for_car(
        &db,
        NewVehicle {
            name: "Greta".into(),
            year: Some(2017),
            make: Some("Volkswagen".into()),
            model: Some("Golf GTI".into()),
            trim_level: Some("Autobahn".into()),
            vin: Some("3VW447AU9HM012345".into()),
            license_plate: Some("ABC 1234".into()),
            purchase_date: Some(date("2017-03-05")),
            purchase_mileage: Some(40),
            ..Default::default()
        },
    )
    .await;
    let read_on = common::today() - chrono::Days::new(12);
    for (day, miles) in [
        (common::today() - chrono::Days::new(73), 60_000),
        (read_on, 61_000),
    ] {
        mileage::create(
            &db,
            v,
            NewMileageEntry {
                mileage: miles,
                recorded_at: Some(day.into()),
                source: None,
                notes: None,
            },
        )
        .await
        .unwrap();
    }

    let (_, body) = page(app(db, "test"), get(&sheet_path(v, a))).await;

    assert!(body.contains("<h1>Greta</h1>"), "{body}");
    assert!(
        body.contains(r#"<p class="ws-car">2017 Volkswagen Golf GTI Autobahn</p>"#),
        "{body}"
    );
    assert!(
        body.contains(r#"<span class="l">VIN</span> 3VW447AU9HM012345</span>"#),
        "{body}"
    );
    assert!(
        body.contains(r#"<span class="l">Plate</span> ABC 1234</span>"#),
        "{body}"
    );
    // The page's day is twelve days after the reading, and the sheet prints
    // the reading, not a projection from it.
    let odometer = format!(r#"<span class="l">Odometer</span> 61,000 mi on {read_on}</span>"#);
    assert!(body.contains(&odometer), "{body}");
}

#[tokio::test]
async fn print_sheet_heading_omits_what_the_car_lacks() {
    use glovebox_shared::inputs::vehicle::NewVehicle;
    let db = db().await;
    let (bare, bare_sheet) = sheet_for_car(
        &db,
        NewVehicle {
            name: "Greta".into(),
            purchase_mileage: Some(40),
            ..Default::default()
        },
    )
    .await;
    let (plated, plated_sheet) = sheet_for_car(
        &db,
        NewVehicle {
            name: "Otto".into(),
            make: Some("Audi".into()),
            license_plate: Some("ABC 1234".into()),
            ..Default::default()
        },
    )
    .await;

    let (_, body) = page(app(db.clone(), "test"), get(&sheet_path(bare, bare_sheet))).await;
    assert!(body.contains("<h1>Greta</h1>"), "{body}");
    for absent in [r#"class="ws-car""#, r#"class="ws-facts""#] {
        assert!(!body.contains(absent), "{absent}: {body}");
    }

    let (_, body) = page(app(db, "test"), get(&sheet_path(plated, plated_sheet))).await;
    assert!(body.contains(r#"<p class="ws-car">Audi</p>"#), "{body}");
    assert!(
        body.contains(r#"<span class="l">Plate</span> ABC 1234</span>"#),
        "{body}"
    );
    for absent in [">VIN<", ">Odometer<"] {
        assert!(!body.contains(absent), "{absent}: {body}");
    }
}

#[tokio::test]
async fn print_sheet_prints_the_symptom_of_a_job_filed_from_an_incident() {
    let db = db().await;
    let (v, a, _) = sheet_fixture(&db).await;
    let rattle = incident::create(
        &db,
        v,
        NewIncident {
            title: "Rattle from the driver's side".into(),
            description: Some("Over bumps below 20 mph.".into()),
            occurred_at: Some((common::today() - chrono::Days::new(11)).into()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .incident
    .id;
    work_item::create(
        &db,
        v,
        NewWorkItem {
            title: "Diagnose the rattle".into(),
            incident_id: Some(rattle),
            appointment_id: Some(a),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let (_, body) = page(app(db, "test"), get(&sheet_path(v, a))).await;

    assert!(
        body.contains(
            "<p class=\"ws-note\"><b>Reported symptom:</b> Rattle from the driver&#39;s side<span \
             class=\"ws-note\">Over bumps below 20 mph.</span></p>"
        ),
        "{body}"
    );
    assert_eq!(body.matches("Reported symptom").count(), 1, "{body}");
}

#[tokio::test]
async fn print_sheet_prints_no_symptom_for_a_job_filed_from_no_incident() {
    let db = db().await;
    let (v, a, _) = sheet_fixture(&db).await;

    let (_, body) = page(app(db, "test"), get(&sheet_path(v, a))).await;

    assert!(body.contains("Front brakes"), "{body}");
    assert!(!body.contains("Reported symptom"), "{body}");
}

#[tokio::test]
async fn print_sheet_of_another_vehicles_appointment_is_not_found() {
    let db = db().await;
    let (_, a, _) = sheet_fixture(&db).await;
    let other = VehicleFixture::new().name("Other").insert_id(&db).await;

    let (status, wrong) = page(app(db.clone(), "test"), get(&sheet_path(other, a))).await;
    let (_, missing) = page(app(db, "test"), get(&sheet_path(other, 9999))).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(wrong, missing.replace("9999", &a.to_string()));
}

#[tokio::test]
async fn appointment_page_links_to_its_print_sheet() {
    let db = db().await;
    let (v, a, _) = sheet_fixture(&db).await;

    let (_, body) = page(
        app(db, "test"),
        get(&format!("/vehicles/{v}/appointments/{a}")),
    )
    .await;

    assert!(
        body.contains(&format!(r#"href="{}""#, sheet_path(v, a))),
        "{body}"
    );
    assert!(body.contains("Print sheet"), "{body}");
}

// Two shops: "Smith Auto" with two Golf services (one partly paid by
// insurance, one with no total), an Alltrack service, an appointment in
// planning and a cancelled one; and "Acme Tires" with nothing. Returns
// (Smith Auto, Golf, the planning appointment).
async fn shop_fixture(db: &DatabaseConnection) -> (i32, i32, i32) {
    use glovebox_shared::{
        entities::{
            service_record::{Payer, Performer},
            service_record_line_item::WorkKind,
        },
        inputs::{
            service_record::{NewLineItem, NewServiceRecord},
            shop::{NewShop, ShopChoice},
        },
        services::{service_record, shop},
    };
    let smith = shop::create(
        db,
        NewShop {
            name: "Smith Auto".into(),
            address: Some("1 Main St".into()),
            phone: Some("555-0100".into()),
            website: Some("https://smith.example".into()),
            specialty: Some("VW".into()),
            notes: Some("Ask for Pat".into()),
        },
    )
    .await
    .unwrap()
    .id;
    glovebox_shared::test_support::add_shop(db, "Acme Tires").await;
    let golf = VehicleFixture::new().name("Golf").insert_id(db).await;
    let alltrack = VehicleFixture::new().name("Alltrack").insert_id(db).await;
    let service = |on: &str, description: &str, total: Option<i32>, lines| NewServiceRecord {
        service_date: date(on),
        performed_by: Some(Performer::Shop),
        shop: Some(ShopChoice::Id(smith)),
        description: Some(description.into()),
        total_cost_cents: total,
        line_items: lines,
        ..Default::default()
    };
    let line = |description: &str, paid_by, cents| NewLineItem {
        description: description.into(),
        work_kind: WorkKind::Repair,
        paid_by,
        quantity: None,
        unit_cost_cents: None,
        cost_cents: Some(cents),
    };
    service_record::create(
        db,
        golf,
        service(
            "2026-10-01",
            "Windshield and wipers",
            Some(50_000),
            Some(vec![
                line("Windshield", Some(Payer::Insurance), 20_000),
                line("Wipers", None, 30_000),
            ]),
        ),
    )
    .await
    .unwrap();
    service_record::create(db, golf, service("2026-09-01", "Inspection", None, None))
        .await
        .unwrap();
    service_record::create(
        db,
        alltrack,
        service("2026-11-01", "Oil change", Some(12_000), None),
    )
    .await
    .unwrap();
    let planning = appointment::create(
        db,
        golf,
        NewAppointment {
            purpose: "Rear brakes".into(),
            shop: Some(ShopChoice::Id(smith)),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let dropped = appointment::create(
        db,
        alltrack,
        NewAppointment {
            purpose: "Alignment".into(),
            shop: Some(ShopChoice::Id(smith)),
            planned_date: Some(date("2026-11-20")),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    appointment::cancel(db, alltrack, dropped).await.unwrap();
    (smith, golf, planning)
}

// The rows of the section headed `heading`, each with its tags stripped and
// its whitespace collapsed.
// `html` without its visually hidden label spans.
fn without_hidden_labels(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some((before, after)) = rest.split_once(r#"<span class="vh">"#) {
        out.push_str(before);
        rest = after.split_once("</span>").map_or("", |(_, tail)| tail);
    }
    out.push_str(rest);
    out
}

fn section_rows(body: &str, heading: &str) -> Vec<String> {
    let section = body
        .split(&format!(">{heading}</h2>"))
        .nth(1)
        .unwrap_or_else(|| panic!("no {heading} section: {body}"));
    let section = section.split("</section>").next().unwrap_or_default();
    section
        .split("<li class=\"row")
        .skip(1)
        .map(|row| {
            let row = row.split("</li>").next().unwrap_or_default();
            let row = row.split_once('>').map_or("", |(_, rest)| rest);
            // The hidden labels have their own test, so a row reads here
            // as it does on a wide window.
            let row = without_hidden_labels(row);
            let mut text = String::new();
            let mut in_tag = false;
            for c in row.chars() {
                match c {
                    '<' => in_tag = true,
                    '>' => {
                        in_tag = false;
                        text.push(' ');
                    }
                    _ if !in_tag => text.push(c),
                    _ => {}
                }
            }
            text.split_whitespace().collect::<Vec<_>>().join(" ")
        })
        .collect()
}

#[tokio::test]
async fn shops_page_lists_each_shop_with_its_counts() {
    let db = db().await;
    let (smith, _, _) = shop_fixture(&db).await;
    let (status, body) = page(app(db, "test"), get("/shops")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(&format!(r#"<a href="/shops/{smith}">Smith Auto</a>"#)),
        "{body}"
    );
    assert_eq!(
        section_rows(&body, "Directory"),
        [
            "Acme Tires 0 0 No services",
            "Smith Auto 1 Main St 3 2 2026-11-01",
        ]
    );
}

#[tokio::test]
async fn an_empty_directory_says_so() {
    let (status, body) = page(app(db().await, "test"), get("/shops")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("No shops yet. Ask Claude to add one."),
        "{body}"
    );
}

#[tokio::test]
async fn a_shop_page_shows_its_work_across_vehicles_and_what_it_cost() {
    let db = db().await;
    let (smith, golf, planning) = shop_fixture(&db).await;
    let (status, body) = page(app(db, "test"), get(&format!("/shops/{smith}"))).await;
    assert_eq!(status, StatusCode::OK);
    for detail in [
        r#"<span class="l">Address</span>1 Main St"#,
        r#"<span class="l">Phone</span>555-0100"#,
        r#"<span class="l">Website</span><a href="https://smith.example" target="_blank" rel="noopener noreferrer">https://smith.example</a>"#,
        r#"<span class="l">Specialty</span>VW"#,
        r#"<span class="l">Invoiced</span><span class="price">$620.00</span>"#,
        r#"<span class="l">You paid</span><span class="price">$420.00</span>"#,
        "Ask for Pat",
    ] {
        assert!(body.contains(detail), "missing {detail}: {body}");
    }
    assert_eq!(
        section_rows(&body, "Services"),
        [
            "2026-11-01 Alltrack Oil change $120.00 $120.00",
            "2026-10-01 Golf Windshield and wipers $500.00 $300.00",
            "2026-09-01 Golf Inspection No total",
        ]
    );
    assert_eq!(
        section_rows(&body, "Appointments"),
        [
            "Alignment Alltrack 2026-11-20 Cancelled",
            "Rear brakes Golf not booked Planning",
        ]
    );
    assert!(
        body.contains(&format!(
            r#"<a href="/vehicles/{golf}/appointments/{planning}">Rear brakes</a>"#
        )),
        "{body}"
    );
    assert!(body.contains("pill pill--plain\">Cancelled"), "{body}");
}

#[tokio::test]
async fn a_missing_shop_is_the_not_found_page() {
    let (status, _) = page(app(db().await, "test"), get("/shops/999")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_shop_website_that_is_not_a_web_address_prints_as_text() {
    use glovebox_shared::{inputs::shop::NewShop, services::shop};
    let db = db().await;
    let id = shop::create(
        &db,
        NewShop {
            name: "Odd Shop".into(),
            website: Some("javascript:alert(1)".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let (_, body) = page(app(db, "test"), get(&format!("/shops/{id}"))).await;
    assert!(
        body.contains(r#"<span class="l">Website</span>javascript:alert(1)</div>"#),
        "{body}"
    );
    assert!(!body.contains(r#"href="javascript"#), "{body}");
}

#[tokio::test]
async fn a_shop_name_links_to_its_page_wherever_it_is_printed() {
    let db = db().await;
    let (smith, golf, planning) = shop_fixture(&db).await;
    let link = format!(r#"<a href="/shops/{smith}">Smith Auto</a>"#);
    for path in [
        "/".to_owned(),
        format!("/vehicles/{golf}/appointments/{planning}"),
        format!("/vehicles/{golf}/appointments/{planning}/print"),
    ] {
        let (status, body) = page(app(db.clone(), "test"), get(&path)).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(body.contains(&link), "{path}: {body}");
        assert!(body.contains(r#"<a href="/shops">Shops</a>"#), "{path}");
    }
}

#[tokio::test]
async fn an_appointment_with_no_shop_links_to_none() {
    let db = db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let a = appointment::create(
        &db,
        v,
        NewAppointment {
            purpose: "Detailing".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    for path in [
        format!("/vehicles/{v}/appointments/{a}"),
        format!("/vehicles/{v}/appointments/{a}/print"),
    ] {
        let (_, body) = page(app(db.clone(), "test"), get(&path)).await;
        assert!(!body.contains(r#"href="/shops/"#), "{path}: {body}");
    }
}

// A Golf service at "Smith Auto" that runs three days and carries every kind
// of link: two lines (one paid by insurance) that add up to $50.00 less than
// the total, a completed job, a quote, a kit, a schedule item, a recall, an
// incident it resolved and one noticed during it. Returns (Smith Auto, Golf,
// the service, the completed job, the quoted job).
async fn service_fixture(db: &DatabaseConnection) -> (i32, i32, i32, i32, i32) {
    use glovebox_shared::{
        entities::{service_record::Payer, service_record_line_item::WorkKind},
        inputs::{
            schedule::NewScheduleItem,
            service_record::{NewLineItem, NewServiceRecord},
            shop::ShopChoice,
        },
        services::{schedule, service_record},
        test_support::{add_job, add_shop, seed_recall},
    };
    let smith = add_shop(db, "Smith Auto").await;
    let golf = VehicleFixture::new().name("Golf").insert_id(db).await;
    let fluid = schedule::create(
        db,
        NewScheduleItem {
            vehicle_id: Some(golf),
            name: "Brake fluid".into(),
            interval_months: Some(24),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let brakes = add_job(db, golf, "Rear brakes", None).await;
    let recall = seed_recall(db, golf, "24V123", "Fuel pump").await;
    let squeal = incident_on(db, golf, IncidentCategory::Noise, "Squeal <rear>").await;
    let light = incident_on(db, golf, IncidentCategory::WarningLight, "EPC light").await;
    let line = |description: &str, work_kind, paid_by, cents| NewLineItem {
        description: description.into(),
        work_kind,
        paid_by,
        quantity: None,
        unit_cost_cents: None,
        cost_cents: Some(cents),
    };
    let service = service_record::create(
        db,
        golf,
        NewServiceRecord {
            service_date: date("2026-10-01"),
            end_date: Some(date("2026-10-03")),
            mileage: Some(61_200),
            description: Some("Brakes & glass".into()),
            total_cost_cents: Some(50_000),
            shop: Some(ShopChoice::Id(smith)),
            notes: Some("Left the old pads in the trunk".into()),
            invoice_ref: Some("INV-77".into()),
            schedule_item_ids: Some(vec![fluid]),
            work_item_ids: Some(vec![brakes]),
            recall_finding_ids: Some(vec![recall]),
            resolved_incident_ids: Some(vec![squeal]),
            noticed_incident_ids: Some(vec![light]),
            line_items: Some(vec![
                line(
                    "Windshield",
                    WorkKind::Repair,
                    Some(Payer::Insurance),
                    20_000,
                ),
                line("Rear pads", WorkKind::Maintenance, None, 25_000),
            ]),
            parts: Some(vec![brake_kit()]),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .record
    .id;
    let quote = work_item::create(
        db,
        golf,
        NewWorkItem {
            title: "Front rotors".into(),
            quote_service_id: Some(service),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    (smith, golf, service, brakes, quote)
}

// A kit of four numbered pads and a set of shims with no number.
fn brake_kit() -> glovebox_shared::inputs::part::NewPart {
    use glovebox_shared::inputs::part::{NewPart, NewPartContent};
    let item = |name: &str, part_number: Option<&str>, quantity| NewPartContent {
        name: name.into(),
        manufacturer: part_number.map(|_| "Akebono".into()),
        part_number: part_number.map(Into::into),
        quantity,
    };
    NewPart {
        name: "Rear brake kit".into(),
        manufacturer: Some("ATE".into()),
        part_number: Some("13.0460".into()),
        contents: vec![
            item("Pads", Some("ACT1633A"), Some(4)),
            item("Shims", None, None),
        ],
        ..Default::default()
    }
}

// An incident dated before the service fixture's service.
async fn incident_on(
    db: &DatabaseConnection,
    vehicle: i32,
    category: IncidentCategory,
    title: &str,
) -> i32 {
    let input = NewIncident {
        category,
        title: title.into(),
        occurred_at: Some(date("2026-09-20")),
        ..Default::default()
    };
    let created = incident::create(db, vehicle, input).await.unwrap();
    created.incident.id
}

fn service_path(v: i32, id: i32) -> String {
    format!("/vehicles/{v}/services/{id}")
}

#[tokio::test]
async fn a_service_page_lists_its_lines_totals_and_linked_work() {
    let db = db().await;
    let (smith, golf, service, brakes, quote) = service_fixture(&db).await;
    let (status, body) = page(app(db, "test"), get(&service_path(golf, service))).await;
    assert_eq!(status, StatusCode::OK);
    let shop_link = format!(r#"<a href="/shops/{smith}">Smith Auto</a>"#);
    for detail in [
        r#"<h1 id="service-title">Brakes &#38; glass</h1>"#.to_owned(),
        r#"<a href="/">Garage</a>"#.to_owned(),
        format!(r#"<span class="l">Vehicle</span><a href="/vehicles/{golf}">Golf</a>"#),
        r#"<span class="l">Date</span>2026-10-01 to 2026-10-03"#.to_owned(),
        format!(r#"<span class="l">Shop</span>{shop_link}"#),
        r#"<span class="l">Odometer</span>61,200 mi"#.to_owned(),
        r#"<span class="l">Invoice</span><span class="pn">INV-77</span>"#.to_owned(),
        r#"<span class="l">Total</span><span class="price">$500.00</span>"#.to_owned(),
        r#"<span class="l">Lines</span><span class="price">$450.00</span>"#.to_owned(),
        r#"<span class="l">You paid</span><span class="price">$300.00</span>"#.to_owned(),
        r#"<span class="l">Paid by others</span><span class="price">$200.00</span>"#.to_owned(),
        "<p>The lines add up to $450.00, which is $50.00 less than the total.</p>".to_owned(),
        r#"<span class="l">Notes </span>Left the old pads in the trunk"#.to_owned(),
        format!(r#"<a href="/vehicles/{golf}/jobs/{brakes}">Rear brakes</a>"#),
        format!(r#"<a href="/vehicles/{golf}/jobs/{quote}">Front rotors</a>"#),
        r#"pill pill--ok">Resolved"#.to_owned(),
        r#"pill pill--plain">Open"#.to_owned(),
    ] {
        assert!(body.contains(&detail), "missing {detail}: {body}");
    }
    for (heading, rows) in [
        (
            "Lines",
            vec![
                "Windshield Repair Insurance $200.00",
                "Rear pads Maintenance $250.00",
            ],
        ),
        ("Jobs completed", vec!["Rear brakes"]),
        ("Quotes", vec!["Front rotors"]),
        (
            "Parts installed",
            vec!["Rear brake kit ATE 13.0460 In the kit Pads × 4 Akebono ACT1633A Shims"],
        ),
        ("Maintenance it counts toward", vec!["Brake fluid"]),
        ("Recalls performed", vec!["Fuel pump 24V123"]),
        (
            "Incidents resolved",
            vec!["Squeal &#60;rear&#62; Noise Resolved"],
        ),
        ("Incidents noticed", vec!["EPC light Warning light Open"]),
    ] {
        assert_eq!(section_rows(&body, heading), rows, "{heading}");
    }
    // The page is read-only and runs nothing.
    assert!(!body.contains("<form"), "{body}");
    assert!(!body.contains("<script"), "{body}");
}

// The shop fixture's "Inspection" names a shop and records nothing else.
#[tokio::test]
async fn a_service_page_omits_its_empty_sections() {
    let db = db().await;
    let (_, golf, _) = shop_fixture(&db).await;
    let services = glovebox_shared::services::service_record::list(&db, golf)
        .await
        .unwrap();
    let inspection = services
        .iter()
        .find(|s| s.record.description.as_deref() == Some("Inspection"))
        .unwrap()
        .record
        .id;
    let (status, body) = page(app(db, "test"), get(&service_path(golf, inspection))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(r#"<span class="l">Total</span><span class="empty">No total</span>"#),
        "{body}"
    );
    for absent in [
        "<h2",
        ">Odometer<",
        ">Invoice<",
        ">Lines<",
        ">You paid<",
        ">Paid by others<",
        ">Paid by<",
        ">Notes <",
        "The lines add up to",
    ] {
        assert!(!body.contains(absent), "{absent} is shown: {body}");
    }
}

#[tokio::test]
async fn a_missing_service_and_another_vehicles_service_are_the_same_not_found_page() {
    let db = db().await;
    let (_, golf, service, _, _) = service_fixture(&db).await;
    let other = VehicleFixture::new().name("Alltrack").insert_id(&db).await;
    let (missing_status, missing) =
        page(app(db.clone(), "test"), get(&service_path(golf, 999))).await;
    let (wrong_status, wrong) = page(app(db, "test"), get(&service_path(other, service))).await;
    assert_eq!(missing_status, StatusCode::NOT_FOUND);
    assert_eq!(wrong_status, StatusCode::NOT_FOUND);
    assert!(missing.contains("Page not found"), "{missing}");
    assert_eq!(wrong, missing);
}

#[tokio::test]
async fn a_shop_page_links_each_service_to_its_page() {
    let db = db().await;
    let (smith, golf, service, _, _) = service_fixture(&db).await;
    let (_, body) = page(app(db, "test"), get(&format!("/shops/{smith}"))).await;
    assert!(
        body.contains(&format!(
            r#"<span class="key"><a href="/vehicles/{golf}/services/{service}">2026-10-01</a></span>"#
        )),
        "{body}"
    );
}

#[tokio::test]
async fn a_self_performed_service_names_no_shop() {
    use glovebox_shared::{
        entities::service_record::Performer, inputs::service_record::NewServiceRecord,
        services::service_record,
    };
    let db = db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let service = service_record::create(
        &db,
        v,
        NewServiceRecord {
            service_date: date("2026-11-01"),
            performed_by: Some(Performer::Owner),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .record
    .id;
    let (status, body) = page(app(db, "test"), get(&service_path(v, service))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(r#"<span class="l">Shop</span>Self-performed"#),
        "{body}"
    );
    assert!(
        body.contains(r#"<h1 id="service-title">Service</h1>"#),
        "{body}"
    );
    assert!(!body.contains(r#"href="/shops/"#), "{body}");
}

// Records `input` on a new vehicle and returns the vehicle, the service and
// the service's page.
async fn service_page_of(
    db: DatabaseConnection,
    input: glovebox_shared::inputs::service_record::NewServiceRecord,
) -> (i32, i32, String) {
    let v = VehicleFixture::new().insert_id(&db).await;
    let created = glovebox_shared::services::service_record::create(&db, v, input)
        .await
        .unwrap();
    let service = created.record.id;
    let (status, body) = page(app(db, "test"), get(&service_path(v, service))).await;
    assert_eq!(status, StatusCode::OK);
    (v, service, body)
}

#[tokio::test]
async fn a_service_line_shows_its_quantity_and_unit_cost_and_the_head_its_parts_and_labor() {
    use glovebox_shared::{
        entities::service_record_line_item::WorkKind,
        inputs::service_record::{NewLineItem, NewServiceRecord},
    };
    let line = |description: &str, quantity, unit_cost_cents| NewLineItem {
        description: description.into(),
        work_kind: WorkKind::Fee,
        paid_by: None,
        quantity,
        unit_cost_cents,
        cost_cents: None,
    };
    let input = NewServiceRecord {
        service_date: date("2026-10-01"),
        parts_cost_cents: Some(4_500),
        labor_cost_cents: Some(12_000),
        line_items: Some(vec![
            line("Shop rags", Some(5.0), Some(900)),
            line("Oil", Some(5.5), None),
            line("Disposal", None, Some(300)),
        ]),
        ..Default::default()
    };
    let (_, _, body) = service_page_of(db().await, input).await;
    assert_eq!(
        section_rows(&body, "Lines"),
        [
            "Shop rags Fee 5 × $9.00",
            "Oil Fee × 5.5",
            "Disposal Fee $3.00 each",
        ]
    );
    for detail in [
        r#"<span class="l">Parts</span><span class="price">$45.00</span>"#,
        r#"<span class="l">Labor</span><span class="price">$120.00</span>"#,
    ] {
        assert!(body.contains(detail), "missing {detail}: {body}");
    }
}

#[tokio::test]
async fn a_service_with_a_blank_description_is_headed_service() {
    let input = glovebox_shared::inputs::service_record::NewServiceRecord {
        service_date: date("2026-10-01"),
        description: Some("  ".into()),
        ..Default::default()
    };
    let (_, _, body) = service_page_of(db().await, input).await;
    assert!(
        body.contains(r#"<h1 id="service-title">Service</h1>"#),
        "{body}"
    );
}

#[tokio::test]
async fn a_service_that_ends_the_day_it_starts_shows_one_date() {
    let input = glovebox_shared::inputs::service_record::NewServiceRecord {
        service_date: date("2026-10-01"),
        end_date: Some(date("2026-10-01")),
        ..Default::default()
    };
    let (_, _, body) = service_page_of(db().await, input).await;
    assert!(
        body.contains(r#"<span class="l">Date</span>2026-10-01</div>"#),
        "{body}"
    );
}

// A service can both notice and resolve one incident, and neither row links
// back to the page it is on.
#[tokio::test]
async fn an_incident_a_service_noticed_and_resolved_links_to_no_service() {
    let db = db().await;
    let v = VehicleFixture::new().insert_id(&db).await;
    let rattle = incident_on(&db, v, IncidentCategory::Noise, "Rattle").await;
    let created = glovebox_shared::services::service_record::create(
        &db,
        v,
        glovebox_shared::inputs::service_record::NewServiceRecord {
            service_date: date("2026-10-01"),
            resolved_incident_ids: Some(vec![rattle]),
            noticed_incident_ids: Some(vec![rattle]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let path = service_path(v, created.record.id);
    let (_, body) = page(app(db, "test"), get(&path)).await;
    assert_eq!(
        section_rows(&body, "Incidents resolved"),
        ["Rattle Noise Resolved"]
    );
    assert_eq!(
        section_rows(&body, "Incidents noticed"),
        ["Rattle Noise Resolved"]
    );
    assert!(!body.contains(&format!(r#"href="{path}""#)), "{body}");
}

#[tokio::test]
async fn a_completed_jobs_page_links_to_the_service_that_completed_it() {
    let db = db().await;
    let (_, golf, service, brakes, quote) = service_fixture(&db).await;
    let job = |id: i32| format!("/vehicles/{golf}/jobs/{id}");
    let (_, body) = page(app(db.clone(), "test"), get(&job(brakes))).await;
    assert!(
        body.contains(&format!(
            r#"<span class="l">Completed by</span><a href="/vehicles/{golf}/services/{service}">Service of 2026-10-01</a>"#
        )),
        "{body}"
    );
    let (_, body) = page(app(db, "test"), get(&job(quote))).await;
    assert!(!body.contains("Completed by"), "{body}");
}

// The day `days` before the page's clock.
fn days_before_today(days: u64) -> chrono::NaiveDate {
    common::today() - chrono::Days::new(days)
}

// The day `days` before the page's clock, as a page prints it.
fn day_label(days: u64) -> String {
    days_before_today(days).format("%Y-%m-%d").to_string()
}

async fn log_reading(db: &DatabaseConnection, vehicle: i32, miles: i32, days_ago: u64) {
    let entry = NewMileageEntry {
        mileage: miles,
        recorded_at: Some(days_before_today(days_ago).into()),
        source: None,
        notes: None,
    };
    mileage::create(db, vehicle, entry).await.unwrap();
}

// A 2017 Golf with every fact its page prints, bought 800 days ago. It has a
// reading 100 days ago and one 10 days ago, a $500.00 service at "Smith Auto"
// 50 days ago with a $200.00 line insurance paid, and an incident 20 days
// ago. Returns (Smith Auto, the Golf, the service).
async fn vehicle_fixture(db: &DatabaseConnection) -> (i32, i32, i32) {
    use glovebox_shared::{
        entities::{service_record::Payer, service_record_line_item::WorkKind},
        inputs::{
            service_record::{NewLineItem, NewServiceRecord},
            shop::ShopChoice,
            vehicle::NewVehicle,
        },
        services::{service_record, vehicle},
        test_support::add_shop,
    };
    let smith = add_shop(db, "Smith Auto").await;
    let golf = vehicle::create(
        db,
        NewVehicle {
            name: "Steve's Golf".into(),
            year: Some(2017),
            make: Some("Volkswagen".into()),
            model: Some("Golf GTI".into()),
            trim_level: Some("SE".into()),
            vin: Some("3VW447AU9HM012345".into()),
            license_plate: Some("ABC 123".into()),
            color: Some("White".into()),
            purchase_date: Some(days_before_today(800)),
            purchase_mileage: Some(40_000),
            warranty_expires_on: Some(days_before_today(900)),
            warranty_expires_miles: Some(72_000),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    log_reading(db, golf, 60_000, 100).await;
    log_reading(db, golf, 61_000, 10).await;
    let service = service_record::create(
        db,
        golf,
        NewServiceRecord {
            service_date: days_before_today(50),
            mileage: Some(60_500),
            description: Some("Brakes & glass".into()),
            total_cost_cents: Some(50_000),
            shop: Some(ShopChoice::Id(smith)),
            line_items: Some(vec![NewLineItem {
                description: "Windshield".into(),
                work_kind: WorkKind::Repair,
                paid_by: Some(Payer::Insurance),
                quantity: None,
                unit_cost_cents: None,
                cost_cents: Some(20_000),
            }]),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .record
    .id;
    let squeal = NewIncident {
        category: IncidentCategory::Noise,
        title: "Squeal".into(),
        occurred_at: Some(days_before_today(20).into()),
        ..Default::default()
    };
    incident::create(db, golf, squeal).await.unwrap();
    (smith, golf, service)
}

#[tokio::test]
async fn a_vehicle_page_shows_its_facts_costs_and_recent_activity() {
    let db = db().await;
    let (_, golf, service) = vehicle_fixture(&db).await;
    let (status, body) = page(app(db, "test"), get(&format!("/vehicles/{golf}"))).await;
    assert_eq!(status, StatusCode::OK);
    for detail in [
        r#"<h1 id="vehicle-title">Steve&#39;s Golf</h1>"#.to_owned(),
        "<p>2017 Volkswagen Golf GTI SE</p>".to_owned(),
        r#"<a href="/">Garage</a>"#.to_owned(),
        r#"<span class="l">VIN</span><span class="pn">3VW447AU9HM012345</span>"#.to_owned(),
        r#"<span class="l">Plate</span><span class="pn">ABC 123</span>"#.to_owned(),
        r#"<span class="l">Color</span>White"#.to_owned(),
        format!(r#"<span class="l">Purchased</span>{}"#, day_label(800)),
        r#"<span class="l">Purchased at</span>40,000 mi"#.to_owned(),
        format!(
            r#"<span class="l">Odometer</span>61,000 mi on {}"#,
            day_label(10)
        ),
        // The readings climb 1,000 miles in 90 days, and the last is ten
        // days old.
        r#"<span class="l">Estimated today</span>about 61,111 mi (estimated)"#.to_owned(),
        format!(
            r#"<span class="l">Warranty ends</span>72,000 mi or {}"#,
            day_label(900)
        ),
        format!(r#"<a href="/vehicles/{golf}/services/{service}">Brakes &#38; glass</a>"#),
    ] {
        assert!(body.contains(&detail), "missing {detail}: {body}");
    }
    assert_eq!(
        section_rows(&body, "Costs"),
        [
            "Total spent $500.00".to_owned(),
            "Service invoices 1 service $500.00".to_owned(),
            "Parts bought outside an invoice $0.00".to_owned(),
            "You paid $300.00".to_owned(),
            "Paid by others $200.00".to_owned(),
            format!("Per 1,000 miles 1,000 mi since {} $500.00", day_label(100)),
        ]
    );
    assert_eq!(
        section_rows(&body, "Recent activity"),
        [
            format!("No note Odometer reading 61,000 mi {}", day_label(10)),
            format!("Squeal Incident {}", day_label(20)),
            format!(
                "Brakes &#38; glass Service 60,500 mi {} $500.00",
                day_label(50)
            ),
            format!("No note Odometer reading 60,000 mi {}", day_label(100)),
        ]
    );
    for absent in ["<form", "<script", ">Garage</span>Archived", ">Sold<"] {
        assert!(!body.contains(absent), "{absent} is shown: {body}");
    }
}

// A service with a blank description still has text to click, and a reading
// with notes shows them where one without shows "No note".
#[tokio::test]
async fn a_vehicle_pages_activity_names_a_blank_service_and_shows_a_readings_notes() {
    use glovebox_shared::{inputs::service_record::NewServiceRecord, services::service_record};
    let db = db().await;
    let v = VehicleFixture::new().name("Golf").insert_id(&db).await;
    let blank = NewServiceRecord {
        service_date: days_before_today(30),
        description: Some("  ".into()),
        ..Default::default()
    };
    let service = service_record::create(&db, v, blank).await.unwrap();
    let entry = NewMileageEntry {
        mileage: 61_000,
        recorded_at: Some(days_before_today(10).into()),
        source: None,
        notes: Some("After the trip".into()),
    };
    mileage::create(&db, v, entry).await.unwrap();
    log_reading(&db, v, 60_000, 60).await;
    let (_, body) = page(app(db, "test"), get(&format!("/vehicles/{v}"))).await;
    assert!(
        body.contains(&format!(
            r#"<a href="/vehicles/{v}/services/{}">Service on {}</a>"#,
            service.record.id,
            day_label(30)
        )),
        "{body}"
    );
    assert!(
        body.contains(r#"<span class="key at-key"><span class="empty">No note</span></span>"#),
        "{body}"
    );
    assert_eq!(
        section_rows(&body, "Recent activity"),
        [
            format!(
                "After the trip Odometer reading 61,000 mi {}",
                day_label(10)
            ),
            format!("Service on {} Service {}", day_label(30), day_label(30)),
            format!("No note Odometer reading 60,000 mi {}", day_label(60)),
        ]
    );
}

#[tokio::test]
async fn a_vehicle_page_omits_its_empty_sections() {
    let db = db().await;
    let v = VehicleFixture::new().name("Golf").insert_id(&db).await;
    let (status, body) = page(app(db, "test"), get(&format!("/vehicles/{v}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(r#"<h1 id="vehicle-title">Golf</h1>"#),
        "{body}"
    );
    assert!(
        body.contains(r#"<span class="l">Odometer</span><span class="empty">No reading</span>"#),
        "{body}"
    );
    for absent in [
        "<h2",
        ">VIN<",
        ">Plate<",
        ">Color<",
        ">Purchased<",
        ">Purchased at<",
        ">Estimated today<",
        ">Warranty ends<",
        ">Sold<",
    ] {
        assert!(!body.contains(absent), "{absent} is shown: {body}");
    }
}

#[tokio::test]
async fn an_archived_vehicles_page_says_so_with_its_sale() {
    use glovebox_shared::{inputs::vehicle::VehicleSale, services::vehicle};
    let db = db().await;
    let (_, golf, _) = vehicle_fixture(&db).await;
    let sale = VehicleSale {
        sold_date: Some(days_before_today(5)),
        sold_mileage: Some(61_050),
        ..Default::default()
    };
    vehicle::archive(&db, golf, sale).await.unwrap();
    let (status, body) = page(app(db, "test"), get(&format!("/vehicles/{golf}"))).await;
    assert_eq!(status, StatusCode::OK);
    for detail in [
        r#"<span class="l">Garage</span>Archived"#.to_owned(),
        format!(r#"<span class="l">Sold</span>{}"#, day_label(5)),
        r#"<span class="l">Sold at</span>61,050 mi"#.to_owned(),
        format!(
            r#"<span class="l">Odometer</span>61,000 mi on {}"#,
            day_label(10)
        ),
    ] {
        assert!(body.contains(&detail), "missing {detail}: {body}");
    }
    // A sold car's mileage is not projected.
    assert!(!body.contains("Estimated today"), "{body}");
    assert_eq!(section_rows(&body, "Recent activity").len(), 4);
}

#[tokio::test]
async fn a_missing_vehicle_is_the_not_found_page() {
    let db = db().await;
    let (status, body) = page(app(db, "test"), get("/vehicles/999")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("Page not found"), "{body}");
}

#[tokio::test]
async fn the_home_service_job_and_appointment_pages_link_to_the_vehicle_page() {
    let db = db().await;
    let (_, golf, service, brakes, _) = service_fixture(&db).await;
    let planned = appointment::create(
        &db,
        golf,
        NewAppointment {
            purpose: "Winter tires".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let (_, home) = page(app(db.clone(), "test"), get("/")).await;
    assert!(
        home.contains(&format!(r#"<h2><a href="/vehicles/{golf}">Golf</a></h2>"#)),
        "{home}"
    );
    let labeled = format!(r#"<span class="l">Vehicle</span><a href="/vehicles/{golf}">Golf</a>"#);
    for path in [
        service_path(golf, service),
        format!("/vehicles/{golf}/jobs/{brakes}"),
        format!("/vehicles/{golf}/appointments/{planned}"),
    ] {
        let (status, body) = page(app(db.clone(), "test"), get(&path)).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(body.contains(&labeled), "{path}: {body}");
    }
}

// The vehicle fixture with a self-performed cabin filter change 300 days ago
// that records no total, a battery on the car and a wiper set that came off
// it. Returns (Smith Auto, the Golf, the shop's service, the owner's service).
async fn history_fixture(db: &DatabaseConnection) -> (i32, i32, i32, i32) {
    use glovebox_shared::{
        entities::service_record::Performer,
        inputs::{
            part::{Install, NewFitting, NewPart},
            service_record::NewServiceRecord,
        },
        services::{part, service_record},
    };
    let (smith, golf, brakes) = vehicle_fixture(db).await;
    let filter = service_record::create(
        db,
        golf,
        NewServiceRecord {
            service_date: days_before_today(300),
            performed_by: Some(Performer::Owner),
            mileage: Some(55_000),
            description: Some("Cabin filter".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .record
    .id;
    let fitted = |days_ago, odometer| Install {
        date: Some(days_before_today(days_ago)),
        odometer: Some(odometer),
        ..Install::default()
    };
    for (name, number, fitting) in [
        (
            "Battery",
            "S5 A11",
            NewFitting::Installed(fitted(200, 58_000)),
        ),
        (
            "Wiper set",
            "A297S",
            NewFitting::Replaced(fitted(400, 50_000)),
        ),
    ] {
        let input = NewPart {
            name: name.into(),
            manufacturer: Some("Bosch".into()),
            part_number: Some(number.into()),
            fitting,
            ..Default::default()
        };
        part::create(db, golf, input).await.unwrap();
    }
    (smith, golf, brakes, filter)
}

fn history_path(v: i32) -> String {
    format!("/vehicles/{v}/history")
}

#[tokio::test]
async fn a_history_page_lists_every_service_oldest_first_and_the_parts_on_the_car() {
    let db = db().await;
    let (smith, golf, brakes, filter) = history_fixture(&db).await;
    let (status, body) = page(app(db, "test"), get(&history_path(golf))).await;
    assert_eq!(status, StatusCode::OK);
    for detail in [
        "<h1>Service history</h1>".to_owned(),
        r#"<p class="ws-car">2017 Volkswagen Golf GTI SE</p>"#.to_owned(),
        r#"<span class="l">VIN</span> 3VW447AU9HM012345"#.to_owned(),
        r#"<span class="l">Printed</span> 2026-11-13"#.to_owned(),
        r#"<span class="l">Services</span> 2"#.to_owned(),
        format!(
            r#"<span class="l">Dates</span> {} to {}"#,
            day_label(300),
            day_label(50)
        ),
        r#"<span class="l">Odometer</span> 55,000 mi to 60,500 mi"#.to_owned(),
        format!(
            r#"<a href="/vehicles/{golf}/services/{filter}">{}</a><span>Cabin filter</span>"#,
            day_label(300)
        ),
        r#"<span class="l">Odometer</span> 55,000 mi</span>"#.to_owned(),
        "<span>Self-performed</span>".to_owned(),
        format!(
            r#"<a href="/vehicles/{golf}/services/{brakes}">{}</a><span>Brakes &#38; glass</span>"#,
            day_label(50)
        ),
        format!(r#"<span class="l">Shop</span> <a href="/shops/{smith}">Smith Auto</a>"#),
        r#"<span class="l">Total</span> <span class="price">$500.00</span>"#.to_owned(),
        r#"<li>Windshield <span class="l">Repair</span></li>"#.to_owned(),
        format!(
            r#"<li>Battery<span class="pn">Bosch S5 A11</span> <span class="l">Installed</span> 58,000 mi on {}</li>"#,
            day_label(200)
        ),
    ] {
        assert!(body.contains(&detail), "missing {detail}: {body}");
    }
    let at = |text: &str| {
        body.find(text)
            .unwrap_or_else(|| panic!("no {text}: {body}"))
    };
    assert!(at("Cabin filter") < at("Brakes &#38; glass"), "{body}");
    assert!(at("Brakes &#38; glass") < at("Parts on the car"), "{body}");
    // One service records a total, and only the battery is on the car.
    assert_eq!(body.matches(r#"<span class="l">Total</span>"#).count(), 1);
    for absent in ["Wiper set", "<form", "<script", r#"class="pill pill--ok"#] {
        assert!(!body.contains(absent), "{absent} is shown: {body}");
    }
}

// The links that leave the history sit in the strip that paper leaves out.
#[tokio::test]
async fn a_vehicle_page_links_to_its_history_and_the_history_links_back_on_screen_only() {
    let db = db().await;
    let (_, golf, _, _) = history_fixture(&db).await;
    let (_, vehicle) = page(app(db.clone(), "test"), get(&format!("/vehicles/{golf}"))).await;
    assert!(
        vehicle.contains(&format!(
            r#"<a href="/vehicles/{golf}/history">Service history</a>"#
        )),
        "{vehicle}"
    );
    let (_, body) = page(app(db, "test"), get(&history_path(golf))).await;
    let (strip, sheet) = body.split_once(r#"<article class="ws page">"#).unwrap();
    let strip = strip.split_once(r#"<div class="noprint page">"#).unwrap().1;
    for link in [
        r#"<a href="/">Garage</a>"#.to_owned(),
        format!(r#"<a href="/vehicles/{golf}">Steve&#39;s Golf</a>"#),
    ] {
        assert!(strip.contains(&link), "missing {link}: {strip}");
    }
    assert!(strip.contains("use the browser's print command"), "{strip}");
    assert!(!sheet.contains("Steve&#39;s Golf"), "{sheet}");
}

#[tokio::test]
async fn a_history_page_omits_its_empty_sections() {
    let db = db().await;
    let v = VehicleFixture::new().name("Golf").insert_id(&db).await;
    let (status, body) = page(app(db, "test"), get(&history_path(v))).await;
    assert_eq!(status, StatusCode::OK);
    for detail in [
        r#"<p class="ws-car">Golf</p>"#,
        r#"<span class="l">Services</span> 0"#,
        "<p>No services recorded.</p>",
    ] {
        assert!(body.contains(detail), "missing {detail}: {body}");
    }
    for absent in [
        "Parts on the car",
        r#"class="ws-job""#,
        ">VIN<",
        ">Dates<",
        ">Odometer<",
    ] {
        assert!(!body.contains(absent), "{absent} is shown: {body}");
    }
}

#[tokio::test]
async fn a_missing_vehicles_history_is_the_not_found_page() {
    let db = db().await;
    let (status, body) = page(app(db, "test"), get(&history_path(999))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("Page not found"), "{body}");
}

// A self-performed service on `vehicle`, `days_ago` days before the page's
// clock, with its description. The result is its id.
async fn plain_service(db: &DatabaseConnection, vehicle: i32, days_ago: u64, text: &str) -> i32 {
    use glovebox_shared::{
        entities::service_record::Performer, inputs::service_record::NewServiceRecord,
        services::service_record,
    };
    service_record::create(
        db,
        vehicle,
        NewServiceRecord {
            service_date: days_before_today(days_ago),
            performed_by: Some(Performer::Owner),
            description: Some(text.into()),
            total_cost_cents: Some(4_500),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .record
    .id
}

async fn planned_job(db: &DatabaseConnection, vehicle: i32, title: &str, cents: Option<i32>) {
    let job = NewWorkItem {
        title: title.into(),
        est_cost_cents: cents,
        ..Default::default()
    };
    work_item::create(db, vehicle, job).await.unwrap();
}

#[tokio::test]
async fn home_shows_each_cars_budget_and_latest_activity() {
    let db = db().await;
    let v = VehicleFixture::new().name("Golf").insert_id(&db).await;
    planned_job(&db, v, "Brakes", Some(65_000)).await;
    planned_job(&db, v, "Wipers", None).await;
    let oldest = plain_service(&db, v, 300, "Oldest service").await;
    let mut latest = Vec::new();
    for (days_ago, text) in [
        (200, "Tire rotation"),
        (100, "Oil change"),
        (20, "Inspection"),
    ] {
        latest.push((plain_service(&db, v, days_ago, text).await, text));
    }

    let (status, body) = page(app(db, "test"), get("/")).await;

    assert_eq!(status, StatusCode::OK);
    for detail in [
        "<h3>Budget and latest activity</h3>".to_owned(),
        format!(r#"<a class="aside" href="/vehicles/{v}">All costs and activity</a>"#),
        r#"<span class="l">Next 12 months</span><span class="price">At least $650.00</span>"#
            .to_owned(),
        r#"<span class="l">Scheduled maintenance</span><span class="price">$0.00</span>"#
            .to_owned(),
        r#"<span class="l">Planned work</span><span class="price">$650.00</span>"#.to_owned(),
        r#"<span class="l">Not in the total</span>1 item has no estimate"#.to_owned(),
        day_label(20),
    ] {
        assert!(body.contains(&detail), "missing {detail}: {body}");
    }
    for (id, text) in latest {
        let link = format!(r#"<a href="/vehicles/{v}/services/{id}">{text}</a>"#);
        assert!(body.contains(&link), "missing {link}: {body}");
    }
    // Only the three latest records are listed.
    assert!(!body.contains("Oldest service"), "{body}");
    assert!(!body.contains(&format!("/services/{oldest}\"")), "{body}");
    // The panel comes after the waiting lists and before the appointments.
    let at = |text: &str| {
        body.find(text)
            .unwrap_or_else(|| panic!("no {text}: {body}"))
    };
    assert!(at("<h3>Open incidents</h3>") < at("<h3>Budget and latest activity</h3>"));
    assert!(at("<h3>Budget and latest activity</h3>") < at("<h3>Appointments</h3>"));
}

#[tokio::test]
async fn home_prints_an_exact_budget_when_every_item_has_an_estimate() {
    let db = db().await;
    let v = VehicleFixture::new().name("Golf").insert_id(&db).await;
    planned_job(&db, v, "Brakes", Some(123_456)).await;

    let (_, body) = page(app(db, "test"), get("/")).await;

    assert!(
        body.contains(
            r#"<span class="l">Next 12 months</span><span class="price">$1,234.56</span>"#
        ),
        "{body}"
    );
    for absent in ["At least", "Not in the total", ">Record<"] {
        assert!(!body.contains(absent), "{absent} is shown: {body}");
    }
}

#[tokio::test]
async fn home_shows_no_budget_or_activity_for_a_car_with_neither() {
    let db = db().await;
    VehicleFixture::new().name("Golf").insert_id(&db).await;

    let (status, body) = page(app(db, "test"), get("/")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Golf"), "{body}");
    for absent in [
        "Budget and latest activity",
        "Next 12 months",
        "All costs and activity",
    ] {
        assert!(!body.contains(absent), "{absent} is shown: {body}");
    }
}

#[tokio::test]
async fn home_lists_no_budget_or_activity_of_an_archived_car() {
    let db = db().await;
    let sold = VehicleFixture::new().name("Old Jetta").insert_id(&db).await;
    plain_service(&db, sold, 30, "Jetta oil change").await;
    glovebox_shared::services::vehicle::archive(
        &db,
        sold,
        glovebox_shared::inputs::vehicle::VehicleSale::default(),
    )
    .await
    .unwrap();

    let (status, body) = page(app(db, "test"), get("/")).await;

    assert_eq!(status, StatusCode::OK);
    for absent in [
        "Old Jetta",
        "Jetta oil change",
        "Budget and latest activity",
        "$45.00",
    ] {
        assert!(!body.contains(absent), "{absent} is shown: {body}");
    }
}
