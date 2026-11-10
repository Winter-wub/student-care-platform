//! The Audience fieldset by choice (#156): each choice shows only its own
//! fields (a whole page without JS shows them all; an HTMX answer starts
//! with the other choices' parts hidden), the count follows a change of
//! choice, and the count and the preview each say one short line per
//! change in their own live region instead of being live regions
//! themselves.

mod announcements_support;

use announcements_support::*;
use harpseal_testing::Browser;

const LIST: &str = "/admin/announcements/audiences";
const COUNT: &str = "/admin/announcements/audience";
const PREVIEW: &str = "/admin/announcements/preview";

async fn opened<'a>(w: &'a World) -> Browser<'a> {
    let b = signed_in(w, "Officer").await;
    assert_eq!(b.get("/admin/announcements/new").await.status, 200);
    b
}

/// The opening tag of the part for `choice`.
fn part<'a>(body: &'a str, choice: &str) -> &'a str {
    let at = body
        .find(&format!(r#"data-scope-for="{choice}""#))
        .unwrap_or_else(|| panic!("no {choice} part: {body}"));
    let end = at + body[at..].find('>').unwrap();
    &body[at..end]
}

fn hidden(body: &str, choice: &str) -> bool {
    part(body, choice).contains("hidden")
}

#[tokio::test]
async fn each_choice_holds_its_own_fields_and_all_show_without_script() {
    let w = boot().await;
    roster(&w).await;
    let b = opened(&w).await;
    let page = b.get("/admin/announcements/new").await;
    let body = &page.body;
    // The groups part holds the attributes and Tags, the students part
    // the hand-picked list.
    let groups = body.find(r#"data-scope-for="groups""#).unwrap();
    let students = body.find(r#"data-scope-for="students""#).unwrap();
    let at = |needle: &str| body.find(needle).unwrap_or_else(|| panic!("{needle}"));
    assert!(groups < at(r#"name="attr.programme""#));
    assert!(at(r#"id="tags""#) < students);
    assert!(students < at(r#"id="students""#));
    // A whole page: nothing hidden by the server, the script hides.
    assert!(
        !hidden(body, "groups") && !hidden(body, "students"),
        "{body}"
    );
    assert!(body.contains("window.hsScope"), "the script");

    // An HTMX answer (a saved Audience) for each choice.
    for (id, scope, rules) in [
        ("sa-all", "everyone", r#"{"v":1,"attributes":{},"tags":[]}"#),
        (
            "sa-grp",
            "groups",
            r#"{"v":1,"attributes":{"year":["2"]},"tags":[]}"#,
        ),
        (
            "sa-hp",
            "students",
            r#"{"v":1,"attributes":{},"tags":[],"students":["code-s1"]}"#,
        ),
    ] {
        run(
            &w.h,
            &format!(
                "INSERT INTO saved_audience (id, name, scope, audience, created_at, updated_at) \
                 VALUES ('{id}', 'n-{id}', '{scope}', '{rules}', 1, 1)"
            ),
        )
        .await;
        let used = b
            .post(&format!("{LIST}/use"), &format!("saved_audience={id}"))
            .await;
        assert_eq!(used.status, 200, "{}", used.body);
        let shown = |c: &str| !hidden(&used.body, c);
        match scope {
            "everyone" => assert!(!shown("groups") && !shown("students"), "{}", used.body),
            "groups" => assert!(shown("groups") && !shown("students"), "{}", used.body),
            _ => assert!(!shown("groups") && shown("students"), "{}", used.body),
        }
    }
}

#[tokio::test]
async fn the_count_follows_a_change_of_choice() {
    let w = boot().await;
    roster(&w).await;
    let b = opened(&w).await;
    let page = b.get("/admin/announcements/new").await;
    let at = page.body.find(r#"id="audience-count""#).unwrap();
    let tag = &page.body[at..at + page.body[at..].find('>').unwrap()];
    // Counted again on any change of the form, the choice included, and
    // not a live region itself.
    assert!(tag.contains("change from:closest form"), "{tag}");
    assert!(!tag.contains("aria-live"), "{tag}");
    // The same filters left filled in, three choices, three answers.
    let filled = "attr.year=3&students=code-s1";
    let mut lines = Vec::new();
    for scope in ["everyone", "groups", "students"] {
        let c = b
            .post(COUNT, &format!("scope={scope}&{filled}&count_was=x"))
            .await;
        assert_eq!(c.status, 200);
        let said = c.body.split(r#"id="count-said""#).nth(1).expect("a line");
        lines.push(said[..said.find("</div>").unwrap()].to_string());
    }
    assert!(lines[0].contains("กลุ่มผู้รับนี้มีนักศึกษา 3 คน"), "{lines:?}");
    assert!(lines[1].contains("กลุ่มผู้รับนี้มีนักศึกษา 1 คน"), "{lines:?}");
    assert!(lines[2].contains("กลุ่มผู้รับนี้มีนักศึกษา 1 คน"), "{lines:?}");
}

#[tokio::test]
async fn the_count_says_one_line_only_when_it_changes() {
    let w = boot().await;
    roster(&w).await;
    let b = opened(&w).await;
    // The first answer after the page loads says nothing.
    let first = b.post(COUNT, "scope=everyone&count_was=-").await;
    assert!(!first.body.contains("count-said"), "{}", first.body);
    // The box carries its line back.
    assert!(first.body.contains("กลุ่มผู้รับนี้มีนักศึกษา 3 คน"), "{}", first.body);
    let same = urlencoded("กลุ่มผู้รับนี้มีนักศึกษา 3 คน");
    let again = b
        .post(COUNT, &format!("scope=everyone&count_was={same}"))
        .await;
    assert!(!again.body.contains("count-said"), "{}", again.body);
    let changed = b
        .post(COUNT, &format!("scope=groups&count_was={same}"))
        .await;
    assert_eq!(changed.body.matches(r#"id="count-said""#).count(), 1);
    assert!(
        changed.body.contains(r#"hx-swap-oob="true""#),
        "{}",
        changed.body
    );
    assert!(changed.body.contains("ยังไม่ได้ระบุกลุ่ม"), "{}", changed.body);
}

fn urlencoded(s: &str) -> String {
    harpseal_testing::urlencode(s)
}

#[tokio::test]
async fn the_preview_says_one_line_when_what_it_shows_changes() {
    let w = boot().await;
    let b = opened(&w).await;
    let page = b.get("/admin/announcements/new").await;
    let at = page.body.find(r#"id="body-preview""#).unwrap();
    let tag = &page.body[at..at + page.body[at..].find('>').unwrap()];
    assert!(!tag.contains("aria-live"), "not a live region: {tag}");
    assert!(page.body.contains(r#"id="preview-said""#), "the region");
    let body = urlencoded("สวัสดี");
    // Empty → text: one line.
    let r = b
        .post(PREVIEW, &format!("body_th={body}&preview_was=empty"))
        .await;
    assert_eq!(r.status, 200);
    assert!(r.body.contains("ตัวอย่างแสดงข้อความแล้ว"), "{}", r.body);
    assert_eq!(r.body.matches(r#"id="preview-said""#).count(), 1);
    // More typing: the preview changes, nothing is said.
    let r = b
        .post(PREVIEW, &format!("body_th={body}{body}&preview_was=shown"))
        .await;
    assert!(!r.body.contains("preview-said"), "{}", r.body);
    // Cleared: one line.
    let r = b.post(PREVIEW, "body_th=&preview_was=shown").await;
    assert!(r.body.contains("ตัวอย่างว่าง"), "{}", r.body);
    // Too long: the error once.
    let long = "ก".repeat(2001);
    let r = b
        .post(
            PREVIEW,
            &format!("body_th={}&preview_was=shown", urlencoded(&long)),
        )
        .await;
    assert_eq!(
        r.body.matches("ยาวเกิน 2,000 ตัวอักษร").count(),
        2,
        "box and line"
    );
    let r = b
        .post(
            PREVIEW,
            &format!("body_th={}&preview_was=long", urlencoded(&long)),
        )
        .await;
    assert!(!r.body.contains("preview-said"), "{}", r.body);
}
