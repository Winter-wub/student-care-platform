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
