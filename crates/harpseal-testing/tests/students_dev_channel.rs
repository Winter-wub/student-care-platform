//! A dev-only Channel outside dev mode (#131): display only. The back
//! office does not offer the Fake Channel (no empty row, no Manual Link
//! option), but a Link that exists on it is still shown and can be
//! revoked, and a crafted Manual Link post to it is refused.

mod students_support;
mod support;

use harpseal::{DeploymentConfig, Statement};
use harpseal_testing::{Harness, TestApp};
use students_support::{CONFIG, flash, signed_in};
use support::{Chat, live_link};

/// The students tests' Deployment, built without dev mode.
async fn boot_production() -> (Harness, TestApp) {
    let h = Harness::new();
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    let app = h
        .boot(h.builder().dev(false).config(config).build().unwrap())
        .await;
    for sql in support::ROSTER {
        h.db().execute(Statement::new(*sql)).await.unwrap();
    }
    (h, app)
}

#[tokio::test]
async fn an_existing_fake_link_still_shows_and_can_be_revoked() {
    let (h, app) = boot_production().await;
    // The Fake Channel still takes webhooks: a Student links over it.
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    let link = live_link(&h, "u1").await;
    let (b, _) = signed_in(&h, &app, "Officer").await;

    let detail = b.get("/admin/students/s1").await.body;
    assert!(detail.contains("fake: "), "the Link's row: {detail}");
    let path = format!("/admin/students/s1/links/{link}/revoke");
    assert!(detail.contains(&format!("action=\"{path}\"")), "{detail}");
    assert!(!detail.contains("<option value=\"fake\">"), "not offered");
    let list = b.get("/admin/students").await.body;
    assert_eq!(list.matches("fake: ").count(), 1, "only s1's row: {list}");
    // Another Student without a Fake Link has no empty Fake row.
    assert!(!b.get("/admin/students/s3").await.body.contains("fake: "));

    assert_eq!(b.post(&path, "confirm=yes").await.status, 303);
    assert_eq!(flash(&b), "ok:kernel.students.revoked");
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status = 'revoked'")
            .await,
        1
    );
    // Revoked, it moves to the history; the empty row goes.
    let after = b.get("/admin/students/s1").await.body;
    assert!(!after.contains("fake: "), "{after}");
    assert!(after.contains("fake · u1"), "{after}");
}

#[tokio::test]
async fn a_manual_link_to_the_fake_channel_is_refused_outside_dev_mode() {
    let (h, app) = boot_production().await;
    let mut c = Chat::new(&h, &app, "u9");
    c.follow().await;
    let (b, _) = signed_in(&h, &app, "Officer").await;
    let r = b
        .post("/admin/students/s3/links", "channel=fake&external_id=u9")
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "err:kernel.students.link_dev_only");
    assert_eq!(app.count("SELECT count(*) FROM link").await, 0);
    // The Thai message names why.
    let page = b.get("/admin/students/s3").await.body;
    assert!(page.contains("ช่องทางนี้มีไว้ทดสอบระบบเท่านั้น"), "{page}");
}
