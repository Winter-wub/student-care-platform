//! The Manual Link picker (#143): on request, the Student detail page lists
//! the recent chat accounts with no live Link, ordered by what each account
//! itself did (newest first, blocked ones last), to Staff holding the
//! Manual Link Permission only; picking one posts the same Manual Link
//! (audit `link.create`), and the raw id stays available.

mod students_support;
mod support;

use harpseal::{DeploymentConfig, Inbound, Statement};
use harpseal_testing::{Browser, Harness, TestApp};
use students_support::{CONFIG, audit_json, boot, flash, htmx_get, run, signed_in};
use support::{Chat, live_link};

/// The `value` of each picker radio in `body`, in page order.
fn picked_ids(body: &str) -> Vec<String> {
    let key = "name=\"pick\" value=\"";
    body.match_indices(key)
        .map(|(at, _)| {
            let rest = &body[at + key.len()..];
            rest[..rest.find('"').unwrap()].to_string()
        })
        .collect()
}

/// The Student page of `id` with the picker's list open (`?pick=1`).
async fn picker(b: &Browser<'_>, id: &str) -> String {
    let r = b.get(&format!("/admin/students/{id}?pick=1")).await;
    assert_eq!(r.status, 200, "{}", r.body);
    r.body
}

/// `who` writes `text` as a webhook event with an id, as LINE and Telegram
/// send them, so the claim lands in `inbound_event`.
async fn wrote(h: &Harness, app: &TestApp, who: &str, text: &str, event: &str) {
    let mut c = Chat::new(h, app, who);
    let ev = Inbound::text(c.who.clone(), text, None).with_event_id(Some(event.into()), false);
    c.send(ev).await;
}

#[tokio::test]
async fn the_list_loads_on_request_and_picking_links_one() {
    let (h, app) = boot().await;
    // u1 is linked: never listed.
    Chat::new(&h, &app, "u1").link("68010001").await;
    Chat::new(&h, &app, "u8").follow().await;
    h.clock().advance_minutes(5);
    Chat::new(&h, &app, "u9").follow().await;
    let (b, staff_id) = signed_in(&h, &app, "Officer").await;

    // A plain view reads no list: only the control that loads it.
    let plain = b.get("/admin/students/s3").await.body;
    assert!(picked_ids(&plain).is_empty());
    assert!(plain.contains("hx-get=\"/admin/students/s3/links/recent\""));
    assert!(plain.contains("href=\"/admin/students/s3?pick=1#ml-pick\""));
    assert!(plain.contains("ขอให้นักศึกษาส่งข้อความอะไรก็ได้มาที่บอทตอนนี้"));
    assert!(
        !plain.contains("บนสุด"),
        "never tells Staff to pick the top one"
    );

    // HTMX gets the fragment; without it, the page with the list open.
    let frag = htmx_get(&b, "/admin/students/s3/links/recent").await;
    assert_eq!(frag.status, 200);
    assert_eq!(picked_ids(&frag.body), ["fake:u9", "fake:u8"]);
    assert!(!frag.body.contains("<h1"), "a fragment, not a page");
    let r = b.get("/admin/students/s3/links/recent").await;
    assert_eq!(r.status, 303);
    assert_eq!(
        r.header("location"),
        Some("/admin/students/s3?pick=1#ml-pick")
    );

    let page = picker(&b, "s3").await;
    assert_eq!(picked_ids(&page), ["fake:u9", "fake:u8"]);
    assert!(page.contains("บัญชีแชทที่ยังไม่ได้เชื่อม"));
    assert!(page.contains("ถามนักศึกษาว่าส่งข้อความเมื่อไร"));
    assert!(page.contains("ทักล่าสุด") && page.contains("ทักครั้งแรก"));
    assert!(page.contains("5 นาทีที่แล้ว"), "u8, relative");
    assert!(page.contains("ยังไม่ยอมรับประกาศความเป็นส่วนตัว"), "stage");
    // The raw id stays, closed, with its field names.
    assert!(page.contains("<details class=\"pick-typed\">"));
    assert!(page.contains("name=\"external_id\"") && page.contains("name=\"channel\""));
    let problems = harpseal_testing::a11y::problems(&page);
    assert!(problems.is_empty(), "{problems:?}");

    let r = b
        .post(
            "/admin/students/s3/links",
            "pick=fake%3Au9&channel=fake&external_id=",
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "ok:kernel.students.linked");
    assert!(!live_link(&h, "u9").await.is_empty());
    let (_, after) = audit_json(&h, "link.create").await;
    assert_eq!(after["method"], "manual");
    assert_eq!(after["channel"], "fake");
    let row = app
        .query("SELECT verified_by_staff_id FROM link WHERE method = 'manual'")
        .await;
    assert_eq!(
        row.get(0, "verified_by_staff_id").unwrap().as_text(),
        Some(staff_id.as_str())
    );
    // Linked now: off the list, on every Student's page.
    assert_eq!(picked_ids(&picker(&b, "s4").await), ["fake:u8"]);
}

