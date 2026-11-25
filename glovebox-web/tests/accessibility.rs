mod common;

use std::collections::BTreeMap;

use axum::http::StatusCode;
use common::{app, db, get, page};

// Every page the demo data reaches by following links from the home page,
// keyed by its path.
async fn demo_pages() -> BTreeMap<String, String> {
    let db = db().await;
    glovebox_shared::seed::demo(&db).await.unwrap();
    let mut pages = BTreeMap::new();
    let mut queue = vec!["/".to_owned()];
    while let Some(path) = queue.pop() {
        if pages.contains_key(&path) {
            continue;
        }
        let (status, body) = page(app(db.clone(), "test"), get(&path)).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        for link in body.split("href=\"").skip(1) {
            let target = link.split('"').next().unwrap_or_default();
            let target = target.split(['?', '#']).next().unwrap_or_default();
            if target.starts_with('/') && !target.starts_with("/assets/") {
                queue.push(target.to_owned());
            }
        }
        pages.insert(path, body);
    }
    pages
}

// The text of each opening tag named `tag` in `html`.
fn tags<'a>(html: &'a str, tag: &str) -> Vec<&'a str> {
    html.split(&format!("<{tag}"))
        .skip(1)
        .filter(|rest| rest.starts_with([' ', '>']))
        .map(|rest| rest.split('>').next().unwrap_or_default())
        .collect()
}

#[tokio::test]
async fn the_demo_reaches_every_kind_of_page() {
    let pages = demo_pages().await;
    let kinds: Vec<&str> = [
        "/",
        "/shops",
        "/shops/1",
        "/vehicles/1",
        "/vehicles/1/history",
    ]
    .into_iter()
    .filter(|path| !pages.contains_key(*path))
    .collect();
    glovebox_shared::test_support::assert_empty(&kinds);
    for part in ["/appointments/", "/jobs/", "/services/", "/print"] {
        assert!(
            pages.keys().any(|path| path.contains(part)),
            "no page under {part}: {:?}",
            pages.keys()
        );
    }
}

