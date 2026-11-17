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
            r#"<span data-l="Status"><span class="pill pill--planning">Planning</span></span>"#
        ),
        "{body}"
    );
    assert!(
        body.contains(r#"<span class="num" data-l="Jobs">1</span>"#),
        "{body}"
    );
    assert!(
        body.contains(
            r#"<span class="num" data-l="Needs open"><span class="n n--todo">2</span></span>"#
        ),
        "{body}"
    );
    assert!(
        body.contains(
            r#"<span class="num" data-l="To-dos open"><span class="n n--todo">1</span></span>"#
        ),
        "{body}"
    );
    // An appointment with nothing open shows plain zeros.
    assert!(
        body.contains(r#"<span class="num" data-l="Needs open"><span class="n">0</span></span>"#),
        "{body}"
    );
    assert!(body.contains("Winter tires"), "{body}");
    assert!(
        body.contains(r#"<span data-l="Date"><span class="empty cap">not booked</span></span>"#),
        "{body}"
    );
    assert!(body.contains("Jobs not scheduled"), "{body}");
    assert!(body.contains("Replace wiper blades"), "{body}");
    assert!(body.contains("<span>Job</span>"), "{body}");
    assert!(
        body.contains(
            r#"<span class="at-job num" data-l="Needs open"><span class="n n--todo">1</span></span>"#
        ),
        "{body}"
    );
    assert!(body.contains("Wash the car"), "{body}");
    assert!(
        body.contains(r#"<span data-l="Date">2026-12-01</span>"#),
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
    let template = model_template::list(db)
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.model.as_deref() == Some("Golf GTI"))
        .expect("seeded GTI template");
    let v = VehicleFixture::new()
        .name("Golf GTI")
        .model_template_id(template.id)
        .purchase_date("2017-03-05")
        .purchase_mileage(40)
        .insert_id(db)
        .await;
    mileage::create(
        db,
        v,
        NewMileageEntry {
            mileage: 64000,
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
    let template = model_template::list(&db)
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.model.as_deref() == Some("Golf GTI"))
        .expect("seeded GTI template");
    let v = VehicleFixture::new()
        .name("Golf GTI")
        .model_template_id(template.id)
        .purchase_date("2024-11-23")
        .purchase_mileage(40)
        .insert_id(&db)
        .await;
    mileage::create(
        &db,
        v,
        NewMileageEntry {
            mileage: 1000,
            recorded_at: Some(common::today().into()),
            source: None,
            notes: None,
        },
    )
    .await
    .unwrap();

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
        block.contains(r#"<span class="pill pill--overdue">Overdue</span>"#),
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
        jobs.contains(r#"<span class="pill pill--overdue">Overdue</span>"#),
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
