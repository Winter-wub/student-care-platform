//! Announcements and a dev-only Channel (#133): deliveries left pending on
//! the Fake Channel when the Deployment leaves dev mode are skipped as
//! unreachable, so the Announcement still finishes.

mod announcements_support;

use announcements_support::*;
use harpseal::modules::announcements::Announcements;
use harpseal::{Deployment, DeploymentConfig, EmailOtp, Timestamp};
use harpseal_testing::{Harness, TestApp};

fn deployment(h: &Harness, dev: bool) -> Deployment {
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    Deployment::builder()
        .dev(dev)
        .config(config.clone())
        .runtime(h.services())
        .channel(h.fake_channel())
        .verification(EmailOtp::new(config.otp))
        .module(Announcements)
        .build()
        .unwrap()
}

async fn tick(h: &Harness, app: &TestApp) -> usize {
    let r = app
        .deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    r.claimed
}

async fn one(app: &TestApp, sql: &'static str) -> String {
    let rows = app.query(sql).await;
    rows.get(0, "v")
        .and_then(|v| v.as_text())
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn a_delivery_pending_on_the_fake_channel_outside_dev_is_skipped_and_the_round_finishes() {
    let h = Harness::new();
    let dev = h.boot(deployment(&h, true)).await;
    student(&h, "s1", "th", "active", "{}").await;
    link(&h, "s1", "fake", "active").await;
    h.create_staff("officer@sealbay.example", PASSWORD, &["Officer"])
        .await;
    let b = dev.browser();
    b.login("officer@sealbay.example", PASSWORD).await;
    assert_eq!(b.get("/admin/announcements").await.status, 200);
    create(&b, &form("ข่าว", "รายละเอียด", "&do=confirm")).await;
    // In dev mode the round starts: its delivery to the Fake Link is
    // pending when the Deployment is redeployed without dev mode.
    assert!(tick(&h, &dev).await > 0);
    let pending = "SELECT status AS v FROM announcement_delivery";
    assert_eq!(one(&dev, pending).await, "pending");
    h.fake_channel().take_sent();

    let app = h.boot(deployment(&h, false)).await;
    for _ in 0..20 {
        if tick(&h, &app).await == 0 {
            break;
        }
    }
    let d = "SELECT status || ':' || error AS v FROM announcement_delivery";
    assert_eq!(one(&app, d).await, "skipped:unreachable");
    assert_eq!(
        one(&app, "SELECT status AS v FROM announcement").await,
        "sent"
    );
    assert!(h.fake_channel().sent().is_empty(), "nothing sent");
}

#[tokio::test]
async fn outside_dev_a_student_linked_only_on_the_fake_channel_is_unlinked() {
    let h = Harness::new();
    let app = h.boot(deployment(&h, false)).await;
    student(&h, "s1", "th", "active", "{}").await;
    link(&h, "s1", "fake", "active").await;
    let code = one(
        &app,
        "SELECT student_code AS v FROM student WHERE id = 's1'",
    )
    .await;
    let mut a = harpseal::audience::Audience::everyone();
    a.students = vec![code.clone()];
    let picked = harpseal::audience::hand_picked(app.deployment(), &a)
        .await
        .unwrap();
    assert_eq!(picked.linked, 0);
    assert_eq!(picked.unlinked, vec![code.clone()]);
    // In dev mode the same Student is linked.
    let dev = h.boot(deployment(&h, true)).await;
    let picked = harpseal::audience::hand_picked(dev.deployment(), &a)
        .await
        .unwrap();
    assert_eq!((picked.linked, picked.unlinked.len()), (1, 0));
}
