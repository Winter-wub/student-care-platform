//! Students pages (#22) over HTTP, Links: status left (revokes every
//! Link), Manual Link and revoke, each through `harpseal::links`.

mod students_support;
mod support;

use harpseal::Message;
use serde_json::json;
use students_support::{audit_json, boot, flash, one, run, signed_in};
use support::{Chat, REVOKED_NOTICE, live_link};

#[tokio::test]
async fn setting_left_asks_first_then_revokes_every_link_and_keeps_history() {
    let (h, app) = boot().await;
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    let link = live_link(&h, "u1").await;
    let (b, _) = signed_in(&h, &app, "Officer").await;
    h.fake_channel().take_sent();

    let ask = b.post("/admin/students/s1/status", "status=left").await;
    assert_eq!(ask.status, 200);
    assert!(
        ask.body
            .contains("บัญชีแชททุกช่องทางของนักศึกษาคนนี้จะถูกยกเลิกการผูก")
    );
    assert!(ask.body.contains("name=\"confirm\" value=\"yes\""));
    assert_eq!(
        one(&app, "SELECT status FROM student WHERE id = 's1'", "status")
            .await
            .as_deref(),
        Some("active")
    );

    let r = b
        .post("/admin/students/s1/status", "status=left&confirm=yes")
        .await;
    assert_eq!(r.status, 303);
    assert_eq!(flash(&b), "ok:kernel.students.status_saved");
    assert_eq!(
        one(&app, "SELECT status FROM student WHERE id = 's1'", "status")
            .await
            .as_deref(),
        Some("left")
    );
    let rows = app.query("SELECT id, status FROM link").await;
    assert_eq!(rows.len(), 1, "history kept");
    assert_eq!(rows.get(0, "id").unwrap().as_text(), Some(link.as_str()));
    assert_eq!(rows.get(0, "status").unwrap().as_text(), Some("revoked"));
    let seen = c.seen_since();
    assert!(
        matches!(&seen[..], [Message::Text(t)] if t == REVOKED_NOTICE),
        "{seen:?}"
    );
    assert!(matches!(c.menu(), Some(None)), "Menu locked");
    assert_eq!(
        audit_json(&h, "student.status").await.0,
        json!({ "status": "active", "manual_overrides": [] })
    );
    assert_eq!(
        audit_json(&h, "student.status").await.1,
        json!({ "status": "left", "manual_overrides": ["status"] })
    );
    assert_eq!(
        audit_json(&h, "link.revoke").await.1,
        json!({ "status": "revoked" })
    );
    assert!(
        !b.get("/admin/students/s1").await.body.contains("/links\""),
        "no Manual Link for a left Student"
    );

    // Back to active: no Link comes back.
    b.post("/admin/students/s1/status", "status=active&confirm=yes")
        .await;
    assert_eq!(
        one(&app, "SELECT status FROM student WHERE id = 's1'", "status")
            .await
            .as_deref(),
        Some("active")
    );
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status <> 'revoked'")
            .await,
        0
    );
}

#[tokio::test]
async fn manual_link_happy_path_unknown_account_and_duplicate() {
    let (h, app) = boot().await;
    let mut c = Chat::new(&h, &app, "u9");
    c.follow().await;
    let (b, staff_id) = signed_in(&h, &app, "Officer").await;
    h.fake_channel().take_sent();

    let r = b
        .post("/admin/students/s3/links", "channel=fake&external_id=+u9+")
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "ok:kernel.students.linked");
    let row = app
        .query("SELECT method, status, verified_by_staff_id FROM link")
        .await;
    assert_eq!(row.get(0, "method").unwrap().as_text(), Some("manual"));
    assert_eq!(row.get(0, "status").unwrap().as_text(), Some("active"));
    assert_eq!(
        row.get(0, "verified_by_staff_id").unwrap().as_text(),
        Some(staff_id.as_str())
    );
    assert!(!c.seen_since().is_empty(), "the notice was Pushed");
    let detail = b.get("/admin/students/s3").await.body;
    assert!(detail.contains("เจ้าหน้าที่ผูกให้") && detail.contains("ผูกโดย Test Staff"));

    b.post(
        "/admin/students/s3/links",
        "channel=fake&external_id=nobody",
    )
    .await;
    assert_eq!(flash(&b), "err:kernel.students.link_invalid");
    b.post("/admin/students/s1/links", "channel=fake&external_id=u9")
        .await;
    assert_eq!(
        flash(&b),
        "err:kernel.students.link_conflict",
        "the account is taken"
    );
    b.post("/admin/students/s3/links", "channel=fake&external_id=u9")
        .await;
    assert_eq!(flash(&b), "err:kernel.students.link_conflict");
    b.post("/admin/students/s3/links", "channel=line&external_id=u9")
        .await;
    assert_eq!(
        flash(&b),
        "err:kernel.students.link_invalid",
        "no such Channel"
    );
    assert_eq!(app.count("SELECT count(*) FROM link").await, 1);
}

#[tokio::test]
async fn revoke_asks_for_confirmation_then_notifies() {
    let (h, app) = boot().await;
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    let link = live_link(&h, "u1").await;
    let (b, _) = signed_in(&h, &app, "Officer").await;
    h.fake_channel().take_sent();
    let path = format!("/admin/students/s1/links/{link}/revoke");

    let detail = b.get("/admin/students/s1").await.body;
    assert!(detail.contains(&format!("action=\"{path}\"")));
    let ask = b.post(&path, "").await;
    assert_eq!(ask.status, 200);
    assert!(ask.body.contains("ยกเลิกการผูกบัญชีนี้?"));
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status = 'active'")
            .await,
        1
    );

    let r = b.post(&path, "confirm=yes").await;
    assert_eq!(r.status, 303);
    assert_eq!(flash(&b), "ok:kernel.students.revoked");
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status = 'revoked'")
            .await,
        1
    );
    let seen = c.seen_since();
    assert!(
        matches!(&seen[..], [Message::Text(t)] if t == REVOKED_NOTICE),
        "{seen:?}"
    );
    b.post(&path, "confirm=yes").await;
    assert_eq!(flash(&b), "err:kernel.students.already_revoked");
    // A Link of another Student is not reachable through this one.
    let other = b
        .post(
            &format!("/admin/students/s3/links/{link}/revoke"),
            "confirm=yes",
        )
        .await;
    assert_eq!(other.status, 404);
}

#[tokio::test]
async fn setting_left_revokes_two_links_and_survives_a_failed_notice() {
    let (h, app) = boot().await;
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    // A second live Link on a Channel this Deployment no longer has: its id
    // sorts first, so the Fake Link is the second one in the batch.
    run(
        &h,
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) \
         VALUES ('00000000-ghost-account', 'ghost', 'g1', 1, 1)",
    )
    .await;
    run(
        &h,
        "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
         linked_at, updated_at) VALUES ('00000000-ghost-link', 's1', '00000000-ghost-account', \
         'ghost', 'active', 'manual', 1, 1)",
    )
    .await;
    let (b, _) = signed_in(&h, &app, "Officer").await;
    h.fake_channel().take_sent();

    let r = b
        .post("/admin/students/s1/status", "status=left&confirm=yes")
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "ok:kernel.students.status_saved_unnotified");
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status = 'revoked'")
            .await,
        2
    );
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'link.revoke'")
            .await,
        2
    );
    let seen = c.seen_since();
    assert!(
        matches!(&seen[..], [Message::Text(t)] if t == REVOKED_NOTICE),
        "the Fake account is told although the first notice failed: {seen:?}"
    );
}
