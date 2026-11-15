# Web Design Pass (Ledger Look) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port the approved ledger mockup into `glovebox-web` so the home, appointment and job pages
show labeled columns, one text style, state pills with marks, and a bundled typeface.

**Architecture:** The look stays in the one inline stylesheet in `templates/base.html`, rewritten
from the mockup's `<style>` block. The need, option and step templates are restructured into CSS
grid rows that share a column template. `view.rs` changes labels and gains one field, and the
server gains two font routes and one policy allowance. Routes, forms and shared logic do not change.

**Tech Stack:** Rust, axum, askama, plain CSS. No script.

**Spec:** `docs/superpowers/specs/2026-11-14-web-design-pass-design.md`. The reference mockup is
`docs/design/design-pass/d.html`; its `<style>` block is the source for every CSS rule and its
markup is the source for every class name. Read both before starting.

## Global Constraints

- No script, no external resource, no image file. Pill marks and the tick are CSS borders and
  gradients.
- The content security policy is exactly this one line:
  `default-src 'none'; style-src 'unsafe-inline'; font-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'`
- `glovebox-web` never names SeaORM (`just check-layering`).
- Every count and sentence about the records comes from a shared function. `view.rs` picks labels.
- Text sizes are 0.8125rem (label), 1rem (data), 1.1875rem (heading) and 1.75rem (title), in
  weights 400 and 600 only. Gray text is a label or a missing value, never a stored value.
- Only text fields, selects and textareas have a boxed outline.
- Color tokens, light and dark, are copied verbatim from the spec's Color table.
- Class names are the mockup's: `sheet`, `sheet-top`, `sheet-head`, `cols`, `row`, `key`, `memo`,
  `l`, `pill`, `step`, `act`, `formbox`, `work`, `links`, `toggle`, `opt`, `pick`, `drawer`, `btn`,
  `tick`, `field`, `fields`, `form-actions`, `form-note`, `error`, `empty`, `num`, `span`.
- Comments follow the project rules: full sentences, no history, no `//` block over 10 lines.
- Never run anything against `data/glovebox.db`. Port 4828 may be in use; serve the seeded `test`
  environment with `GLOVEBOX_ENV=test GLOVEBOX_WEB_PORT=4829 cargo run -q --release -p glovebox-web`.
- Run `just fmt` before each commit. Do not stage `.beads/issues.jsonl`.

## Review Focus

1. An option with no part number (`OptionCard.title` holds the description and `description` is
   `None`): the text spans the Part number and Description columns and nothing is duplicated.
2. A need settled by a linked part with no chosen option: the row shows the step sentence
   ("Ordered from …", "Received on …"), never "No part chosen".
3. A chosen option with no manufacturer, seller or price: the cell is empty and the later columns
   stay in place.
4. A long unbroken part number or description: it wraps inside its column and does not push the
   Price and Fit columns.
5. A failed form post re-renders with the right need open and the right form open inside the new
   markup, with the error line above the fields.

---

### Task 1: Serve the typeface and allow it in the policy

**Files:**
- Create: `glovebox-web/assets/IBMPlexSans-Regular.woff2`, `glovebox-web/assets/IBMPlexSans-SemiBold.woff2`, `glovebox-web/assets/LICENSE-IBM-Plex.txt`
- Create: `glovebox-web/src/assets.rs`
- Modify: `glovebox-web/src/lib.rs` (`router`), `glovebox-web/src/guard.rs` (`POLICY` and its comment)
- Test: `glovebox-web/tests/pages.rs`, `glovebox-web/tests/common/mod.rs`

**Interfaces:**
- Produces: `GET /assets/plex-sans-400.woff2` and `GET /assets/plex-sans-600.woff2`, each `200`
  with `content-type: font/woff2` and `cache-control: public, max-age=604800`.

- [ ] **Step 1: Fetch the unmodified files.** Take `IBMPlexSans-Regular.woff2`,
  `IBMPlexSans-SemiBold.woff2` and `LICENSE.txt` from IBM's release (the `@ibm/plex-sans` npm
  package, expected under `fonts/complete/woff2/`, or the github.com/IBM/plex release archive).
  Download into a scratch directory first, confirm each font starts with the bytes `wOF2`, and
  confirm the license is the SIL Open Font License 1.1 naming IBM. Copy them to
  `glovebox-web/assets/` under the names above.

