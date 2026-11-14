mod common;

use axum::{body::Body, http::StatusCode};
use common::{app, db, get, page, request, send};
use glovebox_shared::{
    inputs::{appointment::NewAppointment, need::NewNeed, todo::NewTodo, work_item::NewWorkItem},
    services::{appointment, need, todo, work_item},
    test_support::VehicleFixture,
};

const CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; base-uri \
                   'none'; frame-ancestors 'none'";

#[tokio::test]
async fn home_lists_open_appointments_and_unscheduled_jobs() {
    let db = db().await;
    let v = VehicleFixture::new().name("Golf GTI").insert_id(&db).await;
    let a = appointment::create(
        &db,
        v,
        NewAppointment {
            purpose: "60k service".into(),
            planned_date: Some("2026-12-01".into()),
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
    assert!(body.contains("2 needs open \u{b7} 1 to-do open"), "{body}");
    assert!(body.contains("Winter tires"), "{body}");
    assert!(body.contains("not booked"), "{body}");
    assert!(body.contains("Not scheduled yet"), "{body}");
    assert!(body.contains("Replace wiper blades"), "{body}");
    assert!(body.contains("1 need open"), "{body}");
    assert!(body.contains("Wash the car"), "{body}");
    assert!(
        !body.contains("Brake job"),
        "a job on an appointment is not unscheduled: {body}"
    );
}

#[tokio::test]
async fn home_shows_env_name_only_when_not_live() {
    let (_, test_body) = page(app(db().await, "test"), get("/")).await;
    let (_, live_body) = page(app(db().await, "live"), get("/")).await;

    assert!(test_body.contains(r#"class="env""#), "{test_body}");
    assert!(!live_body.contains(r#"class="env""#), "{live_body}");
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
