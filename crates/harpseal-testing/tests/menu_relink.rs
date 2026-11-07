//! The Menu re-link after a Menu label save (#66), at the Kernel seam: a
//! back-office save of a Menu label enqueues one Job per changed Locale;
//! the Job re-links every linked account of that Locale (the Fake sees it,
//! no Inbound needed) and only then retires the older revisions; a failed
//! re-link retries without retiring early; a save mid-run supersedes it.

mod admin_support;
mod support;

use admin_support::{boot, signed_in};
use harpseal::{ChannelAccountId, Statement, Timestamp};
use harpseal_channel_fake::{FakeChannel, MenuEvent};
use harpseal_testing::{Harness, TestApp, urlencode};
use support::Chat;

const UNLINK: &str = "/admin/texts/kernel.menu.unlink";

fn save(locale: &str, value: &str) -> String {
    format!(
        "locale={locale}&value={}&version=-&op=save",
        urlencode(value)
    )
}

/// `n` more linked accounts of Students in `locale` (`u00`, `u01`, ...).
async fn linked(h: &Harness, n: usize, locale: &str) -> Vec<ChannelAccountId> {
    let mut out = Vec::new();
    for i in 0..n {
        let id = format!("{locale}{i:02}");
        let sql = [
            format!(
                "INSERT INTO student (id, student_code, display_name, locale, status, created_at, \
                 updated_at) VALUES ('st-{id}', '6802{i:04}{locale}', 'Seal {id}', '{locale}', \
                 'active', 1, 1)"
            ),
            format!(
                "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
                 updated_at) VALUES ('ca-{id}', 'fake', 'u-{id}', 1, 'linked', 1)"
            ),
            format!(
                "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
                 linked_at, updated_at) VALUES ('ln-{id}', 'st-{id}', 'ca-{id}', 'fake', 'active', \
                 'manual', 1, 1)"
            ),
        ];
        for s in sql {
            h.db().execute(Statement::new(s)).await.unwrap();
        }
        out.push(FakeChannel::account(format!("u-{id}")));
    }
    out
}

async fn count(h: &Harness, sql: &str) -> i64 {
    let rows = h.db().query(Statement::new(sql.to_string())).await.unwrap();
    rows.get(0, "n").and_then(|v| v.as_i64()).unwrap_or(-1)
}

async fn relink_jobs(h: &Harness) -> i64 {
    count(
        h,
        "SELECT count(*) AS n FROM job WHERE kind = 'kernel.menu.relink' \
         AND json_extract(payload, '$.revision') IS NULL",
    )
    .await
}

/// Ticks a minute apart until no re-link Job is queued (at most 30).
async fn drain(h: &Harness, app: &TestApp) {
    for _ in 0..30 {
        let now = Timestamp(h.clock().now_ms());
        app.deployment().tick(now).await.unwrap();
        let left = count(
            h,
            "SELECT count(*) AS n FROM job WHERE kind = 'kernel.menu.relink' AND state = 'queued'",
        )
        .await;
        if left == 0 {
            return;
        }
        h.clock().advance_minutes(1);
    }
    panic!("re-link Jobs never drained");
}

fn label_of(h: &Harness, to: &ChannelAccountId) -> Vec<String> {
    let menu = h.fake_channel().menu(to).flatten().expect("Menu unlocked");
    menu.entries.iter().map(|e| e.label.clone()).collect()
}