- [ ] **Step 2: Write the failing tests** in `tests/pages.rs`. Add `send_bytes(app, req) ->
  (Response<Body>, Vec<u8>)` to `tests/common/mod.rs` beside `send`.

```rust
#[tokio::test]
async fn font_files_are_served_as_woff2() {
    for path in ["/assets/plex-sans-400.woff2", "/assets/plex-sans-600.woff2"] {
        let (res, body) = send_bytes(app(db().await, "live"), get(path)).await;
        assert_eq!(res.status(), StatusCode::OK, "{path}");
        assert_eq!(res.headers()["content-type"], "font/woff2", "{path}");
        assert_eq!(res.headers()["cache-control"], "public, max-age=604800", "{path}");
        assert!(body.starts_with(b"wOF2"), "{path}");
    }
}
```

Change the `CSP` constant in `tests/pages.rs` to the policy in Global Constraints.

- [ ] **Step 3: Run** `cargo test -p glovebox-web --test pages`. Expected: `font_files_are_served_as_woff2`
  fails with 404 and `every_response_carries_the_csp` fails on the policy string.

- [ ] **Step 4: Implement.** `src/assets.rs` holds `pub async fn regular() -> impl IntoResponse` and
  `pub async fn semibold() -> impl IntoResponse`, each returning the headers above and the bytes
  from `include_bytes!("../assets/…")`. Register both routes in `router` before the fallback. Set
  `POLICY` to the Global Constraints value and rewrite the comment above it to say that the page
  loads its typeface from this server.

- [ ] **Step 5: Run** `cargo test -p glovebox-web`. Expected: all pass.

- [ ] **Step 6: Commit** `feat(web): serve IBM Plex Sans and allow it in the policy`.

---

### Task 2: Labels and the step sentence in `view.rs`

**Files:**
- Modify: `glovebox-shared/src/services/checklist.rs` (`step_detail` visibility and doc)
- Modify: `glovebox-web/src/view.rs` (`OptionCard::new`, `NeedCard`, `supplier_text`, `approach_text`, `need_card`)
- Test: `glovebox-web/tests/actions.rs`, and the unit tests in `view.rs` if any assert these labels

**Interfaces:**
- Produces: `checklist::step_detail(view: &NeedView) -> String` is `pub`, with a `///` line stating
  what it returns. Its body does not change.
- Produces: `OptionCard.fit_text` is `"Confirmed"`, `"Does not fit"` or `"Unconfirmed"`.
- Produces: `NeedCard::supplier_text()` and `approach_text()` return `"Not decided"` when unset.
- Produces: `NeedCard.quantity` is `"× 2"` (`\u{d7}`, a space, the number) when more than one.
- Produces: `NeedCard.status_line: String`, the value of `checklist::step_detail` for the need,
  with a `///` line saying it is the sentence the need's Next steps row shows.

- [ ] **Step 1: Write the failing tests.** In `tests/actions.rs` update the assertions at the lines
  that check `"Supplier not decided"`, `"Approach not decided"` and `2 needed` to check that the
  body contains `Not decided` and `× 2`. Add:

```rust
#[tokio::test]
async fn a_need_ordered_without_an_option_shows_its_step_sentence() {
    // Seed a need whose part was linked with no chosen option, in stage `ordered`.
    // Use the same service calls the existing ordering tests use to reach that state.
    let (status, body) = get_page(&db, &appointment_path).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("ordered from"), "{body}");
    assert!(!body.contains("No part chosen"), "{body}");
}
```

Add a unit test in `view.rs` that a `NeedCard` for a shop-supplied need has
`status_line == "the shop supplies it"`, and that `OptionCard::new` gives `fit_text == "Confirmed"`
for fitment `confirmed`.

- [ ] **Step 2: Run** `cargo test -p glovebox-web`. Expected: the new and updated tests fail.