// A cell that shows its label only on a narrow window carries the same label
// in a visually hidden span, so a screen reader hears it at every width. A
// cell with no value stays empty, and the narrow layout hides it.
#[tokio::test]
async fn every_cell_with_a_value_carries_a_spoken_label() {
    let pages = demo_pages().await;
    let mut cells = 0;
    for (path, body) in &pages {
        for cell in body.split("data-l=\"").skip(1) {
            let (label, rest) = cell.split_once('"').unwrap();
            let content = rest.split_once('>').unwrap().1;
            if content.starts_with("</span>") {
                continue;
            }
            let spoken = format!(r#"<span class="vh">{label} </span>"#);
            assert!(
                content.starts_with(&spoken),
                "{path}: the {label} cell has no spoken label: {}",
                &content[..content.len().min(120)]
            );
            cells += 1;
        }
    }
    assert!(cells > 100, "only {cells} cells were checked");
}

// The label a narrow window shows is generated content with empty
// alternative text, so the hidden span is the only one read.
#[tokio::test]
async fn the_visible_narrow_label_is_silent() {
    let (_, body) = page(app(db().await, "test"), get("/")).await;
    assert!(
        body.contains(
            r#".row > [data-l]::before { content: attr(data-l); content: attr(data-l) / "";"#
        ),
        "{body}"
    );
}

// A need's row is one disclosure. Its name is the need's name and stage, and
// the whole row is its description, so the values stay available.
#[tokio::test]
async fn a_need_row_is_named_by_its_name_and_stage() {
    let pages = demo_pages().await;
    let mut rows = 0;
    for (path, body) in pages.iter().filter(|(path, _)| path.contains("/jobs/")) {
        for summary in body.split("<summary class=\"row\"").skip(1) {
            let (attributes, content) = summary.split_once('>').unwrap();
            let content = content.split("</summary>").next().unwrap();
            let id = attributes
                .split("id=\"need-")
                .nth(1)
                .and_then(|rest| rest.split("-row\"").next())
                .unwrap_or_else(|| panic!("{path}: a summary with no id: {attributes}"));
            assert_eq!(
                attributes.trim(),
                format!(
                    r#"id="need-{id}-row" aria-labelledby="need-{id}-name need-{id}-stage" aria-describedby="need-{id}-row""#
                ),
                "{path}"
            );
            for part in [
                format!(r#"<span class="key" id="need-{id}-name">"#),
                format!(
                    r#"<span data-l="Stage" id="need-{id}-stage"><span class="vh">Stage </span><span class="pill"#
                ),
            ] {
                assert!(content.contains(&part), "{path}: missing {part}: {content}");
            }
            rows += 1;
        }
    }
    assert!(rows >= 8, "only {rows} need rows were checked");
}

#[tokio::test]
async fn a_need_rows_name_holds_its_name_and_its_stage() {
    let pages = demo_pages().await;
    let body = pages
        .values()
        .find(|body| body.contains(">Rear rotors "))
        .expect("the page of the brakes job");
    let named = body
        .split("-name\">Rear rotors ")
        .nth(1)
        .expect("the rear rotors row");
    let row = named.split("</summary>").next().unwrap();
    assert!(
        row.starts_with(r#"<span class="qty">× 2</span></span>"#),
        "{row}"
    );
    assert!(
        row.contains(
            r#"<span class="vh">Stage </span><span class="pill pill--todo">To order</span>"#
        ),
        "{row}"
    );
}

// Every page has one top heading, and no heading after it skips a level. The
// work sheet's on-screen box of unsettled needs precedes the sheet, so its
// heading comes before the sheet's.
#[tokio::test]
async fn every_page_has_one_h1_and_no_skipped_heading_level() {
    let pages = demo_pages().await;
    for (path, body) in &pages {
        let levels: Vec<u32> = body
            .split("<h")
            .skip(1)
            .filter_map(|rest| rest.chars().next()?.to_digit(10))
            .collect();
        assert_eq!(
            levels.iter().filter(|level| **level == 1).count(),
            1,
            "{path}: {levels:?}"
        );
        if path.ends_with("/print") {
            continue;
        }
        assert_eq!(levels[0], 1, "{path}: {levels:?}");
        for pair in levels.windows(2) {
            assert!(pair[1] <= pair[0] + 1, "{path}: {levels:?}");
        }
    }
}

// A field's name comes from the label that wraps it, and a button with no
// text carries an `aria-label`.
#[tokio::test]
async fn every_form_control_has_a_label() {
    let pages = demo_pages().await;
    let mut fields = 0;
    for (path, body) in &pages {
        for control in ["input", "select", "textarea"] {
            let mut at = 0;
            while let Some(found) = body[at..].find(&format!("<{control} ")) {
                let start = at + found;
                at = start + 1;
                let tag = body[start..].split('>').next().unwrap();
                if tag.contains(r#"type="hidden""#) || tag.contains("aria-label=") {
                    continue;
                }
                let before = &body[..start];
                let opened = before.rfind("<label");
                let closed = before.rfind("</label>");
                assert!(
                    opened.is_some() && opened > closed,
                    "{path}: a field outside a label: {tag}"
                );
                fields += 1;
            }
        }
        for button in body.split("<button").skip(1) {
            let (tag, rest) = button.split_once('>').unwrap();
            let text = rest.split("</button>").next().unwrap();
            assert!(
                !text.trim().is_empty() || tag.contains("aria-label=\""),
                "{path}: a button with no name: {tag}"
            );
        }
    }
    assert!(fields > 20, "only {fields} fields were checked");
}

// The to-do tick is a 24 by 24 pixel target, and the focus ring uses the
// token that stands out on both themes.
#[tokio::test]
async fn the_tick_is_a_24_pixel_target_and_focus_has_its_own_token() {
    let pages = demo_pages().await;
    let body = &pages["/"];
    for rule in [
        ".tick { display: block; position: relative; width: 1.5rem; height: 1.5rem;",
        ":focus-visible { outline: 3px solid var(--focus); outline-offset: 2px; }",
        "--focus: var(--ink);",
        "--focus: var(--accent);",
    ] {
        assert!(body.contains(rule), "missing {rule}");
    }
    for absent in ["outline: none", "outline: 0"] {
        assert!(!body.contains(absent), "{absent}");
    }
    let ticks: Vec<&str> = tags(body, "button")
        .into_iter()
        .filter(|tag| tag.contains(r#"class="tick""#))
        .collect();
    assert!(!ticks.is_empty(), "the home page shows no tick");
    for tick in ticks {
        assert!(tick.contains("aria-label=\"Tick: "), "{tick}");
    }
}
