//! The retire side of the Menu re-link (#66), at the Kernel seam: the
//! Fake sees the re-link then the retire; a save mid-run supersedes it;
//! accounts unlinked mid-run are locked again; a run past the wait limit
//! fails visibly.

mod admin_support;
mod menu_support;
mod support;

use admin_support::{boot, signed_in};
use harpseal::{ChannelAccountId, Statement, Timestamp};
use harpseal_channel_fake::{FakeChannel, MenuEvent};
use harpseal_testing::urlencode;
use menu_support::*;
use support::Chat;

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

#[tokio::test]
async fn the_relock_pages_through_unlinks_and_leaves_onboarding_alone() {
    let (h, app) = boot().await;
    let accounts = linked(&h, 7, "th").await;
    let fake = h.fake_channel();
    fake.set_relink_chunk(2);
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    admin.post(UNLINK, &save("th", "เลิกเชื่อมบัญชี")).await;
    app.deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap();
    // Mid-run: five Students unlink; three new followers start onboarding
    // (their ids sort first, so a first-N selection would take them).
    h.clock().advance_minutes(1);
    let now = h.clock().now_ms();
    let mut sql = Vec::new();
    for i in 0..5 {
        sql.push(format!(
            "UPDATE channel_account SET flow_state = 'awaiting_student_id', updated_at = {now} \
             WHERE id = 'ca-th0{i}'"
        ));
        sql.push(format!(
            "UPDATE link SET status = 'revoked', revoked_at = {now} WHERE id = 'ln-th0{i}'"
        ));
    }
    for i in 0..3 {
        sql.push(format!(
            "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
             updated_at) VALUES ('ca-a{i}', 'fake', 'new-{i}', {now}, 'awaiting_consent', {now})"
        ));
    }
    for q in sql {
        h.db().execute(Statement::new(q)).await.unwrap();
    }
    drain(&h, &app).await;

    for (i, a) in accounts.iter().enumerate().take(5) {
        assert!(matches!(fake.menu(a), Some(None)), "unlinked {i}: locked");
    }
    for i in 0..3 {
        let a = FakeChannel::account(format!("new-{i}"));
        assert!(fake.menu(&a).is_none(), "onboarding {i}: untouched");
    }
    assert_eq!(revision_of(&h, &accounts[6]), 1);
    let pages = count(
        &h,
        "SELECT count(*) AS n FROM job WHERE kind = 'kernel.menu.relink' \
         AND json_extract(payload, '$.phase') = 'relock'",
    )
    .await;
    assert_eq!(pages, 2, "5 accounts, pages of 2: two continuations");
}

#[tokio::test]
async fn a_relock_page_stays_within_its_budget_and_retries_a_failed_account() {
    let (h, app) = boot().await;
    let accounts = linked(&h, 25, "th").await;
    let fake = h.fake_channel();
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    admin.post(UNLINK, &save("th", "เลิกเชื่อมบัญชี")).await;
    // All 25 unlink as the run starts (a new Consent notice would do the
    // same), and the first lock call is rate limited.
    let now = h.clock().now_ms();
    for i in 0..25 {
        for q in [
            format!(
                "UPDATE channel_account SET flow_state = 'awaiting_student_id', \
                 updated_at = {now} WHERE id = 'ca-th{i:02}'"
            ),
            format!(
                "UPDATE link SET status = 'revoked', revoked_at = {now} WHERE id = 'ln-th{i:02}'"
            ),
        ] {
            h.db().execute(Statement::new(q)).await.unwrap();
        }
    }
    fake.rate_limit_menus(1);
    drain(&h, &app).await;

    for (i, a) in accounts.iter().enumerate() {
        assert!(matches!(fake.menu(a), Some(None)), "account {i}: locked");
    }
    let pages = count(
        &h,
        "SELECT count(*) AS n FROM job WHERE kind = 'kernel.menu.relink' \
         AND json_extract(payload, '$.phase') = 'relock'",
    )
    .await;
    // The failed first account waits and is tried again (1), then pages of
    // 20 (the Fake's chunk is 100): 20 + 5 (1).
    assert_eq!(pages, 2);
}
