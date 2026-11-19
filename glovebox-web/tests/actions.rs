mod common;

use axum::{Router, body::Body, http::StatusCode};
use common::{app, db, page, post_to, request};
use glovebox_shared::{
    db::DatabaseConnection,
    entities::{job_need::Approach, need_option::Fitment, part::Supplier},
    inputs::{
        appointment::NewAppointment,
        need::{NewNeed, NewOption, OrderDetails, UpdateNeed},
        part::{
            Fitting, InstallPatch, NewPart, NewPartContent, PartFilter, ReplacePatch, UpdatePart,
        },
        todo::NewTodo,
        work_item::NewWorkItem,
    },
    services::{appointment, need, part, todo, vehicle, work_item},
    test_support::{self, VehicleFixture, date},
};

// Everything the tests share: one vehicle with a booked appointment, one job
// on it, and one need supplied by the owner with a fitting and a
// non-fitting option.
struct World {
    db: DatabaseConnection,
    // One application for every request, so the undo of a deleted to-do,
    // which lives in the server, carries from one request to the next.
    app: Router,
    vehicle: i32,
    appointment: i32,
    job: i32,
    need: i32,
    good: i32,
    bad: i32,
}

impl World {
    // Closes the appointment once its one job is done, since an appointment
    // with open work refuses to close.
    async fn close_appointment(&self) {
        test_support::set_job_status(
            &self.db,
            self.vehicle,
            self.job,
            glovebox_shared::entities::work_item::JobStatus::Done,
        )
        .await;
        appointment::close(&self.db, self.vehicle, self.appointment)
            .await
            .unwrap();
    }

