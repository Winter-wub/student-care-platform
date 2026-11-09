//! A dev-only Channel outside dev mode (#131, #133). The back office does
//! not offer the Fake Channel (no empty row, no Manual Link option), but a
//! Link that exists on it (made while in dev mode) is still shown and can
//! be revoked, with nothing sent to it; a crafted Manual Link post to it is
//! refused.

mod students_support;
mod support;

use harpseal::{DeploymentConfig, Statement};
use harpseal_testing::{Harness, TestApp};
use students_support::{CONFIG, flash, signed_in};
use support::{Chat, live_link};

/// The students tests' Deployment without dev mode, and (on the same
/// database) one in dev mode: the Fake Channel takes no traffic outside dev
/// mode (#133), so Students link over it on the dev one.
async fn boot_production() -> (Harness, TestApp, TestApp) {
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
    (h, dev, app)
}

#[tokio::test]
async fn an_existing_fake_link_still_shows_and_can_be_revoked() {
    let (h, dev, app) = boot_production().await;
    // A Student linked over it while the Deployment was in dev mode.
    let mut c = Chat::new(&h, &dev, "u1");
    c.link("68010001").await;
    h.fake_channel().take_sent();
    let menu = c.menu();
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
    // Nothing is sent to it (#133): no notice (Staff are told the Student
    // was not), the Menu left as it was.
    assert_eq!(flash(&b), "ok:kernel.students.revoked_unnotified");
    assert!(h.fake_channel().sent().is_empty());
    assert_eq!(format!("{:?}", c.menu()), format!("{menu:?}"));
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
    let (h, dev, app) = boot_production().await;
    let mut c = Chat::new(&h, &dev, "u9");
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
