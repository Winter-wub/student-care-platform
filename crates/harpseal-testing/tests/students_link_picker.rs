//! The Manual Link picker (#143): the Student detail page lists the
//! recent chat accounts with no live Link, newest activity first, to Staff
//! holding the Manual Link Permission only; picking one posts the same
//! Manual Link (audit `link.create`), and the raw id stays available.

mod students_support;
mod support;

use students_support::{audit_json, boot, flash, signed_in};
use support::{Chat, live_link};

/// Where the picker's list starts and the account rows in it, in order.
fn picked_ids(body: &str) -> Vec<String> {
    let key = "name=\"pick\" value=\"";
    body.match_indices(key)
        .map(|(at, _)| {
            let rest = &body[at + key.len()..];
            rest[..rest.find('"').unwrap()].to_string()
        })
        .collect()
}

#[tokio::test]
async fn lists_unlinked_accounts_newest_first_and_picking_links_one() {
    let (h, app) = boot().await;
    // u1 is linked: never listed.
    Chat::new(&h, &app, "u1").link("68010001").await;
    Chat::new(&h, &app, "u8").follow().await;
    h.clock().advance_minutes(5);
    Chat::new(&h, &app, "u9").follow().await;
    let (b, staff_id) = signed_in(&h, &app, "Officer").await;

    let page = b.get("/admin/students/s3").await;
    assert_eq!(page.status, 200);
    assert_eq!(picked_ids(&page.body), ["fake:u9", "fake:u8"]);
    assert!(page.body.contains("บัญชีแชทที่ยังไม่ได้เชื่อม"));
    assert!(page.body.contains("ขอให้นักศึกษาส่งข้อความอะไรก็ได้มาที่บอทตอนนี้"));
    assert!(page.body.contains("ทักล่าสุด") && page.body.contains("ทักครั้งแรก"));
    // The raw id stays, closed, with its field names.
    assert!(page.body.contains("<details class=\"pick-typed\">"));
    assert!(page.body.contains("name=\"external_id\""));
    assert!(page.body.contains("name=\"channel\""));
    assert!(
        harpseal_testing::a11y::problems(&page.body).is_empty(),
        "{:?}",
        harpseal_testing::a11y::problems(&page.body)
    );

    // u8 writes again: it moves to the top.
    h.clock().advance_minutes(5);
    Chat::new(&h, &app, "u8").text("สวัสดีครับ").await;
    let page = b.get("/admin/students/s3").await;
    assert_eq!(picked_ids(&page.body), ["fake:u8", "fake:u9"]);

    // Picking one links it, as the typed form did.
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
    // Linked now: off the list (on every Student's page).
    let page = b.get("/admin/students/s4").await;
    assert_eq!(picked_ids(&page.body), ["fake:u8"]);
}

#[tokio::test]
async fn a_pick_and_a_different_typed_id_are_refused_not_guessed() {
    let (h, app) = boot().await;
    Chat::new(&h, &app, "u8").follow().await;
    Chat::new(&h, &app, "u9").follow().await;
    let (b, _) = signed_in(&h, &app, "Officer").await;

    b.post(
        "/admin/students/s3/links",
        "pick=fake%3Au9&channel=fake&external_id=u8",
    )
    .await;
    assert_eq!(flash(&b), "err:kernel.students.pick_both");
    assert_eq!(app.count("SELECT count(*) FROM link").await, 0);

    // The typed path alone still works.
    b.post("/admin/students/s3/links", "channel=fake&external_id=u8")
        .await;
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

    let body = b.get("/admin/students/s3").await.body;
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

    let body = viewer.get("/admin/students/s3").await.body;
    assert!(picked_ids(&body).is_empty());
    assert!(
        !body.contains("u9"),
        "no account id for Staff without the Permission"
    );
    assert!(!body.contains("บัญชีแชทที่ยังไม่ได้เชื่อม"));
    let r = viewer
        .post("/admin/students/s3/links", "pick=fake%3Au9")
        .await;
    assert_eq!(r.status, 403);
    assert_eq!(app.count("SELECT count(*) FROM link").await, 0);
}

#[tokio::test]
async fn an_empty_list_says_what_to_do() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Officer").await;
    let body = b.get("/admin/students/s3").await.body;
    assert!(picked_ids(&body).is_empty());
    assert!(body.contains("ขอให้นักศึกษาทักบอทก่อน แล้วโหลดหน้านี้ใหม่"));
}

#[tokio::test]
async fn the_list_is_capped_at_twenty() {
    let (h, app) = boot().await;
    for i in 0..21 {
        Chat::new(&h, &app, &format!("u{i:02}")).follow().await;
        h.clock().advance_minutes(1);
    }
    let (b, _) = signed_in(&h, &app, "Officer").await;
    let body = b.get("/admin/students/s3").await.body;
    let ids = picked_ids(&body);
    assert_eq!(ids.len(), 20);
    assert_eq!(ids[0], "fake:u20");
    assert!(!ids.contains(&"fake:u00".to_string()), "the oldest is cut");
    assert!(body.contains("แสดงเฉพาะ 20 บัญชีล่าสุด"));
}
