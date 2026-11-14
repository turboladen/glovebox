mod common;

use axum::{body::Body, http::StatusCode};
use common::{app, db, get_page, page, post_form, request};
use glovebox_shared::{
    db::DatabaseConnection,
    inputs::{
        appointment::NewAppointment,
        need::{NewNeed, NewOption, OrderDetails, UpdateNeed},
        part::PartFilter,
        work_item::NewWorkItem,
    },
    services::{appointment, need, part, todo, work_item},
    test_support::{self, VehicleFixture},
};

// Everything the tests share: one vehicle with a booked appointment, one job
// on it, and one need supplied by the owner with a fitting and a
// non-fitting option.
struct World {
    db: DatabaseConnection,
    vehicle: i32,
    appointment: i32,
    job: i32,
    need: i32,
    good: i32,
    bad: i32,
}

impl World {
    async fn new() -> Self {
        let db = db().await;
        let vehicle = VehicleFixture::new().name("Golf GTI").insert_id(&db).await;
        let appointment = appointment::create(
            &db,
            vehicle,
            NewAppointment {
                purpose: "60k service".into(),
                shop_name: Some("Demo Auto Works".into()),
                planned_date: Some("2026-12-01".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        let job = work_item::create(
            &db,
            vehicle,
            NewWorkItem {
                title: "Brakes".into(),
                appointment_id: Some(appointment),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        let need = need::add(
            &db,
            vehicle,
            job,
            NewNeed {
                name: "Front pads".into(),
                supplied_by: Some("self".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        let good = need::add_option(
            &db,
            vehicle,
            need,
            NewOption {
                description: "Ceramic front pads".into(),
                manufacturer: Some("Akebono".into()),
                part_number: Some("ACT1633".into()),
                seller: Some("FCP Euro".into()),
                price_cents: Some(7_499),
                fitment: Some("confirmed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        let bad = need::add_option(
            &db,
            vehicle,
            need,
            NewOption {
                description: "Wrong pads".into(),
                manufacturer: Some("Zimmermann".into()),
                part_number: Some("WRONG-1".into()),
                price_cents: Some(11_200),
                fitment: Some("does_not_fit".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        Self {
            db,
            vehicle,
            appointment,
            job,
            need,
            good,
            bad,
        }
    }

    fn page_path(&self) -> String {
        format!(
            "/vehicles/{}/appointments/{}",
            self.vehicle, self.appointment
        )
    }

    fn job_path(&self) -> String {
        format!("/vehicles/{}/jobs/{}", self.vehicle, self.job)
    }

    fn need_path(&self, action: &str) -> String {
        format!("/vehicles/{}/needs/{}/{action}", self.vehicle, self.need)
    }

    async fn choose_good(&self) {
        need::decide(&self.db, self.vehicle, self.need, Some(self.good), None)
            .await
            .unwrap();
    }

    async fn order_it(&self) {
        need::mark_ordered(
            &self.db,
            self.vehicle,
            self.need,
            OrderDetails {
                seller: Some("FCP Euro".into()),
                purchase_date: Some("2026-11-13".into()),
                cost_cents: Some(7_499),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    async fn part_count(&self) -> usize {
        part::list(&self.db, self.vehicle, PartFilter::default())
            .await
            .unwrap()
            .len()
    }

    async fn get(&self, path: &str) -> (StatusCode, String) {
        get_page(&self.db, path).await
    }

    async fn post(
        &self,
        path: &str,
        fields: &[(&str, &str)],
    ) -> (StatusCode, Option<String>, String) {
        post_form(&self.db, path, fields).await
    }
}

// The markup of one next-steps group, from its heading to the end of its box.
fn group<'a>(body: &'a str, name: &str) -> &'a str {
    let marker = format!(r#"id="g-{name}""#);
    let Some(start) = body.find(&marker) else {
        return "";
    };
    let rest = &body[start..];
    &rest[..rest.find("</section>").unwrap_or(rest.len())]
}

#[tokio::test]
async fn appointment_page_shows_next_steps_then_jobs() {
    let w = World::new().await;
    todo::add(
        &w.db,
        w.vehicle,
        glovebox_shared::inputs::todo::NewTodo {
            title: "Bring the wheel lock key".into(),
            appointment_id: Some(w.appointment),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let (status, body) = w.get(&w.page_path()).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("60k service"), "{body}");
    assert!(
        body.contains("Golf GTI at Demo Auto Works on 2026-12-01"),
        "{body}"
    );
    assert!(body.contains("Planning"), "{body}");
    let steps = body.find("Next steps").expect("next steps heading");
    let jobs = body.find("By job").expect("by-job heading");
    assert!(steps < jobs, "next steps come before the jobs");
    // The row splits the need's name from the rest of its summary.
    let decide = group(&body, "decide");
    assert!(
        decide.contains(r#"<p class="step-need">Front pads</p>"#),
        "{decide}"
    );
    assert!(decide.contains("who supplies it is not decided") || decide.contains("no part chosen"));
    assert!(!decide.contains("Front pads: "), "{decide}");
    assert!(
        decide.contains(r#"<span class="tag">Brakes</span>"#),
        "{decide}"
    );
    assert!(group(&body, "todos").contains("Bring the wheel lock key"));
    assert!(body.contains("Supplier"), "{body}");
}

#[tokio::test]
async fn appointment_header_omits_a_missing_shop_and_date() {
    let w = World::new().await;
    let loose = appointment::create(
        &w.db,
        w.vehicle,
        NewAppointment {
            purpose: "Winter tires".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;

    let (status, body) = w
        .get(&format!("/vehicles/{}/appointments/{loose}", w.vehicle))
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Golf GTI<"), "{body}");
    assert!(!body.contains("Golf GTI at"), "{body}");
    assert!(!body.contains("Golf GTI on"), "{body}");
}

#[tokio::test]
async fn check_group_appears_only_with_warnings() {
    let w = World::new().await;
    let (_, quiet) = w.get(&w.page_path()).await;
    assert!(!quiet.contains(r#"id="g-check""#), "{quiet}");

    // An unconfirmed chosen option raises a warning.
    let unsure = need::add_option(
        &w.db,
        w.vehicle,
        w.need,
        NewOption {
            description: "Unsure pads".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    need::decide(&w.db, w.vehicle, w.need, Some(unsure), None)
        .await
        .unwrap();

    let (_, body) = w.get(&w.page_path()).await;

    let check = group(&body, "check");
    assert!(check.contains("Front pads"), "{body}");
    assert!(
        check.contains("Fitment of the chosen option is not confirmed."),
        "{check}"
    );
    assert!(!check.contains("Front pads: "), "{check}");
    assert!(
        body.find(r#"id="g-check""#) < body.find(r#"id="g-order""#),
        "{body}"
    );
}

#[tokio::test]
async fn supplier_and_approach_are_saved_and_cleared() {
    let w = World::new().await;
    let back = w.page_path();

    let (status, location, _) = w
        .post(
            &w.need_path("supplier"),
            &[
                ("supplied_by", "shop"),
                ("approach", "upgrade"),
                ("back", &back),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some(back.as_str()));
    let (_, body) = w.get(&back).await;
    assert!(body.contains("Shop"), "{body}");
    assert!(body.contains("Upgrade"), "{body}");
    assert!(group(&body, "done").contains("Front pads"), "{body}");

    w.post(
        &w.need_path("supplier"),
        &[("supplied_by", ""), ("approach", ""), ("back", &back)],
    )
    .await;
    let (_, body) = w.get(&back).await;
    assert!(body.contains("Supplier not decided"), "{body}");
    assert!(body.contains("Approach not decided"), "{body}");

    let (status, _, body) = w
        .post(
            &w.need_path("supplier"),
            &[("supplied_by", "robot"), ("approach", ""), ("back", &back)],
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body.contains("Not saved"), "{body}");
}

#[tokio::test]
async fn choosing_an_option_moves_the_need_from_decide_to_order() {
    let w = World::new().await;
    let back = w.page_path();
    let (_, before) = w.get(&back).await;
    assert!(group(&before, "decide").contains("Front pads"), "{before}");
    assert!(!group(&before, "order").contains("Front pads"));

    let good = w.good.to_string();
    let (status, location, _) = w
        .post(
            &w.need_path("decision"),
            &[
                ("option_id", &good),
                ("reason", "Quiet and low dust."),
                ("back", &back),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some(back.as_str()));
    let (_, after) = w.get(&back).await;
    assert!(!group(&after, "decide").contains("Front pads"), "{after}");
    let order = group(&after, "order");
    assert!(order.contains("Front pads"), "{after}");
    assert!(
        order.contains("order Akebono ACT1633 from FCP Euro"),
        "{order}"
    );
    assert!(after.contains("Quiet and low dust."), "{after}");

    // An empty option clears the choice again.
    w.post(
        &w.need_path("decision"),
        &[("option_id", ""), ("reason", ""), ("back", &back)],
    )
    .await;
    let (_, cleared) = w.get(&back).await;
    assert!(
        group(&cleared, "decide").contains("Front pads"),
        "{cleared}"
    );
}

#[tokio::test]
async fn a_need_for_two_shows_its_count_and_the_price_of_both() {
    let w = World::new().await;
    need::update(
        &w.db,
        w.vehicle,
        w.need,
        UpdateNeed {
            quantity: Some(2),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    w.choose_good().await;

    let (_, body) = w.get(&w.page_path()).await;

    let order = group(&body, "order");
    assert!(
        order.contains("order 2 \u{d7} Akebono ACT1633 from FCP Euro"),
        "{order}"
    );
    assert!(
        body.contains(r#"<span class="qty">2 needed</span>"#),
        "{body}"
    );
    assert!(body.contains(r#"value="$149.98""#), "{body}");
    assert!(body.contains("Price for 2"), "{body}");
    assert!(body.contains("2 \u{d7} $74.99"), "{body}");
}

#[tokio::test]
async fn a_need_for_one_shows_no_count() {
    let w = World::new().await;
    w.choose_good().await;

    let (_, body) = w.get(&w.page_path()).await;

    assert!(!body.contains(r#"class="qty""#), "{body}");
    assert!(body.contains(r#"value="$74.99""#), "{body}");
    assert!(!body.contains("Price for"), "{body}");
}

#[tokio::test]
async fn an_option_link_is_shown_only_for_a_web_address() {
    let w = World::new().await;
    for (description, url) in [
        ("Linked pads", "https://example.com/pads?a=1&b=2"),
        ("Scripted pads", "javascript:alert(1)"),
    ] {
        need::add_option(
            &w.db,
            w.vehicle,
            w.need,
            NewOption {
                description: description.into(),
                url: Some(url.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    let (_, body) = w.get(&w.page_path()).await;

    assert!(
        body.contains(
            r#"<a class="opt-link" href="https://example.com/pads?a=1&#38;b=2" target="_blank" rel="noopener noreferrer">"#
        ),
        "{body}"
    );
    assert!(!body.contains("javascript:"), "{body}");
}

#[tokio::test]
async fn editing_only_the_reason_keeps_the_decision_date() {
    let w = World::new().await;
    let back = w.page_path();
    let good = w.good.to_string();
    let path = w.need_path("decision");
    w.post(
        &path,
        &[("option_id", &good), ("reason", "Quiet."), ("back", &back)],
    )
    .await;
    // A decision made earlier than today, so a moved date would show.
    test_support::set_decided_on(&w.db, w.need, "2026-01-02").await;

    let (status, _, _) = w
        .post(
            &path,
            &[
                ("option_id", &good),
                ("reason", "Quiet and low dust."),
                ("back", &back),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    let stored = need::require(&w.db, w.vehicle, w.need).await.unwrap();
    assert_eq!(stored.decided_on.as_deref(), Some("2026-01-02"));
    assert_eq!(
        stored.decision_reason.as_deref(),
        Some("Quiet and low dust.")
    );
}

#[tokio::test]
async fn choosing_a_does_not_fit_option_is_refused() {
    let w = World::new().await;
    let bad = w.bad.to_string();

    let (status, _, body) = w
        .post(
            &w.need_path("decision"),
            &[
                ("option_id", &bad),
                ("reason", ""),
                ("back", &w.page_path()),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body.contains("Not saved"), "{body}");
    assert!(group(&body, "decide").contains("Front pads"), "{body}");
}

#[tokio::test]
async fn does_not_fit_option_is_listed_but_cannot_be_selected() {
    let w = World::new().await;

    let (_, body) = w.get(&w.page_path()).await;

    assert!(body.contains("WRONG-1"), "the option is listed");
    assert!(body.contains("Does not fit"), "{body}");
    let bad = format!(r#"name="option_id" value="{}" disabled"#, w.bad);
    let good = format!(r#"name="option_id" value="{}""#, w.good);
    assert!(body.contains(&bad), "{body}");
    assert!(body.contains(&good), "{body}");
    assert!(
        !body.contains(&format!("{good} disabled")),
        "the fitting option stays selectable"
    );
}

#[tokio::test]
async fn ordering_creates_the_part_and_moves_the_need_to_arriving() {
    let w = World::new().await;
    w.choose_good().await;
    let back = w.page_path();
    let (_, before) = w.get(&back).await;
    // The form is prefilled from the chosen option.
    assert!(before.contains(r#"value="$74.99""#), "{before}");
    assert!(before.contains(r#"value="FCP Euro""#), "{before}");
    assert!(before.contains(r#"value="2026-11-13""#), "{before}");

    let (status, location, _) = w
        .post(
            &w.need_path("ordered"),
            &[
                ("seller", "FCP Euro"),
                ("purchase_date", "2026-11-13"),
                ("cost", "$74.99"),
                ("order_ref", "FCP-1"),
                ("back", &back),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some(back.as_str()));
    assert_eq!(w.part_count().await, 1);
    let (_, after) = w.get(&back).await;
    assert!(!group(&after, "order").contains("Front pads"), "{after}");
    assert!(group(&after, "arriving").contains("Front pads"), "{after}");
    assert!(
        after.contains("Bought from FCP Euro on 2026-11-13 for $74.99, order FCP-1"),
        "{after}"
    );
    assert!(!after.contains('\u{b7}'), "no dot separators remain");
}

#[tokio::test]
async fn ordering_with_bad_money_rerenders_422_and_writes_nothing() {
    let w = World::new().await;
    w.choose_good().await;
    let back = w.page_path();

    for cost in ["-5", "abc"] {
        let (status, location, body) = w
            .post(
                &w.need_path("ordered"),
                &[
                    ("seller", "Typed Seller"),
                    ("purchase_date", "2026-11-13"),
                    ("cost", cost),
                    ("order_ref", "TYPED-REF"),
                    ("back", &back),
                ],
            )
            .await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{cost}");
        assert_eq!(location, None);
        assert!(body.contains("Not saved"), "{body}");
        assert!(body.contains("Price"), "{body}");
        assert!(body.contains("Typed Seller"), "{body}");
        assert!(body.contains("TYPED-REF"), "{body}");
        assert!(body.contains(&format!(r#"value="{cost}""#)), "{body}");
        assert!(body.contains(r#"aria-invalid="true""#), "{body}");
        assert_eq!(w.part_count().await, 0, "{cost}");
    }
}

#[tokio::test]
async fn received_shows_an_undo_banner_and_undo_restores_arriving() {
    let w = World::new().await;
    w.choose_good().await;
    w.order_it().await;
    let back = w.page_path();
    let (_, before) = w.get(&back).await;
    assert!(
        group(&before, "arriving").contains("Front pads"),
        "{before}"
    );
    assert!(!before.contains(r#"role="status""#), "{before}");

    let (status, location, _) = w.post(&w.need_path("received"), &[("back", &back)]).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    let expected = format!("{back}?done=need-{}", w.need);
    assert_eq!(location.as_deref(), Some(expected.as_str()));
    let (_, after) = w.get(&expected).await;
    assert!(after.contains(r#"role="status""#), "{after}");
    assert!(after.contains("Front pads"), "{after}");
    assert!(after.contains(&w.need_path("received/undo")), "{after}");
    assert!(!group(&after, "arriving").contains("Front pads"), "{after}");
    assert!(group(&after, "done").contains("Front pads"), "{after}");

    let (status, location, _) = w
        .post(&w.need_path("received/undo"), &[("back", &back)])
        .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some(back.as_str()));
    let (_, restored) = w.get(&back).await;
    assert!(
        group(&restored, "arriving").contains("Front pads"),
        "{restored}"
    );
}

#[tokio::test]
async fn return_requires_a_reason() {
    let w = World::new().await;
    w.choose_good().await;
    w.order_it().await;

    let (status, _, body) = w
        .post(
            &w.need_path("return"),
            &[("reason", "   "), ("back", &w.page_path())],
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body.contains("Not saved"), "{body}");
    assert!(body.contains(r#"aria-invalid="true""#), "{body}");
    assert_eq!(w.part_count().await, 1);
    let (_, after) = w.get(&w.page_path()).await;
    assert!(group(&after, "arriving").contains("Front pads"), "{after}");
}

#[tokio::test]
async fn return_adds_the_todo_and_moves_the_need_back() {
    let w = World::new().await;
    w.choose_good().await;
    w.order_it().await;
    let back = w.page_path();

    let (status, location, _) = w
        .post(
            &w.need_path("return"),
            &[
                ("reason", "Wrong size"),
                ("option_was_wrong", "true"),
                ("back", &back),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some(back.as_str()));
    let (_, body) = w.get(&back).await;
    assert!(group(&body, "decide").contains("Front pads"), "{body}");
    assert!(!group(&body, "arriving").contains("Front pads"), "{body}");
    assert!(group(&body, "todos").contains("Return"), "{body}");
    let todos = todo::list(&w.db, w.vehicle, Some(Some(w.appointment)), false)
        .await
        .unwrap();
    assert_eq!(todos.len(), 1);
    assert!(group(&body, "todos").contains(&todos[0].title), "{body}");
}

#[tokio::test]
async fn todo_add_tick_undo_delete() {
    let w = World::new().await;
    let back = w.page_path();
    let appointment = w.appointment.to_string();

    let (status, location, _) = w
        .post(
            &format!("/vehicles/{}/todos", w.vehicle),
            &[
                ("title", "Bring wheel locks"),
                ("appointment_id", &appointment),
                ("back", &back),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some(back.as_str()));
    let id = todo::list(&w.db, w.vehicle, Some(Some(w.appointment)), false)
        .await
        .unwrap()[0]
        .id;
    let (_, body) = w.get(&back).await;
    assert!(
        group(&body, "todos").contains("Bring wheel locks"),
        "{body}"
    );

    let todo_path = |action: &str| format!("/vehicles/{}/todos/{id}/{action}", w.vehicle);
    let (status, location, _) = w.post(&todo_path("done"), &[("back", &back)]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let expected = format!("{back}?done=todo-{id}");
    assert_eq!(location.as_deref(), Some(expected.as_str()));
    let (_, ticked) = w.get(&expected).await;
    assert!(ticked.contains(r#"role="status""#), "{ticked}");
    assert!(ticked.contains(&todo_path("done/undo")), "{ticked}");
    assert!(
        !group(&ticked, "todos").contains("Bring wheel locks"),
        "{ticked}"
    );
    assert!(
        group(&ticked, "done").contains("Bring wheel locks"),
        "{ticked}"
    );

    let (status, _, _) = w.post(&todo_path("done/undo"), &[("back", &back)]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, undone) = w.get(&back).await;
    assert!(
        group(&undone, "todos").contains("Bring wheel locks"),
        "{undone}"
    );

    let (status, _, _) = w.post(&todo_path("delete"), &[("back", &back)]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, gone) = w.get(&back).await;
    assert!(!gone.contains("Bring wheel locks"), "{gone}");

    let (status, _, body) = w
        .post(
            &format!("/vehicles/{}/todos", w.vehicle),
            &[
                ("title", "  "),
                ("appointment_id", &appointment),
                ("back", &back),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body.contains("Not saved"), "{body}");
}

#[tokio::test]
async fn home_todos_can_be_added_ticked_and_deleted() {
    let w = World::new().await;
    let add_path = format!("/vehicles/{}/todos", w.vehicle);
    let (_, home) = w.get("/").await;
    assert!(
        home.contains(&format!(r#"action="{add_path}""#)),
        "the home page has an add form: {home}"
    );

    let (status, location, _) = w
        .post(
            &add_path,
            &[("title", "Return the HP Plus pads"), ("back", "/")],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/"));
    let id = todo::list(&w.db, w.vehicle, Some(None), false)
        .await
        .unwrap()[0]
        .id;
    let todo_path = |action: &str| format!("/vehicles/{}/todos/{id}/{action}", w.vehicle);
    let (_, home) = w.get("/").await;
    assert!(home.contains(&todo_path("done")), "{home}");
    assert!(home.contains(&todo_path("delete")), "{home}");

    let (status, location, _) = w.post(&todo_path("done"), &[("back", "/")]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let expected = format!("/?done=todo-{id}");
    assert_eq!(location.as_deref(), Some(expected.as_str()));
    let (_, ticked) = w.get(&expected).await;
    assert!(ticked.contains(r#"role="status""#), "{ticked}");
    assert!(ticked.contains(&todo_path("done/undo")), "{ticked}");

    w.post(&todo_path("done/undo"), &[("back", "/")]).await;
    let (status, _, _) = w.post(&todo_path("delete"), &[("back", "/")]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, gone) = w.get("/").await;
    assert!(!gone.contains("Return the HP Plus pads"), "{gone}");

    let (status, _, body) = w.post(&add_path, &[("title", " "), ("back", "/")]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body.contains("Not saved"), "{body}");
    assert!(
        body.contains("Open work"),
        "the home page is shown again: {body}"
    );
}

#[tokio::test]
async fn back_outside_the_site_falls_back_to_home() {
    let w = World::new().await;
    for back in [
        "https://evil.example/",
        "//evil.example/x",
        "/\\evil.example",
        "evil",
        "",
        "/with space",
        "/line\r\nbreak",
    ] {
        let (status, location, _) = w
            .post(
                &w.need_path("supplier"),
                &[("supplied_by", "self"), ("approach", ""), ("back", back)],
            )
            .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{back:?}");
        assert_eq!(location.as_deref(), Some("/"), "{back:?}");
    }
    let (_, location, _) = w
        .post(
            &w.need_path("supplier"),
            &[
                ("supplied_by", "self"),
                ("approach", ""),
                ("back", &w.job_path()),
            ],
        )
        .await;
    assert_eq!(location.as_deref(), Some(w.job_path().as_str()));
}

#[tokio::test]
async fn need_of_another_vehicle_is_404() {
    let w = World::new().await;
    let other = VehicleFixture::new()
        .name("Other car")
        .insert_id(&w.db)
        .await;
    let other_page = format!("/vehicles/{other}/appointments/{}", w.appointment);
    // The same request from the owning vehicle succeeds, so the 404s below
    // come from the wrong vehicle and not from a missing route.
    let (control, _, _) = w
        .post(
            &w.need_path("supplier"),
            &[("supplied_by", "self"), ("approach", ""), ("back", "/")],
        )
        .await;
    assert_eq!(control, StatusCode::SEE_OTHER);

    for action in [
        "supplier",
        "decision",
        "ordered",
        "received",
        "received/undo",
        "return",
    ] {
        let path = format!("/vehicles/{other}/needs/{}/{action}", w.need);
        let (status, _, body) = w
            .post(
                &path,
                &[
                    ("reason", "x"),
                    ("supplied_by", ""),
                    ("approach", ""),
                    ("back", "/"),
                ],
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{action}");
        assert!(!body.contains("Front pads"), "{action}: {body}");
        let (missing, _, _) = w
            .post(
                &format!("/vehicles/{other}/needs/99999/{action}"),
                &[
                    ("reason", "x"),
                    ("supplied_by", ""),
                    ("approach", ""),
                    ("back", "/"),
                ],
            )
            .await;
        assert_eq!(
            missing,
            StatusCode::NOT_FOUND,
            "{action} for a missing need"
        );
    }
    // Nothing changed.
    assert_eq!(w.part_count().await, 0);
    let (status, body) = w.get(&other_page).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!body.contains("60k service"), "{body}");
    let (status, _) = w.get(&format!("/vehicles/{other}/jobs/{}", w.job)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = w
        .post(
            &format!("/vehicles/{other}/todos/99999/done"),
            &[("back", "/")],
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn job_page_lists_the_jobs_needs() {
    let w = World::new().await;
    w.choose_good().await;
    w.order_it().await;

    let (status, body) = w.get(&w.job_path()).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Brakes"), "{body}");
    assert!(body.contains("Front pads"), "{body}");
    assert!(body.contains("Ordered"), "{body}");
    assert!(body.contains("Bought from FCP Euro"), "{body}");
    assert!(
        body.contains(&w.page_path()),
        "links back to the appointment"
    );
    assert!(
        body.contains(&format!(r#"name="back" value="{}""#, w.job_path())),
        "forms return to the job page: {body}"
    );
}

#[tokio::test]
async fn page_contains_no_script_tag() {
    let w = World::new().await;
    w.choose_good().await;
    let failed = w
        .post(
            &w.need_path("ordered"),
            &[("cost", "abc"), ("back", &w.page_path())],
        )
        .await
        .2;
    let (_, appointment) = w.get(&w.page_path()).await;
    let (_, job) = w.get(&w.job_path()).await;
    let (_, home) = w.get("/").await;
    assert!(appointment.contains("Front pads") && job.contains("Front pads"));

    for body in [failed, appointment, job, home] {
        assert!(body.contains("<html"), "{body}");
        let lower = body.to_ascii_lowercase();
        assert!(!lower.contains("<script"), "{body}");
        assert!(!lower.contains(" onclick="), "{body}");
    }
}

#[tokio::test]
async fn clearing_a_decision_with_the_prefilled_reason_clears_it() {
    let w = World::new().await;
    let back = w.page_path();
    let good = w.good.to_string();
    w.post(
        &w.need_path("decision"),
        &[("option_id", &good), ("reason", "Quiet."), ("back", &back)],
    )
    .await;

    // The form sends the stored reason along with the empty choice.
    let (status, _, body) = w
        .post(
            &w.need_path("decision"),
            &[("option_id", ""), ("reason", "Quiet."), ("back", &back)],
        )
        .await;

    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let (_, after) = w.get(&back).await;
    assert!(group(&after, "decide").contains("Front pads"), "{after}");
    assert!(!after.contains("Quiet."), "{after}");
}

#[tokio::test]
async fn a_need_name_with_a_colon_renders_whole() {
    let w = World::new().await;
    let oil = need::add(
        &w.db,
        w.vehicle,
        w.job,
        NewNeed {
            name: "Oil: 5W-30".into(),
            supplied_by: Some("self".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let option = need::add_option(
        &w.db,
        w.vehicle,
        oil,
        NewOption {
            description: "Any oil".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    need::decide(&w.db, w.vehicle, oil, Some(option), None)
        .await
        .unwrap();

    let (_, body) = w.get(&w.page_path()).await;

    let order = group(&body, "order");
    assert!(
        order.contains(r#"<p class="step-need">Oil: 5W-30</p>"#),
        "{order}"
    );
    assert!(order.contains("order Any oil"), "{order}");
    let check = group(&body, "check");
    assert!(
        check.contains(r#"<p class="step-need">Oil: 5W-30</p>"#),
        "{check}"
    );
    assert!(
        check.contains("Fitment of the chosen option is not confirmed."),
        "{check}"
    );
}

#[tokio::test]
async fn open_query_opens_the_need_and_the_decide_link_uses_it() {
    let w = World::new().await;
    let (_, closed) = w.get(&w.page_path()).await;
    assert!(
        closed.contains(&format!(
            r#"href="{1}?open=need-{0}#need-{0}-decide""#,
            w.need,
            w.page_path()
        )),
        "{closed}"
    );
    assert!(!closed.contains(&format!(r#"id="need-{}" open"#, w.need)));

    let (_, opened) = w
        .get(&format!("{}?open=need-{}", w.page_path(), w.need))
        .await;

    assert!(
        opened.contains(&format!(r#"id="need-{}" open"#, w.need)),
        "{opened}"
    );
}

#[tokio::test]
async fn a_need_with_no_options_asks_the_owner_to_ask_claude() {
    let w = World::new().await;
    let bare = need::add(
        &w.db,
        w.vehicle,
        w.job,
        NewNeed {
            name: "Wiper blades".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;

    let (_, body) = w.get(&w.page_path()).await;

    let decide = group(&body, "decide");
    assert!(decide.contains("Wiper blades"), "{decide}");
    assert!(
        decide.contains("ask Claude to research options"),
        "{decide}"
    );
    assert!(
        !body.contains(&format!("need-{bare}-decide")),
        "no link to a form that is not rendered"
    );
}

#[tokio::test]
async fn the_undo_banner_needs_the_need_received_or_the_todo_done() {
    let w = World::new().await;
    let loose = todo::add(
        &w.db,
        w.vehicle,
        glovebox_shared::inputs::todo::NewTodo {
            title: "Bring keys".into(),
            appointment_id: Some(w.appointment),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    for done in [format!("need-{}", w.need), format!("todo-{loose}")] {
        let (_, body) = w.get(&format!("{}?done={done}", w.page_path())).await;
        assert!(!body.contains(r#"role="status""#), "{done}: {body}");
    }

    w.choose_good().await;
    w.order_it().await;
    need::mark_received(&w.db, w.vehicle, w.need, None)
        .await
        .unwrap();
    todo::complete(&w.db, w.vehicle, loose, None).await.unwrap();
    for done in [format!("need-{}", w.need), format!("todo-{loose}")] {
        let (_, body) = w.get(&format!("{}?done={done}", w.page_path())).await;
        assert!(body.contains(r#"role="status""#), "{done}: {body}");
    }
}

#[tokio::test]
async fn a_failed_write_never_shows_another_vehicles_page() {
    let w = World::new().await;
    let other = VehicleFixture::new()
        .name("Other car")
        .insert_id(&w.db)
        .await;
    let other_appointment = appointment::create(
        &w.db,
        other,
        NewAppointment {
            purpose: "Other visit".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let foreign_back = format!("/vehicles/{other}/appointments/{other_appointment}");

    let (status, _, body) = w
        .post(
            &w.need_path("ordered"),
            &[("cost", "abc"), ("back", &foreign_back)],
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!body.contains("Other car"), "{body}");
    assert!(!body.contains("Other visit"), "{body}");
    assert!(body.contains("Not saved"), "{body}");
}

#[tokio::test]
async fn cross_origin_post_to_an_action_is_refused() {
    let w = World::new().await;
    w.choose_good().await;
    let req = request("POST", &w.need_path("ordered"))
        .header("origin", "http://evil.example")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from("seller=X&back=%2F"))
        .unwrap();

    let (status, _) = page(app(w.db.clone(), "test"), req).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(w.part_count().await, 0);
}

#[tokio::test]
async fn todo_of_another_vehicle_is_404_like_a_missing_one() {
    let w = World::new().await;
    let other = VehicleFixture::new()
        .name("Other car")
        .insert_id(&w.db)
        .await;
    let mine = todo::add(
        &w.db,
        w.vehicle,
        glovebox_shared::inputs::todo::NewTodo {
            title: "Mine".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;

    for action in ["done", "done/undo", "delete"] {
        let (real, _, real_body) = w
            .post(
                &format!("/vehicles/{other}/todos/{mine}/{action}"),
                &[("back", "/")],
            )
            .await;
        let (missing, _, missing_body) = w
            .post(
                &format!("/vehicles/{other}/todos/99999/{action}"),
                &[("back", "/")],
            )
            .await;
        assert_eq!(real, StatusCode::NOT_FOUND, "{action}");
        assert_eq!(missing, StatusCode::NOT_FOUND, "{action}");
        // The messages differ only in the id.
        let alert = |body: &str| {
            let start = body.find(r#"role="alert">"#).expect("an error block") + 13;
            body[start..start + body[start..].find("</p>").unwrap()].to_owned()
        };
        assert_eq!(
            alert(&real_body).replace(&mine.to_string(), "N"),
            alert(&missing_body).replace("99999", "N"),
            "{action}"
        );
    }
    let still = todo::list(&w.db, w.vehicle, Some(None), true)
        .await
        .unwrap();
    assert_eq!(still.len(), 1);
    assert!(still[0].done_on.is_none());
}

// A failed write answers at the form's own address, so a link on the page it
// shows must name the page's path.
#[tokio::test]
async fn the_links_on_a_page_shown_after_a_failed_write_lead_back_to_that_page() {
    let w = World::new().await;
    need::update(
        &w.db,
        w.vehicle,
        w.need,
        glovebox_shared::inputs::need::UpdateNeed {
            supplied_by: Some(None),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    w.choose_good().await;
    need::update_option(
        &w.db,
        w.vehicle,
        w.good,
        glovebox_shared::inputs::need::UpdateOption {
            fitment: Some("unconfirmed".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let (status, _, body) = w
        .post(
            &w.need_path("return"),
            &[("reason", "wrong size"), ("back", &w.page_path())],
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let page = w.page_path();
    let need = w.need;
    for link in [
        format!(r#"href="{page}?open=need-{need}#need-{need}-supplier""#),
        format!(r#"href="{page}?open=need-{need}#need-{need}""#),
    ] {
        assert!(body.contains(&link), "missing {link}: {body}");
    }
    assert!(!body.contains(r#"href="?"#), "{body}");
}

// The decision form and the return form both have a `reason` input.
#[tokio::test]
async fn a_failed_return_does_not_fill_the_decision_reason() {
    let w = World::new().await;

    let (status, _, body) = w
        .post(
            &w.need_path("return"),
            &[("reason", "wrong size"), ("back", &w.page_path())],
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!body.contains("wrong size"), "{body}");
}

// The need has moved on by the time the write arrives, so the page shown in
// answer leaves the form out and the message needs another place.
#[tokio::test]
async fn a_failed_write_whose_form_is_gone_still_shows_its_error() {
    let w = World::new().await;
    let returned = w
        .post(
            &w.need_path("return"),
            &[("reason", "wrong size"), ("back", &w.page_path())],
        )
        .await;
    assert_eq!(returned.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(returned.2.contains("Not saved"), "{}", returned.2);
    assert!(returned.2.contains("has no part linked"), "{}", returned.2);

    w.choose_good().await;
    w.order_it().await;
    let ordered_twice = w
        .post(
            &w.need_path("ordered"),
            &[("cost", "74.99"), ("back", &w.job_path())],
        )
        .await;
    assert_eq!(ordered_twice.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        ordered_twice.2.contains("already has a part linked"),
        "{}",
        ordered_twice.2
    );
    assert_eq!(w.part_count().await, 1);
}

#[tokio::test]
async fn a_failed_to_do_add_for_a_vehicle_the_home_page_does_not_list_shows_its_error() {
    let w = World::new().await;

    let (status, _, body) = w
        .post(
            "/vehicles/99999/todos",
            &[("title", "Rotate the tires"), ("back", "/")],
        )
        .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("Not saved"), "{body}");
    assert!(body.contains("Vehicle 99999 not found"), "{body}");
}
