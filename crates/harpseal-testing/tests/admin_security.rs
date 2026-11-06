//! Security Events page (#27) over HTTP: events produced by the chat
//! flow, filters, one sensitive-read row per request, Permission denials.

mod admin_support;
mod support;

use admin_support::{audits, boot, htmx_get, rows, run, signed_in};
use harpseal_testing::{Harness, TestApp};
use support::Chat;

/// The OTP lockout of s1, through the chat flow (as `chat_flow_security`).
async fn lockout(h: &Harness, app: &TestApp) {
    let mut phone = Chat::new(h, app, "phone");
    phone.link("68010001").await;
    phone.press("kernel.link.unlink").await;
    phone.press("kernel.link.unlink_confirm").await;
    let mut other = Chat::new(h, app, "tablet-0123456789abcdef");
    other.follow().await;
    other.press("kernel.consent.accept").await;
    other.text("68010001").await;
    let code = other.code();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for _ in 0..3 {
        other.text(wrong).await;
    }
}

#[tokio::test]
async fn events_are_listed_newest_first_filtered_and_each_request_is_logged_once() {
    let (h, app) = boot().await;
    lockout(&h, &app).await;
    // A second kind, a day later, about another Student.
    h.clock().advance_minutes(24 * 60);
    run(
        &h,
        &format!(
            "INSERT INTO security_event (id, at, kind, student_id, details) VALUES \
             ('e2', {}, 'link.duplicate_attempt', 's4', '{{\"channel\":\"fake\"}}')",
            h.clock().now_ms()
        ),
    )
    .await;
    let (b, _) = signed_in(&h, &app, "Viewer").await;

    let all = b.get("/admin/security").await;
    assert_eq!(all.status, 200, "{}", all.body);
    assert_eq!(rows(&all.body), 2);
    let (dup, lock) = (
        all.body.find("title=\"link.duplicate_attempt\"").unwrap(),
        all.body.find("title=\"otp.lockout\"").unwrap(),
    );
    assert!(dup < lock, "newest first");
    assert!(
        all.body.contains("href=\"/admin/students/s1\""),
        "Student linked"
    );
    assert!(all.body.contains("68010001"));
    assert!(all.body.contains("tablet…cdef"), "external id shortened");
    assert!(
        all.body.contains("<dt>channel</dt><dd>fake</dd>"),
        "details as key/value"
    );
    assert!(
        all.body.contains("<option value=\"otp.send_failed\""),
        "known kinds offered"
    );
    assert_eq!(audits(&h, "security.view").await, 1);

    let only = b.get("/admin/security?kind=otp.lockout").await;
    assert_eq!(rows(&only.body), 1);
    assert!(!only.body.contains("title=\"link.duplicate_attempt\""));
    assert_eq!(audits(&h, "security.view").await, 2);
    let target = admin_support::text(
        &h,
        "SELECT target_id FROM audit_log WHERE action = ? ORDER BY rowid DESC",
        "security.view",
        "target_id",
    )
    .await
    .unwrap();
    assert!(target.contains("kind=otp.lockout"), "{target}");

    // By Student code or name, and by local date.
    assert_eq!(
        rows(&b.get("/admin/security?student=68010004").await.body),
        1
    );
    assert_eq!(rows(&b.get("/admin/security?student=Nok").await.body), 1);
    assert_eq!(rows(&b.get("/admin/security?student=nobody").await.body), 0);
    assert_eq!(
        rows(&b.get("/admin/security?from=2026-11-06").await.body),
        1
    );
    assert_eq!(rows(&b.get("/admin/security?to=2026-11-05").await.body), 1);

    // The HTMX fragment is a request too; it varies on HX-Request.
    let before = audits(&h, "security.view").await;
    let frag = htmx_get(&b, "/admin/security?kind=link.duplicate_attempt").await;
    assert!(
        frag.body.starts_with("<div id=\"security-results\""),
        "{}",
        frag.body
    );
    assert_eq!(frag.header("vary"), Some("HX-Request"));
    assert_eq!(audits(&h, "security.view").await, before + 1);
}

#[tokio::test]
async fn staff_without_the_permission_get_403_and_nothing_is_logged() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Reader").await;
    assert_eq!(b.get("/admin/security").await.status, 403);
    assert_eq!(b.get("/admin/audit").await.status, 403);
    assert_eq!(audits(&h, "security.view").await, 0);
    let home = b.get("/admin").await.body;
    assert!(!home.contains("href=\"/admin/security\""), "not in the nav");
}