- [ ] **Step 3: Implement** the Interfaces block. The template still prints "No part chosen" until
  Task 4, so `a_need_ordered_without_an_option_shows_its_step_sentence` stays failing; mark it
  `#[ignore = "passes once the need row prints status_line"]` and remove the attribute in Task 4.

- [ ] **Step 4: Run** `cargo test --workspace`. Expected: all pass, one ignored.

- [ ] **Step 5: Commit** `feat(web): ledger labels and a step sentence on every need card`.

---

### Task 3: The stylesheet, the header and the pills

**Files:**
- Modify: `glovebox-web/templates/base.html` (the whole `<style>` block and the `<header>`),
  `glovebox-web/templates/banner.html`
- Test: `glovebox-web/tests/pages.rs`

**Interfaces:**
- Produces: every class in Global Constraints, styled as in the mockup, plus these pill classes.
  The existing class values (`stage_class`, `fit_class`, `status_class`) pick the kind in CSS, so
  no template needs a new field:

| Kind | Selectors |
|---|---|
| needs the owner | `.pill--undecided`, `.pill--to_order`, `.fit--unconfirmed` |
| on its way | `.pill--ordered` |
| settled | `.pill--received`, `.pill--installed`, `.pill--shop_supplies`, `.pill--done`, `.fit--confirmed` |
| problem | `.fit--does_not_fit`, `.pill--rejected` |
| plain fact | `.pill` with none of the above |
| database name | `.pill--env` (accent fill, weight 600, no mark) |

- [ ] **Step 1: Write the failing test.**

```rust
#[tokio::test]
async fn every_page_loads_the_bundled_typeface_and_labels_the_database() {
    let (_, body) = get_page(&db().await, "/").await;
    assert!(body.contains(r#"url("/assets/plex-sans-400.woff2")"#), "{body}");
    assert!(body.contains(r#"url("/assets/plex-sans-600.woff2")"#), "{body}");
    assert!(body.contains(r#"<span class="l">Database </span><span class="pill pill--env""#), "{body}");
    assert!(body.contains(r#"<span class="l">Today </span>"#), "{body}");
}
```

- [ ] **Step 2: Run it.** Expected: FAIL.

- [ ] **Step 3: Replace the stylesheet.** Start from the mockup's `<style>` block. Keep its token
  blocks but apply dark with `@media (prefers-color-scheme: dark)` on `:root` only (the mockup's
  `:has(#t-dark:checked)` switch and the `.mock` rules are not shipped). Replace the single
  `@font-face` with two, weights 400 and 600, pointing at the Task 1 routes, `font-display: swap`.
  Rename the mockup's pill modifiers to the selectors in the table above. Add
  `overflow-wrap: anywhere` to `.row > span` and `.step > span`. Keep `<meta name="color-scheme">`.

- [ ] **Step 4: Rewrite the header and banner** to the mockup's markup: wordmark, `Database` label
  with `<span class="pill pill--env" title="{{ chrome.environment_title }}">`, the Garage link,
  `Today` label with the `<time>`. The hint sentence and the banner's message, action and `back`
  field keep their current values.

- [ ] **Step 5: Run** `cargo test -p glovebox-web`. Expected: all pass. Pages are only partly styled
  until Task 6.

- [ ] **Step 6: Commit** `feat(web): ledger stylesheet, header and state pills`.

---

### Task 4: Job panels, need rows and options

**Files:**
- Modify: `glovebox-web/templates/job_box.html`, `glovebox-web/templates/need_row.html`
- Test: `glovebox-web/tests/actions.rs`, `glovebox-web/tests/pages.rs`

**Interfaces:**
- Consumes: `NeedCard.status_line`, the labels from Task 2, the classes from Task 3.
- Produces: the need row and option row markup below. Every existing form keeps its `action`,
  field names, `id` (`need-{id}`, `need-{id}-decide`, `need-{id}-supplier`, `opt-{id}-why`),
  `aria-*` attributes, error placement rules (`error_in`, `error_elsewhere`, `invalid`) and
  conditions (`can_decide`, `can_order`, `can_receive`, `can_unreceive`, `can_return`).

