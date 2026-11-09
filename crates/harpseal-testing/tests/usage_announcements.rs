//! Usage beside Announcements (#26): the send form's live estimate shows
//! this month's usage per Channel, and a failed multicast group counts in
//! `usage_daily.failed` (one per Message per recipient in the group).

mod announcements_support;

use announcements_support::*;
use harpseal::modules::announcements::Announcements;
use harpseal::{Deployment, DeploymentConfig, EmailOtp, Locale, Message, Statement};
use harpseal_channel_fake::FakeProfile;
use harpseal_testing::Harness;

const TODAY: &str = "2026-11-05";

async fn usage(h: &Harness, channel: &str, day: &str, metered: i64) {
    h.db()
        .execute(
            Statement::new("INSERT INTO usage_daily (channel, day, metered) VALUES (?, ?, ?)")
                .bind(channel)
                .bind(day)
                .bind(metered),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn the_estimate_shows_this_months_usage_without_a_quota() {
    let w = boot().await;
    roster(&w).await;
    usage(&w.h, "fake", "2026-10-31", 500).await; // last month: not counted
    usage(&w.h, "fake", TODAY, 42).await;
    let b = signed_in(&w, "Officer").await;
    let c = b
        .post("/admin/announcements/audience", "attr.programme=MSC")
        .await;
    assert_eq!(c.status, 200);
    assert!(
        c.body
            .contains("<span class=\"month-used\">ใช้ไปแล้ว 42 เดือนนี้</span>"),
        "{}",
        c.body
    );
    assert!(c.body.contains("ใช้ไปแล้ว 0 เดือนนี้"), "{}", c.body);
}

#[tokio::test]
async fn the_estimate_shows_used_of_the_cap_with_a_quota() {
    let h = Harness::new().with_fake_profile(FakeProfile::LineLike);
    let src = format!("{CONFIG}\n[quotas.fake]\nmetered_per_month = 300\n");
    let config = DeploymentConfig::from_toml(&src).unwrap();
    // Dev mode: the Fake Channel stands in for LINE here, and only dev
    // mode shows a dev-only Channel's Quota (#131).
    let d = Deployment::builder()
        .dev(true)
        .config(config.clone())
        .runtime(h.services())
        .channel(h.fake_channel())
        .verification(EmailOtp::new(config.otp))
        .module(Announcements)
        .build()
        .unwrap();
    let app = h.boot(d).await;
    usage(&h, "fake", TODAY, 245).await;
    student(&h, "s1", "th", "active", "{}").await;
    link(&h, "s1", "fake", "active").await;
    h.create_staff("officer@sealbay.example", PASSWORD, &["Officer"])
        .await;
    let b = app.browser();
    b.login("officer@sealbay.example", PASSWORD).await;
    assert_eq!(b.get("/admin/announcements/new").await.status, 200);
    let c = b
        .post("/admin/announcements/audience", "scope=everyone")
        .await;
    assert_eq!(c.status, 200);
    assert!(c.body.contains("ใช้ไปแล้ว 245/300 เดือนนี้"), "{}", c.body);
    // 245 of 300 is past 80 %: the Officer's pages carry the banner.
    assert!(
        b.get("/admin/announcements")
            .await
            .body
            .contains("usage-line warn")
    );
}

#[tokio::test]
async fn outside_dev_mode_the_estimate_lists_a_dev_only_channels_reach_not_its_usage() {
    // As the Quota banner (#131): the Fake Channel's usage and cap are not
    // shown outside dev mode, but the Students it reaches still are.
    let h = Harness::new().with_fake_profile(FakeProfile::LineLike);
    let src = format!("{CONFIG}\n[quotas.fake]\nmetered_per_month = 300\n");
    let config = DeploymentConfig::from_toml(&src).unwrap();
    let d = Deployment::builder()
        .config(config.clone())
        .runtime(h.services())
        .channel(h.fake_channel())
        .verification(EmailOtp::new(config.otp))
        .module(Announcements)
        .build()
        .unwrap();
    let app = h.boot(d).await;
    usage(&h, "fake", TODAY, 245).await;
    student(&h, "s1", "th", "active", "{}").await;
    link(&h, "s1", "fake", "active").await;
    h.create_staff("officer@sealbay.example", PASSWORD, &["Officer"])
        .await;
    let b = app.browser();
    b.login("officer@sealbay.example", PASSWORD).await;
    assert_eq!(b.get("/admin/announcements/new").await.status, 200);
    let c = b
        .post("/admin/announcements/audience", "scope=everyone")
        .await;
    assert_eq!(c.status, 200);
    assert!(c.body.contains("fake: 1"), "{}", c.body);
    assert!(!c.body.contains("month-used"), "{}", c.body);
    assert!(!c.body.contains("245"), "{}", c.body);
}

#[tokio::test]
async fn a_failed_multicast_group_counts_each_recipient_in_it_as_failed() {
    let w = boot_with(2).await;
    let to: Vec<_> = (1..=5)
        .map(|i| fake_account("fake", &format!("s{i}")))
        .collect();
    let msgs = vec![Message::text("ประกาศหนึ่ง"), Message::text("ประกาศสอง")];
    w.line.fail_next_batch_at_with(1, "fake: 500");
    let sent = w
        .app
        .deployment()
        .send_many_with_key("fake", &to, &msgs, &Locale::th(), None, "k-usage")
        .await
        .unwrap();
    assert_eq!(sent.results.len(), 2);
    assert!(sent.stopped.is_some());
    let failed = count(&w, "SELECT failed FROM usage_daily WHERE channel = 'fake'").await;
    let metered = count(&w, "SELECT metered FROM usage_daily WHERE channel = 'fake'").await;
    // Group 1 (2 recipients) x 2 Messages; group 0 went out (1 unit each).
    assert_eq!((failed, metered), (4, 2));
    // The first group failing: nothing went out, failures still count.
    w.line.fail_next_batch_at_with(0, "fake: 500");
    let sent = w
        .app
        .deployment()
        .send_many_with_key("fake", &to[4..], &msgs, &Locale::th(), None, "k-usage-2")
        .await
        .unwrap();
    assert!(sent.results.is_empty() && sent.stopped.is_some());
    let failed = count(&w, "SELECT failed FROM usage_daily WHERE channel = 'fake'").await;
    assert_eq!(failed, 6);
    // A 429 on a group adds nothing: the Job sends it again later.
    w.line.fail_next_batch_at(0);
    let sent = w
        .app
        .deployment()
        .send_many_with_key("fake", &to[4..], &msgs, &Locale::th(), None, "k-usage-3")
        .await
        .unwrap();
    assert!(sent.stopped.is_some());
    let failed = count(&w, "SELECT failed FROM usage_daily WHERE channel = 'fake'").await;
    assert_eq!(failed, 6);
}