    async fn new() -> Self {
        let db = db().await;
        let vehicle = VehicleFixture::new().name("Golf GTI").insert_id(&db).await;
        let appointment = appointment::create(
            &db,
            vehicle,
            NewAppointment {
                purpose: "60k service".into(),
                shop_name: Some("Demo Auto Works".into()),
                planned_date: Some(date("2026-12-01")),
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
                supplied_by: Some(Supplier::Owner),
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
                fitment: Some(Fitment::Confirmed),
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
                fitment: Some(Fitment::DoesNotFit),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        Self {
            app: app(db.clone(), "test"),
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
                purchase_date: Some(date("2026-11-13")),
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
        page(self.app.clone(), common::get(path)).await
    }

    async fn post(
        &self,
        path: &str,
        fields: &[(&str, &str)],
    ) -> (StatusCode, Option<String>, String) {
        post_to(self.app.clone(), path, fields).await
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

// Returns the markup of one need's row in its job, without its details.
fn need_summary(body: &str, need_id: i32) -> &str {
    let rest = &body[body.find(&format!(r#"id="need-{need_id}""#)).unwrap()..];
    &rest[..rest.find("</summary>").unwrap()]
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
    for (label, value) in [
        ("Vehicle", "Golf GTI"),
        ("Shop", "Demo Auto Works"),
        ("Date", "2026-12-01"),
    ] {
        assert!(
            body.contains(&format!(r#"<span class="l">{label}</span>{value}</div>"#)),
            "{label}: {body}"
        );
    }
    assert!(
        body.contains(r#"<span class="l">Status</span><span class="pill pill--"#),
        "{body}"
    );
    for label in ["need open", "to-do open", "job"] {
        assert!(
            body.contains(&format!(r#"<span class="l cap">{label}</span>"#)),
            "{label}: {body}"
        );
    }
    assert!(
        body.contains(
            r#"<span class="l">Status</span><span class="pill pill--wait">Booked</span>"#
        ),
        "a dated appointment before its day reads booked: {body}"
    );
    let steps = body.find("Next steps").expect("next steps heading");
    let jobs = body.find("By job").expect("by-job heading");
    assert!(steps < jobs, "next steps come before the jobs");
    // The row splits the need's name from the rest of its summary.
    let decide = group(&body, "decide");
    assert!(
        decide
            .contains(r#"<li class="step"><span>Brakes</span><span class="key">Front pads</span>"#),
        "{decide}"
    );
    assert!(decide.contains("who supplies it is not decided") || decide.contains("no part chosen"));
    assert!(!decide.contains("Front pads: "), "{decide}");
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
    assert!(
        body.contains(r#"<span class="l">Vehicle</span>Golf GTI</div>"#),
        "{body}"
    );
    assert!(!body.contains(r#"<span class="l">Shop</span>"#), "{body}");
    assert!(!body.contains(r#"<span class="l">Date</span>"#), "{body}");
    assert!(
        body.contains(
            r#"<span class="l">Status</span><span class="pill pill--plain">Planning</span>"#
        ),
        "an undated appointment reads planning: {body}"
    );
}

#[tokio::test]
async fn an_appointment_reads_in_the_shop_from_its_date() {
    let w = World::new().await;
    let pill = r#"<span class="pill pill--wait">In the shop</span>"#;
    for planned in [common::today(), common::today() - chrono::Days::new(3)] {
        appointment::update_with_status(
            &w.db,
            w.vehicle,
            w.appointment,
            glovebox_shared::inputs::appointment::UpdateAppointment {
                planned_date: Some(Some(planned)),
                ..Default::default()
            },
            None,
            common::today(),
        )
        .await
        .unwrap();

        let (_, page) = w.get(&w.page_path()).await;
        assert!(page.contains(pill), "{planned}: {page}");
        let (_, home) = w.get("/").await;
        assert!(home.contains(pill), "{planned}: {home}");
    }
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
    assert_eq!(body.matches(">Not decided</span>").count(), 2, "{body}");
    assert!(!body.contains("Supplier not decided"), "{body}");

    let (status, _, body) = w
        .post(
            &w.need_path("supplier"),
            &[("supplied_by", "robot"), ("approach", ""), ("back", &back)],
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body.contains("Not saved"), "{body}");
    assert!(body.contains("Must be one of: self, shop"), "{body}");
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
        body.contains("<span class=\"qty\">\u{d7} 2</span>"),
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
            r#"<a href="https://example.com/pads?a=1&#38;b=2" target="_blank" rel="noopener noreferrer">"#
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
    assert_eq!(stored.decided_on, Some(date("2026-01-02")));
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
        // The order form is open in its step row, with the error above the fields.
        let order = group(&body, "order");
        let form = &order[order
            .find(r#"<details class="drawer drawer--end" open>"#)
            .expect("open form")..];
        let error = form
            .find(r#"<p class="error" role="alert">"#)
            .expect("error");
        let fields = form.find(r#"<div class="fields">"#).expect("fields");
        assert!(error < fields, "{form}");
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
async fn a_stale_received_post_keeps_the_date_already_stored() {
    let w = World::new().await;
    w.choose_good().await;
    w.order_it().await;
    let back = w.page_path();
    // The page was loaded before the part was marked received elsewhere.
    need::mark_received(&w.db, w.vehicle, w.need, Some(date("2026-11-13")))
        .await
        .unwrap();

    let (status, _, body) = w.post(&w.need_path("received"), &[("back", &back)]).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body.contains("Not saved"), "{body}");
    let refusal = format!(
        "Need {}&#39;s part was already received on 2026-11-13",
        w.need
    );
    assert!(body.contains(&refusal), "{body}");
    assert!(!body.contains(r#"role="status""#), "{body}");
    let part_id = need::require(&w.db, w.vehicle, w.need)
        .await
        .unwrap()
        .part_id
        .unwrap();
    let stored = part::get(&w.db, w.vehicle, part_id).await.unwrap();
    assert_eq!(stored.received_on, Some(date("2026-11-13")));
}

#[tokio::test]
async fn a_stale_arrival_post_for_an_installed_part_is_refused() {
    let w = World::new().await;
    w.choose_good().await;
    w.order_it().await;
    need::mark_received(&w.db, w.vehicle, w.need, Some(date("2026-11-14")))
        .await
        .unwrap();
    let part_id = need::require(&w.db, w.vehicle, w.need)
        .await
        .unwrap()
        .part_id
        .unwrap();
    // The page was loaded before the part went on the car.
    part::update(
        &w.db,
        w.vehicle,
        part_id,
        UpdatePart {
            fitting: Fitting::Installed(InstallPatch::default(), ReplacePatch::default()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let back = w.page_path();

    for (action, words) in [
        ("received", "can be marked received"),
        ("received/undo", "can have its arrival date cleared"),
    ] {
        let (status, _, body) = w.post(&w.need_path(action), &[("back", &back)]).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{action}");
        assert!(body.contains("Not saved"), "{action}: {body}");
        let refusal =
            format!("Part {part_id} is installed; only an ordered or received part {words}");
        assert!(body.contains(&refusal), "{action}: {body}");
        assert!(!body.contains(r#"role="status""#), "{action}: {body}");
        let stored = part::get(&w.db, w.vehicle, part_id).await.unwrap();
        assert_eq!(stored.received_on, Some(date("2026-11-14")), "{action}");
    }
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
async fn a_replaced_part_reads_installed_and_offers_no_return() {
    let w = World::new().await;
    w.choose_good().await;
    w.order_it().await;
    let part_id = need::require(&w.db, w.vehicle, w.need)
        .await
        .unwrap()
        .part_id
        .unwrap();
    part::update(
        &w.db,
        w.vehicle,
        part_id,
        UpdatePart {
            fitting: Fitting::Replaced(InstallPatch::default(), ReplacePatch::default()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let (_, body) = w.get(&w.page_path()).await;
    assert!(group(&body, "done").contains("Front pads"), "{body}");
    assert!(need_summary(&body, w.need).contains("Installed"), "{body}");
    assert!(!body.contains(&w.need_path("return")), "{body}");
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

    let (status, location, _) = w.post(&todo_path("delete"), &[("back", &back)]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let deleted = format!("{back}?done=deleted-todo-{id}");
    assert_eq!(location.as_deref(), Some(deleted.as_str()));
    let (_, gone) = w.get(&deleted).await;
    assert!(
        !group(&gone, "todos").contains("Bring wheel locks"),
        "{gone}"
    );
    assert!(gone.contains(r#"role="status""#), "{gone}");
    assert!(
        gone.contains("Deleted \u{201c}Bring wheel locks\u{201d}."),
        "{gone}"
    );
    assert!(gone.contains(&todo_path("restore")), "{gone}");

    let (status, location, _) = w.post(&todo_path("restore"), &[("back", &back)]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some(back.as_str()));
    let (_, restored) = w.get(&back).await;
    assert!(
        group(&restored, "todos").contains("Bring wheel locks"),
        "{restored}"
    );
    let open = todo::list(&w.db, w.vehicle, Some(Some(w.appointment)), false)
        .await
        .unwrap();
    assert_eq!(open.iter().map(|t| t.id).collect::<Vec<_>>(), vec![id]);

    // The address of the banner, loaded again after the undo, shows none.
    let (_, stale) = w.get(&deleted).await;
    assert!(!stale.contains(r#"role="status""#), "{stale}");
    // A second undo, as a double click sends, lands where the first did.
    let (status, location, _) = w.post(&todo_path("restore"), &[("back", &back)]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some(back.as_str()));
    assert_eq!(
        todo::list(&w.db, w.vehicle, None, true)
            .await
            .unwrap()
            .len(),
        1
    );

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

// An undecided need of a done job reads the same in its job row as in Next
// steps.
#[tokio::test]
async fn an_open_need_of_a_done_job_says_the_job_is_done_in_its_row() {
    let w = World::new().await;
    test_support::set_job_status(
        &w.db,
        w.vehicle,
        w.job,
        glovebox_shared::entities::work_item::JobStatus::Done,
    )
    .await;

    let (_, body) = w.get(&w.page_path()).await;

    let row = need_summary(&body, w.need);
    assert!(row.contains("the job is done"), "{row}");
    assert!(!row.contains("no part chosen"), "{row}");
    assert!(
        row.contains(r#"class="pill pill--plain">Undecided"#),
        "{row}"
    );
    assert!(!row.contains(r#"class="span empty""#), "{row}");
}

#[tokio::test]
async fn an_open_need_of_a_dropped_job_asks_nothing() {
    let w = World::new().await;
    test_support::set_job_status(
        &w.db,
        w.vehicle,
        w.job,
        glovebox_shared::entities::work_item::JobStatus::Dropped,
    )
    .await;

    let (_, body) = w
        .get(&format!("/vehicles/{}/jobs/{}", w.vehicle, w.job))
        .await;

    let row = need_summary(&body, w.need);
    assert!(row.contains("the job is dropped"), "{row}");
    assert!(
        row.contains(r#"class="pill pill--plain">Undecided"#),
        "{row}"
    );
}

// A shop-supplied need with no researched options still has a form for the
// reason the shop supplies it.
#[tokio::test]
async fn a_shop_need_with_no_options_takes_a_reason() {
    let w = World::new().await;
    let fluid = need::add(
        &w.db,
        w.vehicle,
        w.job,
        NewNeed {
            name: "Brake fluid".into(),
            supplied_by: Some(Supplier::Shop),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let action = format!("/vehicles/{}/needs/{fluid}/decision", w.vehicle);
    let (_, page) = w.get(&w.page_path()).await;
    assert!(page.contains(&format!(r#"action="{action}""#)), "{page}");
    assert!(page.contains("No options researched."), "{page}");

    let back = w.page_path();
    let (status, _, _) = w
        .post(
            &action,
            &[
                ("stored_reason", ""),
                ("reason", "They bleed it with their own"),
                ("back", &back),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let saved = need::require(&w.db, w.vehicle, fluid).await.unwrap();
    assert_eq!(
        saved.decision_reason.as_deref(),
        Some("They bleed it with their own")
    );
}

// A refused order date marks the date input, not the price input.
#[tokio::test]
async fn a_bad_order_date_marks_the_date_input() {
    let w = World::new().await;
    w.choose_good().await;
    let back = w.page_path();
    let (status, _, body) = w
        .post(
            &w.need_path("ordered"),
            &[
                ("seller", "FCP Euro"),
                ("purchase_date", "2026-13-45"),
                ("cost", "74.99"),
                ("back", &back),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body.contains(r#"name="purchase_date" value="2026-13-45" aria-invalid="true""#),
        "{body}"
    );
    assert!(
        !body.contains(r#"name="cost" value="74.99" aria-invalid="true""#),
        "{body}"
    );
}

#[tokio::test]
async fn a_closed_appointment_shows_no_add_form_and_refuses_a_new_todo() {
    let w = World::new().await;
    let back = w.page_path();
    let appointment = w.appointment.to_string();
    w.close_appointment().await;

    let (_, body) = w.get(&back).await;
    assert!(!body.contains(">Add a to-do<"), "{body}");

    let (status, _, body) = w
        .post(
            &format!("/vehicles/{}/todos", w.vehicle),
            &[
                ("title", "Bring wheel locks"),
                ("appointment_id", &appointment),
                ("back", &back),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let expected = format!(
        r#"<div class="page notice"><p class="error" role="alert"><b>Not saved.</b> Appointment {} is closed, so it takes no new work</p><p>The to-do you typed: Bring wheel locks. Add it from the home page.</p></div>"#,
        w.appointment
    );
    assert!(body.contains(&expected), "{body}");
    glovebox_shared::test_support::assert_empty(
        &todo::list(&w.db, w.vehicle, None, true).await.unwrap(),
    );
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
    let (status, location, _) = w.post(&todo_path("delete"), &[("back", "/")]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let deleted = format!("/?done=deleted-todo-{id}");
    assert_eq!(location.as_deref(), Some(deleted.as_str()));
    let (_, gone) = w.get(&deleted).await;
    assert!(
        gone.contains("Deleted \u{201c}Return the HP Plus pads\u{201d}."),
        "{gone}"
    );
    assert!(gone.contains(&todo_path("restore")), "{gone}");
    assert!(!gone.contains(&todo_path("done")), "{gone}");

    let (status, location, _) = w.post(&todo_path("restore"), &[("back", "/")]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/"));
    let (_, home) = w.get("/").await;
    assert!(home.contains(&todo_path("done")), "{home}");
    assert!(!home.contains(r#"role="status""#), "{home}");

    w.post(&todo_path("delete"), &[("back", "/")]).await;
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

async fn add_todo(w: &World, title: &str) -> i32 {
    todo::add(
        &w.db,
        w.vehicle,
        NewTodo {
            title: title.into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id
}

#[tokio::test]
async fn deleting_a_second_todo_replaces_the_first_undo() {
    let w = World::new().await;
    let a = add_todo(&w, "Wash").await;
    let b = add_todo(&w, "Wax").await;
    let path = |id: i32, action: &str| format!("/vehicles/{}/todos/{id}/{action}", w.vehicle);
    w.post(&path(a, "delete"), &[("back", "/")]).await;
    w.post(&path(b, "delete"), &[("back", "/")]).await;

    let (_, first) = w.get(&format!("/?done=deleted-todo-{a}")).await;
    assert!(!first.contains(r#"role="status""#), "{first}");
    let (status, _, body) = w.post(&path(a, "restore"), &[("back", "/")]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body.contains("That delete can no longer be undone."),
        "{body}"
    );

    let (status, _, _) = w.post(&path(b, "restore"), &[("back", "/")]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let left = todo::list(&w.db, w.vehicle, None, true).await.unwrap();
    assert_eq!(left.iter().map(|t| t.id).collect::<Vec<_>>(), vec![b]);
}

#[tokio::test]
async fn a_second_post_of_a_delete_keeps_the_undo() {
    let w = World::new().await;
    let id = add_todo(&w, "Wash").await;
    let delete = format!("/vehicles/{}/todos/{id}/delete", w.vehicle);
    let deleted = format!("/?done=deleted-todo-{id}");
    for _ in 0..2 {
        let (status, location, _) = w.post(&delete, &[("back", "/")]).await;
        assert_eq!(status, StatusCode::SEE_OTHER);
        assert_eq!(location.as_deref(), Some(deleted.as_str()));
    }
    let (_, home) = w.get(&deleted).await;
    assert!(home.contains(r#"role="status""#), "{home}");

    // Once the undo has run, a delete of a to-do that is gone is refused.
    w.post(
        &format!("/vehicles/{}/todos/{id}/restore", w.vehicle),
        &[("back", "/")],
    )
    .await;
    todo::delete(&w.db, w.vehicle, id).await.unwrap();
    let (status, _, _) = w.post(&delete, &[("back", "/")]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_undo_onto_a_closed_appointment_returns_home() {
    let w = World::new().await;
    let back = w.page_path();
    let id = todo::add(
        &w.db,
        w.vehicle,
        NewTodo {
            title: "Bring wheel locks".into(),
            appointment_id: Some(w.appointment),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let path = |action: &str| format!("/vehicles/{}/todos/{id}/{action}", w.vehicle);
    w.post(&path("delete"), &[("back", &back)]).await;
    w.close_appointment().await;

    let (status, location, _) = w.post(&path("restore"), &[("back", &back)]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/"));
    let (_, home) = w.get("/").await;
    assert!(home.contains("Bring wheel locks"), "{home}");
}

#[tokio::test]
async fn un_ticking_a_todo_on_a_cancelled_appointment_returns_home() {
    let w = World::new().await;
    let back = w.page_path();
    let id = todo::add(
        &w.db,
        w.vehicle,
        NewTodo {
            title: "Bring wheel locks".into(),
            appointment_id: Some(w.appointment),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    todo::complete(&w.db, w.vehicle, id, None).await.unwrap();
    appointment::cancel(&w.db, w.vehicle, w.appointment)
        .await
        .unwrap();

    let undo = format!("/vehicles/{}/todos/{id}/done/undo", w.vehicle);
    let (status, location, _) = w.post(&undo, &[("back", &back)]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/"));
    let (_, home) = w.get("/").await;
    assert!(home.contains("Bring wheel locks"), "{home}");
}

// A refused restore keeps the undo, so the next try meets the same refusal
// rather than "can no longer be undone".
#[tokio::test]
async fn a_refused_undo_keeps_its_row() {
    let w = World::new().await;
    let spare = VehicleFixture::new().name("Spare").insert_id(&w.db).await;
    let id = todo::add(
        &w.db,
        spare,
        NewTodo {
            title: "Wash".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let path = |action: &str| format!("/vehicles/{spare}/todos/{id}/{action}");
    w.post(&path("delete"), &[("back", "/")]).await;
    vehicle::delete_unused(&w.db, spare).await.unwrap();

    for _ in 0..2 {
        let (status, _, body) = w.post(&path("restore"), &[("back", "/")]).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert!(!body.contains("can no longer be undone"), "{body}");
    }
}

#[tokio::test]
async fn a_delete_undo_shows_only_on_the_page_that_listed_the_todo() {
    let w = World::new().await;
    let id = todo::add(
        &w.db,
        w.vehicle,
        NewTodo {
            title: "Bring wheel locks".into(),
            appointment_id: Some(w.appointment),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let other = appointment::create(
        &w.db,
        w.vehicle,
        NewAppointment {
            purpose: "Tires".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    w.post(
        &format!("/vehicles/{}/todos/{id}/delete", w.vehicle),
        &[("back", &w.page_path())],
    )
    .await;

    let done = format!("?done=deleted-todo-{id}");
    let (_, listed) = w.get(&format!("{}{done}", w.page_path())).await;
    assert!(listed.contains(r#"role="status""#), "{listed}");
    for page in [
        format!("/vehicles/{}/appointments/{other}{done}", w.vehicle),
        format!("{}{done}", w.job_path()),
        format!("/{done}"),
    ] {
        let (_, body) = w.get(&page).await;
        assert!(!body.contains(r#"role="status""#), "{page}: {body}");
    }

    // A to-do on no appointment is listed on the home page, never a job's.
    let home_todo = add_todo(&w, "Wash").await;
    w.post(
        &format!("/vehicles/{}/todos/{home_todo}/delete", w.vehicle),
        &[("back", "/")],
    )
    .await;
    let done = format!("?done=deleted-todo-{home_todo}");
    let (_, home) = w.get(&format!("/{done}")).await;
    assert!(home.contains(r#"role="status""#), "{home}");
    let (_, job) = w.get(&format!("{}{done}", w.job_path())).await;
    assert!(!job.contains(r#"role="status""#), "{job}");
}

#[tokio::test]
async fn a_delete_undo_belongs_to_its_vehicle() {
    let w = World::new().await;
    let other = VehicleFixture::new().name("Other").insert_id(&w.db).await;
    let id = add_todo(&w, "Wash").await;
    let path = |vid: i32, action: &str| format!("/vehicles/{vid}/todos/{id}/{action}");
    w.post(&path(w.vehicle, "delete"), &[("back", "/")]).await;

    let (status, _, body) = w.post(&path(other, "restore"), &[("back", "/")]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body.contains("That delete can no longer be undone."),
        "{body}"
    );
    glovebox_shared::test_support::assert_empty(
        &todo::list(&w.db, other, None, true).await.unwrap(),
    );

    let (_, home) = w.get(&format!("/?done=deleted-todo-{id}")).await;
    let restore = path(w.vehicle, "restore");
    assert!(home.contains(&format!(r#"action="{restore}""#)), "{home}");
    let (status, _, _) = w.post(&restore, &[("back", "/")]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
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
    let (_, page) = w.get(&back).await;
    assert!(
        page.contains(r#"name="stored_reason" value="Quiet.""#),
        "{page}"
    );
    let (status, _, body) = w
        .post(
            &w.need_path("decision"),
            &[
                ("option_id", ""),
                ("reason", "Quiet."),
                ("stored_reason", "Quiet."),
                ("back", &back),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let (_, after) = w.get(&back).await;
    assert!(group(&after, "decide").contains("Front pads"), "{after}");
    assert!(!after.contains("Quiet."), "{after}");
}

// Undoes the escaping the page applies to text in markup.
fn unescape(html: &str) -> String {
    html.replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

// A browser posts every line break in a form field as CRLF.
fn as_posted(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\n', "\r\n")
}

#[tokio::test]
async fn an_unedited_multi_line_reason_round_trips_as_unedited() {
    let w = World::new().await;
    let back = w.page_path();
    need::decide(
        &w.db,
        w.vehicle,
        w.need,
        Some(w.good),
        Some("Quiet on the highway.\nCheaper than OEM & \"close\".".into()),
    )
    .await
    .unwrap();

    let (_, page) = w.get(&back).await;
    let hidden = r#"name="stored_reason" value=""#;
    let start = page.find(hidden).expect("hidden stored_reason") + hidden.len();
    let stored = unescape(&page[start..start + page[start..].find('"').unwrap()]);
    let open = r#"<textarea name="reason">"#;
    let start = page.find(open).expect("reason box") + open.len();
    let shown = unescape(&page[start..start + page[start..].find("</textarea>").unwrap()]);
    assert_eq!(stored, shown);
    assert!(stored.contains('\n'), "{stored:?}");

    // Choosing no option with the box as rendered clears the owner's choice
    // and its reason; an edited box would be refused instead.
    let (status, _, body) = w
        .post(
            &w.need_path("decision"),
            &[
                ("option_id", ""),
                ("reason", &as_posted(&shown)),
                ("stored_reason", &as_posted(&stored)),
                ("back", &back),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let after = need::require(&w.db, w.vehicle, w.need).await.unwrap();
    assert_eq!(after.chosen_option_id, None);
    assert_eq!(after.decision_reason, None);
}

// A decision post without stored_reason, as from a stale tab, reads its
// reason box as unedited.
#[tokio::test]
async fn a_decision_post_without_the_stored_reason_reads_the_box_as_unedited() {
    let w = World::new().await;
    let back = w.page_path();
    let good = w.good.to_string();
    w.post(
        &w.need_path("decision"),
        &[("option_id", &good), ("reason", "Quiet."), ("back", &back)],
    )
    .await;

    let (status, _, body) = w
        .post(
            &w.need_path("decision"),
            &[("option_id", ""), ("reason", "Quiet."), ("back", &back)],
        )
        .await;

    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let stored = need::require(&w.db, w.vehicle, w.need).await.unwrap();
    assert_eq!(stored.chosen_option_id, None);
    assert_eq!(stored.decision_reason, None);
}

#[tokio::test]
async fn a_reason_typed_with_no_choice_on_an_owner_need_is_refused() {
    let w = World::new().await;
    let back = w.page_path();
    let good = w.good.to_string();
    w.post(
        &w.need_path("decision"),
        &[("option_id", &good), ("reason", "Quiet."), ("back", &back)],
    )
    .await;

    let (status, _, body) = w
        .post(
            &w.need_path("decision"),
            &[
                ("option_id", ""),
                ("reason", "Not sure yet."),
                ("stored_reason", "Quiet."),
                ("back", &back),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body.contains("Reason: needs a chosen option unless the shop supplies the need"),
        "{body}"
    );
    assert!(
        body.contains(r#"<textarea name="reason" aria-invalid="true">"#),
        "{body}"
    );
    let stored = need::require(&w.db, w.vehicle, w.need).await.unwrap();
    assert_eq!(stored.chosen_option_id, Some(w.good));
    assert_eq!(stored.decision_reason.as_deref(), Some("Quiet."));
}

impl World {
    async fn supply_from_the_shop(&self, reason: Option<&str>) {
        need::update(
            &self.db,
            self.vehicle,
            self.need,
            UpdateNeed {
                supplied_by: Some(Some(Supplier::Shop)),
                decision_reason: reason.map(|r| Some(r.into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn saving_no_choice_on_a_shop_need_keeps_its_reason_until_it_is_emptied() {
    let w = World::new().await;
    let back = w.page_path();
    w.supply_from_the_shop(Some("The shop uses OEM pads."))
        .await;

    let (status, _, body) = w
        .post(
            &w.need_path("decision"),
            &[
                ("option_id", ""),
                ("reason", "The shop uses OEM pads."),
                ("stored_reason", "The shop uses OEM pads."),
                ("back", &back),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let (_, after) = w.get(&back).await;
    assert!(
        need_summary(&after, w.need).contains("The shop uses OEM pads."),
        "{after}"
    );

    w.post(
        &w.need_path("decision"),
        &[
            ("option_id", ""),
            ("reason", ""),
            ("stored_reason", "The shop uses OEM pads."),
            ("back", &back),
        ],
    )
    .await;
    let (_, after) = w.get(&back).await;
    assert!(!after.contains("The shop uses OEM pads."), "{after}");
}

#[tokio::test]
async fn clearing_a_shop_needs_choice_drops_that_choices_reason() {
    let w = World::new().await;
    let back = w.page_path();
    w.supply_from_the_shop(None).await;
    let good = w.good.to_string();
    w.post(
        &w.need_path("decision"),
        &[
            ("option_id", &good),
            ("reason", "Cheaper than OEM."),
            ("back", &back),
        ],
    )
    .await;

    let (status, _, body) = w
        .post(
            &w.need_path("decision"),
            &[
                ("option_id", ""),
                ("reason", "Cheaper than OEM."),
                ("stored_reason", "Cheaper than OEM."),
                ("back", &back),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let (_, after) = w.get(&back).await;
    assert!(!after.contains("Cheaper than OEM."), "{after}");
}

#[tokio::test]
async fn a_new_reason_with_no_choice_on_a_shop_need_replaces_the_choices() {
    let w = World::new().await;
    let back = w.page_path();
    w.supply_from_the_shop(None).await;
    let good = w.good.to_string();
    w.post(
        &w.need_path("decision"),
        &[
            ("option_id", &good),
            ("reason", "Cheaper than OEM."),
            ("back", &back),
        ],
    )
    .await;

    let (status, _, body) = w
        .post(
            &w.need_path("decision"),
            &[
                ("option_id", ""),
                ("reason", "The shop stocks OEM pads."),
                ("stored_reason", "Cheaper than OEM."),
                ("back", &back),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let stored = need::require(&w.db, w.vehicle, w.need).await.unwrap();
    assert_eq!(stored.chosen_option_id, None);
    assert_eq!(
        stored.decision_reason.as_deref(),
        Some("The shop stocks OEM pads.")
    );
}

#[tokio::test]
async fn moving_a_shop_need_to_the_owner_clears_its_reason() {
    let w = World::new().await;
    let back = w.page_path();
    w.supply_from_the_shop(Some("The shop uses OEM pads."))
        .await;

    let (status, _, body) = w
        .post(
            &w.need_path("supplier"),
            &[("supplied_by", "self"), ("approach", ""), ("back", &back)],
        )
        .await;

    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let (_, after) = w.get(&back).await;
    assert!(!after.contains("The shop uses OEM pads."), "{after}");
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
            supplied_by: Some(Supplier::Owner),
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
        order.contains(r#"<span class="key">Oil: 5W-30</span>"#),
        "{order}"
    );
    assert!(order.contains("order Any oil"), "{order}");
    let check = group(&body, "check");
    assert!(
        check.contains(r#"<span class="key">Oil: 5W-30</span>"#),
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
            fitment: Some(Fitment::Unconfirmed),
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

#[tokio::test]
async fn fit_labels_leave_the_word_fit_to_the_column_header() {
    let w = World::new().await;
    w.choose_good().await;

    let (_, body) = w.get(&w.page_path()).await;

    assert!(body.contains(">Confirmed</span>"), "{body}");
    assert!(!body.contains("Fit confirmed"), "{body}");
}

#[tokio::test]
async fn a_need_ordered_without_an_option_shows_its_step_sentence() {
    let w = World::new().await;
    let need_id = need::add(
        &w.db,
        w.vehicle,
        w.job,
        NewNeed {
            name: "Brake fluid".into(),
            supplied_by: Some(Supplier::Owner),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let part_id = part::create(
        &w.db,
        w.vehicle,
        NewPart {
            name: "Brake fluid".into(),
            seller: Some("FCP Euro".into()),
            purchase_date: Some(date("2026-11-13")),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    need::link_part(&w.db, w.vehicle, need_id, part_id, None)
        .await
        .unwrap();

    let (_, body) = w.get(&w.job_path()).await;

    let row = need_summary(&body, need_id);
    assert!(row.contains("Bought from FCP Euro on 2026-11-13"), "{row}");
    assert!(!row.contains("No part chosen"), "{row}");
}

#[tokio::test]
async fn a_linked_part_with_no_chosen_option_fills_the_part_columns() {
    let w = World::new().await;
    let need_id = need::add(
        &w.db,
        w.vehicle,
        w.job,
        NewNeed {
            name: "Front rotors".into(),
            quantity: Some(2),
            supplied_by: Some(Supplier::Owner),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let part_id = part::create(
        &w.db,
        w.vehicle,
        NewPart {
            name: "Coat Z front rotors 340x30".into(),
            manufacturer: Some("Zimmermann".into()),
            part_number: Some("1K0615301AD".into()),
            seller: Some("FCP Euro".into()),
            cost_cents: Some(19_198),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    need::link_part(&w.db, w.vehicle, need_id, part_id, None)
        .await
        .unwrap();

    let (_, body) = w.get(&w.job_path()).await;

    let row = need_summary(&body, need_id);
    for cell in [
        r#"<span data-l="Brand">Zimmermann</span>"#,
        r#"<span class="pn" data-l="Part number">1K0615301AD</span>"#,
        r#"<span data-l="Description">Coat Z front rotors 340x30</span>"#,
        r#"<span data-l="Seller">FCP Euro</span>"#,
        r#"<span class="num price" data-l="Price">$191.98</span>"#,
    ] {
        assert!(row.contains(cell), "{cell}: {row}");
    }
}

// A kit's items show under the row whether the linked part fills the part
// columns itself or a chosen option does.
#[tokio::test]
async fn a_need_row_lists_its_kits_items_on_the_job_and_appointment_pages() {
    let w = World::new().await;
    need::decide(&w.db, w.vehicle, w.need, Some(w.good), None)
        .await
        .unwrap();
    let kit = need::mark_ordered(&w.db, w.vehicle, w.need, OrderDetails::default())
        .await
        .unwrap()
        .part
        .part
        .id;
    part::update(
        &w.db,
        w.vehicle,
        kit,
        UpdatePart {
            add_contents: vec![
                NewPartContent {
                    name: "Pads".into(),
                    manufacturer: Some("Akebono".into()),
                    part_number: Some("ACT1633A".into()),
                    quantity: Some(4),
                },
                NewPartContent {
                    name: "Shims".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    for path in [w.job_path(), w.page_path()] {
        let (_, body) = w.get(&path).await;
        let row = need_summary(&body, w.need);
        assert!(
            row.contains(
                r#"<span class="line kit"><span class="l">In the kit</span><span class="kit-item">Pads × 4<span class="pn">Akebono ACT1633A</span></span><span class="kit-item">Shims</span></span>"#
            ),
            "{path}: {row}"
        );
    }
}

#[tokio::test]
async fn a_need_row_with_no_kit_lists_no_items() {
    let w = World::new().await;

    let (_, body) = w.get(&w.job_path()).await;

    assert!(!need_summary(&body, w.need).contains("In the kit"));
}

#[tokio::test]
async fn a_linked_part_with_no_decision_shows_a_pill_and_no_check_row() {
    let w = World::new().await;
    let need_id = need::add(
        &w.db,
        w.vehicle,
        w.job,
        NewNeed {
            name: "Front rotors".into(),
            supplied_by: Some(Supplier::Owner),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let part_id = part::create(
        &w.db,
        w.vehicle,
        NewPart {
            name: "Coat Z front rotors".into(),
            seller: Some("FCP Euro".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    need::link_part(&w.db, w.vehicle, need_id, part_id, None)
        .await
        .unwrap();

    let (_, body) = w.get(&w.page_path()).await;

    let row = need_summary(&body, need_id);
    assert!(
        row.contains(r#"<span class="pill">No decision recorded</span>"#),
        "{row}"
    );
    assert!(!body.contains("A part is linked"), "{body}");
    assert!(!body.contains(r#"id="g-check""#), "{body}");
}

#[tokio::test]
async fn a_linked_part_with_no_number_shows_its_name_as_the_part() {
    let w = World::new().await;
    let need_id = need::add(
        &w.db,
        w.vehicle,
        w.job,
        NewNeed {
            name: "Brake fluid".into(),
            supplied_by: Some(Supplier::Owner),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let part_id = part::create(
        &w.db,
        w.vehicle,
        NewPart {
            name: "ESI6-32N brake fluid".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    need::link_part(&w.db, w.vehicle, need_id, part_id, None)
        .await
        .unwrap();

    let (_, body) = w.get(&w.job_path()).await;

    let row = need_summary(&body, need_id);
    assert!(
        row.contains(r#"<span class="pn span2" data-l="Part">ESI6-32N brake fluid</span>"#),
        "{row}"
    );
}

#[tokio::test]
async fn a_job_shows_one_header_line_and_each_part_in_its_own_columns() {
    let w = World::new().await;
    need::decide(
        &w.db,
        w.vehicle,
        w.need,
        Some(w.good),
        Some("Quiet and low dust.".into()),
    )
    .await
    .unwrap();

    let (_, body) = w.get(&w.page_path()).await;

    assert!(
        body.contains(
            "<span>Need</span><span>Stage</span><span>Brand</span><span>Part \
             number</span><span>Description</span><span>Seller</span><span \
             class=\"num\">Price</span><span>Fit</span>"
        ),
        "{body}"
    );
    assert!(
        body.contains(r#"<span data-l="Brand">Akebono</span>"#),
        "{body}"
    );
    assert!(
        body.contains(r#"<span class="pn" data-l="Part number">ACT1633</span>"#),
        "{body}"
    );
    assert!(
        body.contains(r#"<span class="num price" data-l="Price">$74.99</span>"#),
        "{body}"
    );
    assert!(
        body.contains(r#"<span class="l">Reason</span>Quiet and low dust."#),
        "{body}"
    );
}

#[tokio::test]
async fn an_option_without_a_part_number_spans_the_number_and_description_columns() {
    let w = World::new().await;
    need::add_option(
        &w.db,
        w.vehicle,
        w.need,
        NewOption {
            description: "Pollen cabin air filter".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let (_, body) = w.get(&w.page_path()).await;

    assert!(
        body.contains(r#"<span class="pn span2" data-l="Part">Pollen cabin air filter</span>"#),
        "{body}"
    );
    assert!(
        !body.contains(r#"data-l="Description">Pollen cabin air filter"#),
        "{body}"
    );
}

#[tokio::test]
async fn a_chosen_option_without_a_seller_or_price_keeps_its_columns() {
    let w = World::new().await;
    let bare = need::add_option(
        &w.db,
        w.vehicle,
        w.need,
        NewOption {
            description: "Pads".into(),
            part_number: Some("BARE-1".into()),
            fitment: Some(Fitment::Confirmed),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    need::decide(&w.db, w.vehicle, w.need, Some(bare), None)
        .await
        .unwrap();

    let (_, body) = w.get(&w.page_path()).await;

    let row = need_summary(&body, w.need);
    assert!(row.contains(r#"<span data-l="Brand"></span>"#), "{row}");
    assert!(row.contains(r#"<span data-l="Seller"></span>"#), "{row}");
    assert!(
        row.contains(r#"<span class="num price" data-l="Price"></span>"#),
        "{row}"
    );
}

#[tokio::test]
async fn an_action_keeps_one_name() {
    let w = World::new().await;
    w.choose_good().await;
    w.order_it().await;

    let (_, body) = w.get(&w.page_path()).await;

    // The need shows the return form in its step row and in its job row,
    // each with an opener and a submit button.
    assert_eq!(body.matches(">Return it<").count(), 4, "{body}");
    assert_eq!(body.matches(">Add a to-do<").count(), 2, "{body}");
    assert!(body.contains("Why are you returning it?"), "{body}");
    for gone in [
        "Wrong part, returning it",
        "Record the return",
        ">Add to-do<",
        "What was wrong",
    ] {
        assert!(!body.contains(gone), "{gone}: {body}");
    }
}

#[tokio::test]
async fn a_done_step_row_keeps_its_stage_pill() {
    let w = World::new().await;
    w.choose_good().await;
    w.order_it().await;
    need::mark_received(&w.db, w.vehicle, w.need, Some(date("2026-11-14")))
        .await
        .unwrap();

    let (_, body) = w.get(&w.page_path()).await;

    let done = group(&body, "done");
    assert!(
        done.contains(r#"<span class="pill pill--ok">Received</span>"#),
        "{done}"
    );
    assert!(done.contains(">Return it<"), "{done}");
}

// An open form in a step row is a block under the sentence, so the drawer
// must not take the flex layout of the action cell.
#[tokio::test]
async fn the_order_drawer_in_a_step_row_is_not_an_action_cell() {
    let w = World::new().await;
    w.choose_good().await;

    let (_, body) = w.get(&w.page_path()).await;

    let order = group(&body, "order");
    assert!(
        order.contains(r#"<details class="drawer drawer--end">"#),
        "{order}"
    );
    assert!(!order.contains(r#"class="drawer act""#), "{order}");
}

#[tokio::test]
async fn the_option_radios_are_one_named_group() {
    let w = World::new().await;

    let (_, body) = w.get(&w.page_path()).await;

    assert!(
        body.contains(r#"<div role="radiogroup" aria-label="Options for Front pads">"#),
        "{body}"
    );
    assert!(
        body.contains(r#"<span class="l" aria-hidden="true">Options</span>"#),
        "{body}"
    );
}

impl World {
    fn sheet_path(&self) -> String {
        format!("{}/print", self.page_path())
    }

    // Posts what the sheet's "Ask the shop" form sends.
    async fn ask_the_shop(&self) -> (StatusCode, Option<String>, String) {
        self.ask_from_the_sheet("shop").await
    }

    async fn ask_from_the_sheet(&self, supplied_by: &str) -> (StatusCode, Option<String>, String) {
        self.post(
            &self.need_path("supplier"),
            &[("supplied_by", supplied_by), ("back", &self.sheet_path())],
        )
        .await
    }
}

// The sheet's "Please supply" list, or the empty string when it has none.
fn please_supply(body: &str) -> &str {
    body.split_once("<h2>Please supply</h2>")
        .map_or("", |(_, rest)| rest.split("</ul>").next().unwrap_or(""))
}

#[tokio::test]
async fn ask_the_shop_from_the_sheet_settles_an_undecided_need() {
    let w = World::new().await;
    let (_, before) = w.get(&w.sheet_path()).await;
    assert!(before.contains("Front pads (Brakes)"), "{before}");

    let (status, location, _) = w.ask_the_shop().await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some(w.sheet_path().as_str()));
    let (_, body) = w.get(&w.sheet_path()).await;
    assert!(body.contains("Everything is settled."), "{body}");
    assert!(please_supply(&body).contains("Front pads"), "{body}");
}

#[tokio::test]
async fn ask_the_shop_from_the_sheet_settles_a_need_to_order() {
    let w = World::new().await;
    w.choose_good().await;

    w.ask_the_shop().await;

    let (_, body) = w.get(&w.sheet_path()).await;
    assert!(body.contains("Everything is settled."), "{body}");
    assert!(please_supply(&body).contains("Front pads"), "{body}");
}

#[tokio::test]
async fn ask_the_shop_failure_shows_the_sheet_again() {
    let w = World::new().await;
    need::delete(&w.db, w.vehicle, w.need).await.unwrap();

    let (status, _, body) = w.ask_the_shop().await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("Everything is settled."), "{body}");
    assert!(body.contains("Not saved."), "{body}");
    assert!(body.contains("not found"), "{body}");
}

#[tokio::test]
async fn ask_the_shop_leaves_a_newer_approach_alone() {
    let w = World::new().await;
    let (_, sheet) = w.get(&w.sheet_path()).await;
    assert!(!sheet.contains(r#"name="approach""#), "{sheet}");
    // The approach changes after the sheet was loaded.
    need::update(
        &w.db,
        w.vehicle,
        w.need,
        UpdateNeed {
            approach: Some(Some(Approach::Upgrade)),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    w.ask_the_shop().await;

    let need = need::require(&w.db, w.vehicle, w.need).await.unwrap();
    assert_eq!(need.approach, Some(Approach::Upgrade));
    assert_eq!(need.supplied_by, Some(Supplier::Shop));
}

#[tokio::test]
async fn ask_the_shop_refusal_shows_beside_its_row() {
    let w = World::new().await;

    let (status, _, body) = w.ask_from_the_sheet("bogus").await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body.matches("Not saved.").count(), 1, "{body}");
    let row = body
        .split_once(&format!(r#"<li id="need-{}">"#, w.need))
        .and_then(|(_, rest)| rest.split_once("</li>"))
        .map_or("", |(row, _)| row);
    assert!(row.contains("Not saved."), "{body}");
    assert!(row.contains("Supplier:"), "{body}");
}