#[tokio::test]
async fn a_message_that_keeps_the_flow_still_brings_the_account_up() {
    let (h, app) = boot().await;
    Chat::new(&h, &app, "u8").follow().await;
    h.clock().advance_minutes(5);
    Chat::new(&h, &app, "u9").follow().await;
    h.clock().advance_minutes(5);
    // u8 waits on the Consent Card; a text leaves it there.
    let before = app
        .query("SELECT flow_state FROM channel_account WHERE external_id = 'u8'")
        .await;
    wrote(&h, &app, "u8", "สวัสดีครับ", "ev-u8-1").await;
    let after = app
        .query("SELECT flow_state FROM channel_account WHERE external_id = 'u8'")
        .await;
    assert_eq!(format!("{:?}", before.rows), format!("{:?}", after.rows));
    assert_eq!(
        app.count("SELECT count(*) FROM inbound_event WHERE external_id = 'u8'")
            .await,
        1
    );
    let (b, _) = signed_in(&h, &app, "Officer").await;
    assert_eq!(picked_ids(&picker(&b, "s3").await), ["fake:u8", "fake:u9"]);
}

#[tokio::test]
async fn a_staff_revoke_does_not_lift_an_account_above_a_newer_message() {
    let (h, app) = boot().await;
    // s1 linked from u1 (wrongly, say); then the right account u9 writes.
    Chat::new(&h, &app, "u1").link("68010001").await;
    h.clock().advance_minutes(5);
    Chat::new(&h, &app, "u9").follow().await;
    wrote(&h, &app, "u9", "สวัสดีครับ", "ev-u9-1").await;
    h.clock().advance_minutes(1);
    // Staff then revoke u1 (here: s1 marked left, which revokes it and
    // touches channel_account.updated_at).
    let (b, _) = signed_in(&h, &app, "Officer").await;
    b.post("/admin/students/s1/status", "status=left&confirm=yes")
        .await;
    assert_eq!(flash(&b), "ok:kernel.students.status_saved");

    let page = picker(&b, "s3").await;
    assert_eq!(picked_ids(&page), ["fake:u9", "fake:u1"]);
    // u1 comes back with its stage: it accepted the notice before.
    assert!(page.contains("รอรหัสนักศึกษา"), "{page}");
}

#[tokio::test]
async fn blocked_accounts_sort_last_with_their_flag() {
    let (h, app) = boot().await;
    Chat::new(&h, &app, "u8").follow().await;
    h.clock().advance_minutes(5);
    let mut c = Chat::new(&h, &app, "u7");
    c.follow().await;
    c.send(Inbound::blocked(c.who.clone())).await;
    let (b, _) = signed_in(&h, &app, "Officer").await;
    let page = picker(&b, "s3").await;
    assert_eq!(picked_ids(&page), ["fake:u8", "fake:u7"]);
    assert_eq!(page.matches("บล็อกบอทอยู่").count(), 1);
}

#[tokio::test]
async fn two_accounts_writing_within_two_minutes_get_a_warning() {
    let (h, app) = boot().await;
    Chat::new(&h, &app, "u8").follow().await;
    h.clock().advance_minutes(10);
    Chat::new(&h, &app, "u9").follow().await;
    let (b, _) = signed_in(&h, &app, "Officer").await;
    let warn = "มีมากกว่าหนึ่งบัญชีที่ทักบอทในช่วง 2 นาทีที่ผ่านมา";
    assert!(!picker(&b, "s3").await.contains(warn));
    h.clock().advance_minutes(1);
    wrote(&h, &app, "u8", "สวัสดีครับ", "ev-u8-1").await;
    assert!(picker(&b, "s3").await.contains(warn));
}

#[tokio::test]
async fn refused_posts_name_their_reason() {
    let (h, app) = boot().await;
    Chat::new(&h, &app, "u1").link("68010001").await;
    Chat::new(&h, &app, "u8").follow().await;
    Chat::new(&h, &app, "u9").follow().await;
    let (b, _) = signed_in(&h, &app, "Officer").await;
    let post = |body: &'static str| b.post("/admin/students/s3/links", body);

    post("pick=fake%3Au9&channel=fake&external_id=u8").await;
    assert_eq!(flash(&b), "err:kernel.students.pick_both");
    post("pick=fake%3Anobody&channel=fake&external_id=").await;
    assert_eq!(flash(&b), "err:kernel.students.link_invalid");
    post("channel=fake&external_id=").await;
    assert_eq!(flash(&b), "err:kernel.students.link_invalid");
    post("pick=fake%3Au1").await;
    assert_eq!(flash(&b), "err:kernel.students.link_conflict");
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE method = 'manual'")
            .await,
        0
    );

    // The typed path alone still works.
    post("channel=fake&external_id=u8").await;
    assert_eq!(flash(&b), "ok:kernel.students.linked");
}

