//! Announcements (#25) review round: stale writes enqueue nothing, acks
//! survive a resend, stuck rounds finish, blocked accounts are skipped,
//! edits touch only changed Locales, and the edges of the Jobs.

mod announcements_support;

use announcements_support::*;
use harpseal::{Message, Timestamp};

fn has_ack(m: &Message) -> bool {
    m.buttons().iter().any(|b| b.action_id().is_some())
}

async fn tick_once(w: &World) {
    let now = Timestamp(w.h.clock().now_ms());
    let r = w.app.deployment().tick(now).await.unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
}

#[tokio::test]
async fn a_stale_retract_deletes_nothing_and_a_later_retract_still_does() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("ข่าว", "เดิม", "&do=confirm")).await;
    drain(&w).await;
    let stale = version(&w, &id).await;
    // Someone else edits meanwhile (version + 1).
    let edit = format!("/admin/announcements/{id}/edit");
    b.get(&edit).await;
    let r = b
        .post(
            &edit,
            &format!("{}&version={stale}", form("ข่าว", "ใหม่", "")),
        )
        .await;
    assert_eq!(r.status, 303);
    let retract = format!("/admin/announcements/{id}/retract");
    let r = b
        .post(&retract, &format!("version={stale}&confirm=yes"))
        .await;
    assert_eq!(r.status, 303);
    drain(&w).await;
    assert!(
        w.tg.deleted().is_empty(),
        "the lost retract enqueued nothing"
    );
    assert_eq!(texts(&w, "SELECT status FROM announcement").await, ["sent"]);
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM job WHERE kind = 'announcements.retract'"
        )
        .await,
        0
    );
    let v = version(&w, &id).await;
    let r = b.post(&retract, &format!("version={v}&confirm=yes")).await;
    assert_eq!(r.status, 303);
    drain(&w).await;
    assert_eq!(w.tg.deleted().len(), 2);
    assert_eq!(
        texts(&w, "SELECT status FROM announcement").await,
        ["retracted"]
    );
}

#[tokio::test]
async fn an_acknowledgement_survives_a_resend() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("ประชุม", "อย่าลืม", "&require_ack=1&do=confirm")).await;
    drain(&w).await;
    let ack = format!("announcements.ack.{id}");
    press(&w, "fake", "s1", &ack).await;
    w.tg.take_sent();
    w.line.take_sent();
    let v = version(&w, &id).await;
    let r = resend_confirmed(&b, &id, v).await;
    assert_eq!(r.status, 303);
    drain(&w).await;
    // s1's resent Card has no [รับทราบ]; s3 (not acknowledged) keeps it.
    let s1 = w.tg.sent_to(&fake_account(TG, "s1"));
    assert_eq!(s1.len(), 1);
    assert!(!has_ack(&s1[0]));
    let s3 = w.line.sent_to(&fake_account("fake", "s3"));
    assert!(has_ack(&s3[0]));
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM announcement_delivery WHERE student_id = 's1' AND round = 1 \
             AND acked_at IS NOT NULL"
        )
        .await,
        2
    );
    let again = press(&w, TG, "s1", &ack).await;
    assert_eq!(text(&again[0]), "รับทราบแล้ว");
    let st = harpseal::modules::announcements::stats(w.app.deployment(), &id)
        .await
        .unwrap();
    assert_eq!((st.acked, st.in_audience), (1, 3));
}

#[tokio::test]
async fn a_round_whose_jobs_all_ended_is_finished_when_the_page_loads() {
    let w = boot_with(1).await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    w.line.fail_next_batch_at(0);
    let id = create(&b, &form("ข่าว", "รายละเอียด", "&do=confirm")).await;
    drain(&w).await;
    assert_eq!(
        texts(&w, "SELECT status FROM announcement").await,
        ["sending"]
    );
    // The retry never comes back (its lock expired on the last attempt).
    run(
        &w.h,
        "UPDATE job SET state = 'failed' WHERE state = 'queued'",
    )
    .await;
    let page = b.get(&format!("/admin/announcements/{id}")).await;
    assert_eq!(page.status, 200);
    assert_eq!(texts(&w, "SELECT status FROM announcement").await, ["sent"]);
    assert_eq!(
        texts(
            &w,
            "SELECT error FROM announcement_delivery WHERE status = 'failed'"
        )
        .await,
        ["ran out of attempts"]
    );

    // A parent that failed every attempt: the round ends with nothing sent.
    let id2 = create(&b, &form("ข่าว 2", "รายละเอียด", "&do=confirm")).await;
    run(
        &w.h,
        "UPDATE job SET state = 'failed' WHERE kind = 'announcements.deliver' AND state = 'queued'",
    )
    .await;
    b.get(&format!("/admin/announcements/{id2}")).await;
    assert_eq!(
        texts(
            &w,
            &format!("SELECT status FROM announcement WHERE id = '{id2}'")
        )
        .await,
        ["sent"]
    );
}

#[tokio::test]
async fn a_student_who_blocked_after_the_snapshot_is_skipped_not_multicast() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    create(&b, &form("ข่าว", "รายละเอียด", "&do=confirm")).await;
    tick_once(&w).await; // the parent: rows snapshotted, children queued
    run(
        &w.h,
        "UPDATE channel_account SET blocked_at = 1 WHERE id = 'a-fake-s3'",
    )
    .await;
    drain(&w).await;
    assert_eq!(
        texts(
            &w,
            "SELECT status || ':' || error FROM announcement_delivery WHERE student_id = 's3'"
        )
        .await,
        ["skipped:unreachable"]
    );
    let calls = w.line.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].recipients, 1, "s1 only");
    assert_eq!(
        count(&w, "SELECT metered FROM usage_daily WHERE channel = 'fake'").await,
        1
    );
}