fn revision_of(h: &Harness, to: &ChannelAccountId) -> u32 {
    h.fake_channel()
        .menu(to)
        .flatten()
        .expect("Menu unlocked")
        .revision()
}

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
async fn the_fake_sees_the_relink_then_the_retire_and_the_new_labels() {
    let (h, app) = boot().await;
    let mut c = Chat::new(&h, &app, "linked");
    c.link("68010001").await;
    let first = FakeChannel::account("linked");
    assert_eq!(revision_of(&h, &first), 0);
    let en = linked(&h, 1, "en").await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let edited = "เลิกเชื่อมบัญชี";
    admin.post(UNLINK, &save("th", edited)).await;
    drain(&h, &app).await;

    // No Inbound: the re-link alone shows the new label.
    assert!(label_of(&h, &first).contains(&edited.to_string()));
    assert_eq!(revision_of(&h, &first), 1);
    assert_eq!(
        h.fake_channel().menu_events(),
        [
            MenuEvent::Relinked {
                accounts: vec![first.clone()],
                revision: 1
            },
            MenuEvent::Retired {
                revisions: vec![0],
                live: 1
            },
        ],
        "the en account is not re-linked; th re-linked before the retire"
    );
    assert!(h.fake_channel().menu(&en[0]).is_none());
    // A later set_menu carries the newest revision.
    c.follow().await;
    assert_eq!(revision_of(&h, &first), 1);
    let states = count(
        &h,
        "SELECT count(*) AS n FROM menu_revision WHERE locale = 'th' \
         AND ((revision = 0 AND state = 'retired') OR (revision = 1 AND state = 'live'))",
    )
    .await;
    assert_eq!(states, 2);
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
async fn a_second_save_mid_run_ends_with_only_the_newest_revision_live() {
    let (h, app) = boot().await;
    let accounts = linked(&h, 5, "th").await;
    let fake = h.fake_channel();
    fake.set_relink_chunk(2);
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    admin.post(UNLINK, &save("th", "เลิกเชื่อมบัญชี")).await;
    app.deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap();
    // Revision 1 reached two accounts; now revision 2 is saved.
    let reached = |r: u32| {
        let seen = |a: &ChannelAccountId| h.fake_channel().menu(a).flatten().map(|m| m.revision());
        accounts.iter().filter(|a| seen(a) == Some(r)).count()
    };
    assert_eq!(reached(1), 2, "mid-run");
    let token = {
        let body = admin.get(UNLINK).await.body;
        let at = body.find("name=\"locale\" value=\"th\"").unwrap();
        admin_support::hidden(&body[at..], "version")
    };
    let form = format!(
        "locale=th&value={}&version={}&op=save",
        urlencode("ยกเลิกการเชื่อมบัญชี"),
        urlencode(&token)
    );
    let r = admin.post(UNLINK, &form).await;
    assert_eq!(r.status, 303, "{}", r.body);
    h.clock().advance_minutes(1);
    drain(&h, &app).await;

    for a in &accounts {
        assert_eq!(revision_of(&h, a), 2, "{a:?}");
        assert!(label_of(&h, a).contains(&"ยกเลิกการเชื่อมบัญชี".to_string()));
    }
    let retires: Vec<MenuEvent> = fake
        .menu_events()
        .into_iter()
        .filter(|e| matches!(e, MenuEvent::Retired { .. }))
        .collect();
    assert_eq!(
        retires,
        [MenuEvent::Retired {
            revisions: vec![0, 1],
            live: 2
        }],
        "revision 1 is never retired in favour of itself, and only 2 stays"
    );
    let live = count(
        &h,
        "SELECT count(*) AS n FROM menu_revision WHERE locale = 'th' AND state <> 'retired'",
    )
    .await;
    assert_eq!(live, 1);
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

#[tokio::test]
async fn an_account_unlinked_mid_run_gets_the_locked_menu_back() {
    let (h, app) = boot().await;
    let accounts = linked(&h, 5, "th").await;
    let fake = h.fake_channel();
    fake.set_relink_chunk(2);
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    admin.post(UNLINK, &save("th", "เลิกเชื่อมบัญชี")).await;
    app.deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap();
    assert_eq!(
        revision_of(&h, &accounts[0]),
        1,
        "re-linked in the first chunk"
    );
    // The Student unlinks mid-run; the platform still shows them the
    // unlocked Menu (as a batch moving them would).
    h.clock().advance_minutes(1);
    let now = h.clock().now_ms();
    for sql in [
        format!(
            "UPDATE channel_account SET flow_state = 'awaiting_student_id', updated_at = {now} \
             WHERE id = 'ca-th00'"
        ),
        format!("UPDATE link SET status = 'revoked', revoked_at = {now} WHERE id = 'ln-th00'"),
    ] {
        h.db().execute(Statement::new(sql)).await.unwrap();
    }
    drain(&h, &app).await;
    assert!(
        matches!(fake.menu(&accounts[0]), Some(None)),
        "locked again: {:?}",
        fake.menu(&accounts[0])
    );
    assert_eq!(revision_of(&h, &accounts[4]), 1);
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

#[tokio::test]
async fn a_run_past_the_wait_limit_fails_visibly() {
    let (h, app) = boot().await;
    linked(&h, 2, "th").await;
    let fake = h.fake_channel();
    fake.rate_limit_relinks(1000);
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    admin.post(UNLINK, &save("th", "เลิกเชื่อมบัญชี")).await;
    app.deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap();
    // The continuation has waited once; pretend it is the 200th time.
    let sql = "UPDATE job SET payload = json_set(payload, '$.waits', 200) \
               WHERE kind = 'kernel.menu.relink' AND state = 'queued'";
    h.db().execute(Statement::new(sql)).await.unwrap();
    h.clock().advance_minutes(1);
    let r = app
        .deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap();
    assert_eq!(r.retried, 1, "{r:?}");
    let rows = h
        .db()
        .query(Statement::new(
            "SELECT last_error FROM job WHERE kind = 'kernel.menu.relink' \
             AND last_error IS NOT NULL",
        ))
        .await
        .unwrap();
    let err = rows
        .get(0, "last_error")
        .and_then(|v| v.as_text())
        .unwrap_or_default()
        .to_string();
    assert!(err.contains("after 200 waits"), "{err}");
}