#[tokio::test]
async fn account_ids_are_escaped_and_long_ones_shortened() {
    let (h, app) = boot().await;
    Chat::new(&h, &app, "<b>x</b>").follow().await;
    Chat::new(&h, &app, "U4af4980629a1b2c3d4e5f60718293a4b")
        .follow()
        .await;
    let (b, _) = signed_in(&h, &app, "Officer").await;

    let body = picker(&b, "s3").await;
    assert!(!body.contains("<b>x</b>"), "raw markup from an id");
    assert!(body.contains("&lt;b&gt;x&lt;/b&gt;"));
    assert!(body.contains("U4af49…3a4b"));
    assert!(body.contains("value=\"fake:U4af4980629a1b2c3d4e5f60718293a4b\""));
}

#[tokio::test]
async fn only_staff_with_the_manual_link_permission_see_the_list() {
    let (h, app) = boot().await;
    Chat::new(&h, &app, "u9").follow().await;
    let (viewer, _) = signed_in(&h, &app, "Viewer").await;

    let body = picker(&viewer, "s3").await;
    assert!(picked_ids(&body).is_empty());
    assert!(!body.contains("u9"), "no account id without the Permission");
    assert!(!body.contains("/links/recent"));
    let frag = htmx_get(&viewer, "/admin/students/s3/links/recent").await;
    assert_eq!(frag.status, 403);
    assert!(!frag.body.contains("u9"));
    let r = viewer
        .post("/admin/students/s3/links", "pick=fake%3Au9")
        .await;
    assert_eq!(r.status, 403);
    assert_eq!(app.count("SELECT count(*) FROM link").await, 0);
}

#[tokio::test]
async fn production_lists_no_fake_accounts_and_refuses_a_fake_pick() {
    let h = Harness::new();
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    let dev = h
        .boot(h.builder().config(config.clone()).build().unwrap())
        .await;
    let app = h
        .boot(h.builder().dev(false).config(config).build().unwrap())
        .await;
    for sql in support::ROSTER {
        h.db().execute(Statement::new(*sql)).await.unwrap();
    }
    Chat::new(&h, &dev, "u9").follow().await;
    let (b, _) = signed_in(&h, &app, "Officer").await;

    let page = picker(&b, "s3").await;
    assert!(picked_ids(&page).is_empty());
    assert!(!page.contains("u9"));
    b.post("/admin/students/s3/links", "pick=fake%3Au9").await;
    assert_eq!(flash(&b), "err:kernel.students.link_dev_only");
    assert_eq!(app.count("SELECT count(*) FROM link").await, 0);
}

#[tokio::test]
async fn an_empty_list_says_what_to_do() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Officer").await;
    let body = picker(&b, "s3").await;
    assert!(picked_ids(&body).is_empty());
    assert!(body.contains("ขอให้นักศึกษาทักบอทก่อน แล้วโหลดรายการใหม่"));
}

#[tokio::test]
async fn accounts_silent_for_a_week_drop_off() {
    let (h, app) = boot().await;
    Chat::new(&h, &app, "u8").follow().await;
    h.clock().advance_minutes(8 * 24 * 60);
    Chat::new(&h, &app, "u9").follow().await;
    let (b, _) = signed_in(&h, &app, "Officer").await;
    assert_eq!(picked_ids(&picker(&b, "s3").await), ["fake:u9"]);
}

#[tokio::test]
async fn the_list_is_capped_at_twenty() {
    let (h, app) = boot().await;
    for i in 0..21 {
        Chat::new(&h, &app, &format!("u{i:02}")).follow().await;
        h.clock().advance_minutes(1);
    }
    let (b, _) = signed_in(&h, &app, "Officer").await;
    let body = picker(&b, "s3").await;
    let ids = picked_ids(&body);
    assert_eq!(ids.len(), 20);
    assert_eq!(ids[0], "fake:u20");
    assert!(!ids.contains(&"fake:u00".to_string()), "the oldest is cut");
    assert!(body.contains("แสดงเฉพาะ 20 บัญชีล่าสุด"));
}

#[tokio::test]
async fn the_query_reaches_every_table_through_an_index() {
    let (h, _app) = boot().await;
    run(&h, "ANALYZE").await;
    let sql = harpseal::students::manual_link_picker_sql(2).replace('?', "0");
    let plan = h
        .db()
        .query(Statement::new(format!("EXPLAIN QUERY PLAN {sql}")))
        .await
        .unwrap();
    let steps: Vec<String> = (0..plan.len())
        .map(|i| {
            plan.get(i, "detail")
                .unwrap()
                .as_text()
                .unwrap()
                .to_string()
        })
        .collect();
    let scans: Vec<&String> = steps
        .iter()
        // `cand` and the subquery are the small candidate sets in memory.
        .filter(|d| d.starts_with("SCAN ") && !d.starts_with("SCAN (subquery") && *d != "SCAN cand")
        .collect();
    assert!(scans.is_empty(), "{steps:#?}");
    let all = steps.join("\n");
    assert!(all.contains("inbound_event_sender"), "{all}");
    assert!(all.contains("channel_account_first_seen"), "{all}");
}