#[tokio::test]
async fn an_edit_touches_only_cards_whose_text_changed() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let both = "&title_en=News&body_en=Text";
    let id = create(&b, &form("ข่าว", "เดิม", &format!("{both}&do=confirm"))).await;
    drain(&w).await;
    let edit = format!("/admin/announcements/{id}/edit");
    // Expiry only: no Job, no edit.
    b.get(&edit).await;
    let v = version(&w, &id).await;
    let f = form(
        "ข่าว",
        "เดิม",
        &format!("{both}&expires=2026-12-31&version={v}"),
    );
    assert_eq!(b.post(&edit, &f).await.status, 303);
    drain(&w).await;
    assert!(w.tg.edited().is_empty());
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM job WHERE kind = 'announcements.edit'"
        )
        .await,
        0
    );
    // English only: s4 (en) on the Telegram-like Channel, not s1 (th).
    let v = version(&w, &id).await;
    let f = form(
        "ข่าว",
        "เดิม",
        &format!("&title_en=News&body_en=Changed&version={v}"),
    );
    assert_eq!(b.post(&edit, &f).await.status, 303);
    drain(&w).await;
    let edited = w.tg.edited();
    assert_eq!(edited.len(), 1);
    let s4 = w.tg.sent_to(&fake_account(TG, "s4"));
    assert!(edited.iter().all(|(pid, _)| {
        w.tg.sent()
            .iter()
            .any(|s| &s.id == pid && s.to == fake_account(TG, "s4"))
    }));
    assert_eq!(s4.len(), 1);
}

#[tokio::test]
async fn the_last_attempt_marks_rows_failed_with_the_error_kind() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    create(&b, &form("ข่าว", "รายละเอียด", "&do=confirm")).await;
    tick_once(&w).await;
    // No retries left: the first run of each child is its last.
    run(
        &w.h,
        "UPDATE job SET max_attempts = 0 WHERE kind = 'announcements.deliver.channel'",
    )
    .await;
    w.line.fail_next_batch_at(0);
    drain(&w).await;
    assert_eq!(
        texts(
            &w,
            "SELECT DISTINCT status || ':' || error FROM announcement_delivery WHERE channel = 'fake'"
        )
        .await,
        ["failed:rate_limited"]
    );
    assert_eq!(texts(&w, "SELECT status FROM announcement").await, ["sent"]);
}

#[tokio::test]
async fn cards_older_than_48_hours_are_too_old_to_delete() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("ข่าว", "รายละเอียด", "&do=confirm")).await;
    drain(&w).await;
    w.h.clock().advance_minutes(49 * 60);
    // The Staff session ended meanwhile.
    b.login("officer@sealbay.example", PASSWORD).await;
    b.get(&format!("/admin/announcements/{id}")).await;
    let v = version(&w, &id).await;
    let r = b
        .post(
            &format!("/admin/announcements/{id}/retract"),
            &format!("version={v}&confirm=yes"),
        )
        .await;
    assert_eq!(r.status, 303);
    drain(&w).await;
    assert_eq!(
        texts(&w, "SELECT status FROM announcement").await,
        ["retracted"]
    );
    assert!(w.tg.deleted().is_empty());
    assert_eq!(
        texts(
            &w,
            "SELECT DISTINCT removal FROM announcement_delivery WHERE channel = 'tg'"
        )
        .await,
        ["too_old"]
    );
}

#[tokio::test]
async fn an_expired_announcement_refuses_an_ack() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create(
        &b,
        &form(
            "ข่าว",
            "รายละเอียด",
            "&require_ack=1&expires=2026-11-05&do=confirm",
        ),
    )
    .await;
    drain(&w).await;
    w.h.clock().advance_minutes(2 * 24 * 60);
    let listed = press(&w, "fake", "s1", "announcements.latest").await;
    assert_eq!(text(&listed[0]), "ยังไม่มีข่าวสำหรับคุณ", "hidden once expired");
    // #77: once expired, closed to Students; nothing is recorded.
    let ack = press(&w, "fake", "s1", &format!("announcements.ack.{id}")).await;
    assert_eq!(text(&ack[0]), "ข่าวนี้ไม่เปิดให้ดูแล้ว");
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM announcement_delivery WHERE acked_at IS NOT NULL"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn a_viewer_cannot_edit_confirm_cancel_or_count() {
    let w = boot().await;
    roster(&w).await;
    let officer = signed_in(&w, "Officer").await;
    let id = create(&officer, &form("ร่าง", "รายละเอียด", "&do=draft")).await;
    let viewer = signed_in(&w, "Viewer").await;
    let base = format!("/admin/announcements/{id}");
    assert_eq!(viewer.get(&format!("{base}/edit")).await.status, 403);
    for path in [
        format!("{base}/edit"),
        format!("{base}/confirm"),
        format!("{base}/cancel"),
        "/admin/announcements/audience".to_string(),
    ] {
        assert_eq!(viewer.post(&path, "version=0").await.status, 403, "{path}");
    }
    assert_eq!(
        texts(&w, "SELECT status FROM announcement").await,
        ["draft"]
    );
}