- [ ] **Step 1: Write the failing tests.** Update the assertion that expects
  `<span class="qty">2 needed</span>` to expect `<span class="qty">× 2</span>`. Add, using the
  seeded-appointment helper the existing tests use:

```rust
#[tokio::test]
async fn a_job_shows_one_header_line_and_each_part_in_its_own_columns() {
    // body is the appointment page with a need whose chosen option has every field.
    for head in ["Need", "Stage", "Brand", "Part number", "Description", "Seller", "Price", "Fit"] {
        assert!(body.contains(&format!("<span>{head}</span>")) || body.contains(&format!(r#"<span class="num">{head}</span>"#)), "{head}");
    }
    assert!(body.contains(r#"<span data-l="Brand">Volkswagen</span>"#), "{body}");
    assert!(body.contains(r#"<span class="pn" data-l="Part number">5Q0615601E</span>"#), "{body}");
    assert!(body.contains(r#"<span class="l">Reason</span>"#), "{body}");
}

#[tokio::test]
async fn an_option_without_a_part_number_spans_the_number_and_description_columns() {
    // The option has a description and no part number.
    assert!(body.contains(r#"<span class="pn span2" data-l="Part">Pollen cabin air filter</span>"#), "{body}");
    assert!(!body.contains(r#"data-l="Description">Pollen cabin air filter"#), "{body}");
}

#[tokio::test]
async fn a_chosen_option_without_a_seller_or_price_keeps_its_columns() {
    assert!(body.contains(r#"<span data-l="Seller"></span>"#), "{body}");
    assert!(body.contains(r#"<span class="num" data-l="Price"></span>"#), "{body}");
}
```

Remove the `#[ignore]` from Task 2's test.

- [ ] **Step 2: Run** `cargo test -p glovebox-web`. Expected: the four tests fail.

