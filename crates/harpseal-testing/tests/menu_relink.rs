//! The Menu re-link after a Menu label save (#66), at the Kernel seam:
//! a back-office save of a Menu label enqueues one Job per changed Locale;
//! the Job re-links every linked account of that Locale; a failed re-link
//! retries without retiring early; an unreachable account is passed over.

mod admin_support;
mod menu_support;
mod support;

use admin_support::{boot, signed_in};
use harpseal::{ChannelAccountId, Statement, Timestamp};
use harpseal_channel_fake::{FakeChannel, MenuEvent};
use harpseal_testing::Harness;
use menu_support::*;

#[tokio::test]
async fn a_label_save_enqueues_one_job_per_locale_and_other_texts_none() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let welcome = "/admin/texts/kernel.flow.welcome";
    let r = admin.post(welcome, &save("th", "ยินดีต้อนรับสู่ Seal Bay")).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(relink_jobs(&h).await, 0, "not a Menu label");

    let r = admin.post(UNLINK, &save("th", "เลิกเชื่อมบัญชี")).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(relink_jobs(&h).await, 1);
    let r = admin.post(UNLINK, &save("en", "Disconnect")).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(relink_jobs(&h).await, 2, "one per Locale");
    let th = count(
        &h,
        "SELECT count(*) AS n FROM job WHERE kind = 'kernel.menu.relink' \
         AND json_extract(payload, '$.locale') = 'th'",
    )
    .await;
    assert_eq!(th, 1);
    // Revision 0 (the Menu shown so far) and 1 per Locale.
    let revs = count(&h, "SELECT count(*) AS n FROM menu_revision").await;
    assert_eq!(revs, 4);
}

#[tokio::test]
async fn a_failed_relink_retries_and_never_retires_early() {
    let (h, app) = boot().await;
    let accounts = linked(&h, 5, "th").await;
    let fake = h.fake_channel();
    fake.set_relink_chunk(2);
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    admin.post(UNLINK, &save("th", "เลิกเชื่อมบัญชี")).await;
    // The second chunk fails after one account.
    let now = Timestamp(h.clock().now_ms());
    app.deployment().tick(now).await.unwrap();
    fake.fail_next_relink_at(1);
    h.clock().advance_minutes(1);
    let r = app
        .deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap();
    assert_eq!(r.retried, 1, "{r:?}");
    let retired = |events: &[MenuEvent]| {
        events
            .iter()
            .any(|e| matches!(e, MenuEvent::Retired { .. }))
    };
    assert!(!retired(&fake.menu_events()), "not before every account");
    drain(&h, &app).await;
    for a in &accounts {
        assert_eq!(revision_of(&h, a), 1, "{a:?}");
    }
    let events = fake.menu_events();
    assert!(
        matches!(events.last(), Some(MenuEvent::Retired { revisions, live: 1 }) if revisions == &[0])
    );
    assert_eq!(
        events.iter().filter(|e| retired(&[(*e).clone()])).count(),
        1
    );
}

#[tokio::test]
async fn one_unreachable_account_is_passed_over_and_the_relink_completes() {
    let (h, app) = boot().await;
    let accounts = linked(&h, 5, "th").await;
    let fake = h.fake_channel();
    fake.set_relink_chunk(2);
    fake.set_unreachable(&accounts[1], true);
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    admin.post(UNLINK, &save("th", "เลิกเชื่อมบัญชี")).await;
    drain(&h, &app).await;

    for (i, a) in accounts.iter().enumerate().filter(|(i, _)| *i != 1) {
        assert_eq!(revision_of(&h, a), 1, "account {i}");
    }
    assert!(
        fake.menu_events()
            .iter()
            .any(|e| matches!(e, MenuEvent::Retired { live: 1, .. })),
        "retired after the others were re-linked"
    );
    let unreachable = count(
        &h,
        "SELECT count(*) AS n FROM link WHERE id = 'ln-th01' AND status = 'unreachable'",
    )
    .await;
    assert_eq!(unreachable, 1, "the Link is marked unreachable");
    let live = count(
        &h,
        "SELECT count(*) AS n FROM menu_revision WHERE revision = 1 AND state = 'live'",
    )
    .await;
    assert_eq!(live, 1);
    let failed = count(
        &h,
        "SELECT count(*) AS n FROM job WHERE kind = 'kernel.menu.relink' AND state = 'failed'",
    )
    .await;
    assert_eq!(failed, 0);
}

/// A Channel that keeps the default `relink_menu` (set_menu per account),
/// on the Fake's state.
struct Plain(FakeChannel);

#[async_trait::async_trait]
impl harpseal::Channel for Plain {
    fn kind(&self) -> harpseal::ChannelKind {
        self.0.kind()
    }
    async fn parse_webhook(
        &self,
        req: harpseal::WebhookRequest,
    ) -> harpseal::Result<Vec<harpseal::Inbound>> {
        self.0.parse_webhook(req).await
    }
    async fn download(&self, file: &harpseal::IncomingFile) -> harpseal::Result<Vec<u8>> {
        self.0.download(file).await
    }
    async fn send(
        &self,
        to: &ChannelAccountId,
        d: &harpseal::Delivery,
        m: &harpseal::Message,
        l: &harpseal::Locale,
    ) -> harpseal::Result<harpseal::SendReceipt> {
        self.0.send(to, d, m, l).await
    }
    async fn set_menu(
        &self,
        to: &ChannelAccountId,
        menu: Option<&harpseal::Menu>,
    ) -> harpseal::Result<()> {
        self.0.set_menu(to, menu).await
    }
}

#[tokio::test]
async fn the_default_relink_passes_over_an_unreachable_account() {
    use harpseal::Channel as _;
    let h = Harness::new();
    let config = harpseal::DeploymentConfig::from_toml(admin_support::CONFIG).unwrap();
    let plain = h.fake_channel().with_kind("plain");
    let d = h
        .builder()
        .config(config)
        .channel(Plain(plain.clone()))
        .build()
        .unwrap();
    let app = h.boot(d).await;
    let mut to = Vec::new();
    for i in 0..5 {
        for sql in [
            format!(
                "INSERT INTO student (id, student_code, display_name, locale, status, created_at, \
                 updated_at) VALUES ('p{i}', '680500{i}', 'Seal {i}', 'th', 'active', 1, 1)"
            ),
            format!(
                "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
                 updated_at) VALUES ('pa{i}', 'plain', 'chat-{i}', 1, 'linked', 1)"
            ),
            format!(
                "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
                 linked_at, updated_at) VALUES ('pl{i}', 'p{i}', 'pa{i}', 'plain', 'active', \
                 'manual', 1, 1)"
            ),
        ] {
            h.db().execute(Statement::new(sql)).await.unwrap();
        }
        to.push(ChannelAccountId::new(plain.kind(), format!("chat-{i}")));
    }
    plain.set_unreachable(&to[1], true);
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    admin.post(UNLINK, &save("th", "เลิกเชื่อมบัญชี")).await;
    drain(&h, &app).await;

    for i in [0, 2, 3, 4] {
        assert_eq!(revision_of(&h, &to[i]), 1, "chat {i}");
    }
    let n = count(
        &h,
        "SELECT count(*) AS n FROM link WHERE id = 'pl1' AND status = 'unreachable'",
    )
    .await;
    assert_eq!(n, 1);
    let live = count(
        &h,
        "SELECT count(*) AS n FROM menu_revision WHERE revision = 1 AND state = 'live'",
    )
    .await;
    assert_eq!(
        live, 1,
        "the revision went live; nothing is stuck in linking"
    );
}