- [ ] **Step 3: Rewrite `job_box.html`** to the mockup's `article.sheet`: a `div.sheet-top` holding
  `header.sheet-head` (the linked title, the status pill, then `<span class="aside">` with the open
  count's number in `<span class="n">` and its text) and the `div.cols` header line. Add the class
  `aside--todo` when `j.open.n > 0`. A job with no needs keeps its "No needs yet." line.

- [ ] **Step 4: Rewrite `need_row.html`** to the mockup's structure:
  - `summary.row`: name (`key`, with `qty`), stage pill, then either the six part cells from
    `n.chosen` or `<span class="span">` holding `n.status_line` and, when the need has options,
    `n.options_note`. Capitalize the sentence in CSS with `.row .span::first-letter`.
  - The `memo` line: Supplier, Approach and (when present) Reason as `l` and value pairs. Then
    `n.purchase`, each of `n.notes` and the chosen option's link as their own `line` spans.
  - A part with no `description` puts `title` in `<span class="pn span2" data-l="Part">`, and
    `.span2 { grid-column: span 2; }`.
  - Options: each `label.row.opt` with the radio in `span.pick`, the same six cells, and a `memo`
    line for the rejected reason or the fit note. A non-chosen option's link goes in that line.
  - `div.work` blocks for "Your decision" and "Actions", with the wording from the spec's Wording
    table.

- [ ] **Step 5: Run** `cargo test -p glovebox-web`. Expected: all pass, none ignored.

- [ ] **Step 6: Commit** `feat(web): needs and options in ledger columns`.

---

### Task 5: Step rows, the appointment head and the forms

**Files:**
- Modify: `glovebox-web/templates/appointment.html`, `step_row.html`, `todo_row.html`,
  `order_form.html`, `return_form.html`
- Test: `glovebox-web/tests/pages.rs`, `glovebox-web/tests/actions.rs`

**Interfaces:**
- Consumes: classes from Task 3.
- Produces: step rows as `li.step` with four children in order: job title, need name (`key`),
  sentence, action (`act`). Forms use `form.formbox`.

- [ ] **Step 1: Write the failing tests.**

```rust
#[tokio::test]
async fn the_appointment_head_labels_each_value() {
    for label in ["Vehicle", "Shop", "Date", "Status", "Needs open", "To-dos open", "Jobs"] {
        assert!(body.contains(&format!(r#"<span class="l">{label}</span>"#)), "{label}");
    }
}

#[tokio::test]
async fn a_step_row_reads_job_then_need_then_sentence() {
    assert!(body.contains(r#"<li class="step"><span>Brakes</span><span class="key">Rear pads</span>"#), "{body}");
}

#[tokio::test]
async fn an_action_keeps_one_name() {
    assert_eq!(body.matches(">Return it<").count() % 2, 0, "opener and submit come in pairs");
    assert!(!body.contains("Wrong part, returning it"));
    assert!(!body.contains("Record the return"));
    assert!(!body.contains(">Add to-do<"));
    assert!(body.contains("Why are you returning it?"));
}
```

Add a test that posts an invalid cost to `/vehicles/{vid}/needs/{id}/ordered` and asserts the
re-rendered page has `<details class="drawer" open>` for that need's order form, a
`<p class="error" role="alert">` before its `div.fields`, and `aria-invalid="true"` on the cost
input. Reuse an existing failing-order test if one already asserts this and only its markup
strings need updating.

- [ ] **Step 2: Run** `cargo test -p glovebox-web`. Expected: the new tests fail.

- [ ] **Step 3: Rewrite the templates** to the mockup's markup. The appointment head prints the
  values it prints today, each under its label; the two counts and the job count come from the
  same expressions as now. The appointment's status stays a pill. Groups are `section.sheet`, with
  `sheet--todo` on Check, Decide and Order. A Done row keeps its stage pill after the sentence and
  its "Return it" opener (`drawer drawer--quiet`) under it. The order form keeps the option's
  listing link and its note about purchases. The decide row with no options keeps its
  "No options researched yet" note.

- [ ] **Step 4: Run** `cargo test -p glovebox-web`. Expected: all pass.

- [ ] **Step 5: Commit** `feat(web): step rows, labeled appointment head and one name per action`.

---

### Task 6: Home, job and error pages, narrow widths, docs

**Files:**
- Modify: `glovebox-web/templates/home.html`, `job.html`, `not_found.html`, `failure.html`,
  `base.html` (narrow-width rules only)
- Modify: `CLAUDE.md` (the "No script" bullet's policy sentence and the paragraph that begins
  "The look lives in one inline stylesheet")
- Test: `glovebox-web/tests/pages.rs`

- [ ] **Step 1: Write the failing test.** Update `home_lists_open_appointments_and_unscheduled_jobs`
  to expect each appointment as a `li.step`-style row with labeled values (`Date`, `Shop`) and the
  open counts as the shared text gives them, and "Add a to-do" on the opener and the button.

- [ ] **Step 2: Run it.** Expected: FAIL.

- [ ] **Step 3: Restyle the four templates** with the Task 3 classes: one `sheet` per vehicle
  section on home, the job page reusing `job_box.html` under its breadcrumb, and the error pages as
  a title and a sentence. No new classes unless a rule is missing.

- [ ] **Step 4: Check narrow widths by eye.** Serve the seeded `test` environment (seed it first
  with `just seed test` if `data/glovebox-test.db` is missing). At 390px and 800px every row reads
  as label and value pairs with no horizontal scroll, and every form is usable. Fix the rules under
  the `max-width: 68rem` block.

- [ ] **Step 5: Walk the pages** at 1280px in light and dark against the mockup: choose an option,
  mark ordered, mark received, return, tick and untick a to-do, and submit one invalid cost. The
  browser console shows no policy violation and both fonts load from `127.0.0.1`. Scroll a long job
  and confirm its head and header line stay at the top. Take a grayscale screenshot and confirm
  each pill kind is told apart by its mark.

- [ ] **Step 6: Update `CLAUDE.md`** so the look paragraph describes this design (one typeface in
  two weights and four sizes, panels on a tinted ground, gray for labels, pills with marks, boxes
  only on fields, solid button commits, outlined opens, underlined is minor) and the policy
  sentence says the typeface is the one thing the page loads besides its own HTML.

- [ ] **Step 7: Run** `just ci`. Expected: every step passes.

- [ ] **Step 8: Commit** `feat(web): ledger look on the home, job and error pages`.
