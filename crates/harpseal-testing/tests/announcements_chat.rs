//! Announcements (#25) in chat: acknowledge, stats, the "ข่าวล่าสุด" Menu
//! entry, and what a retract hides.

mod announcements_support;

use announcements_support::*;
use harpseal::Message;
use harpseal::modules::announcements::{self, AckOutcome};

async fn sent_one(w: &World, extra: &str) -> String {
    let b = signed_in(w, "Officer").await;
    let id = create(&b, &form("ประชุม", "ประชุมนักศึกษา", extra)).await;
    drain(w).await;
    id
}

#[tokio::test]
async fn ack_is_idempotent_covers_every_row_and_counts_in_stats() {
    let w = boot().await;
    roster(&w).await;
    let id = sent_one(&w, "&require_ack=1&do=confirm").await;
    let action = format!("announcements.ack.{id}");
    let st = announcements::stats(w.app.deployment(), &id).await.unwrap();
    assert_eq!((st.acked, st.in_audience), (0, 3));

    let first = press(&w, "fake", "s1", &action).await;
    assert_eq!(text(&first[0]), "บันทึกว่ารับทราบแล้ว ขอบคุณครับ");
    // Both of s1's rows (LINE-like and Telegram-like) are acknowledged.
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM announcement_delivery WHERE student_id = 's1' AND acked_at IS NOT NULL"
        )
        .await,
        2
    );
    // A second tap, from the other Channel: already done.
    let again = press(&w, TG, "s1", &action).await;
    assert_eq!(text(&again[0]), "รับทราบแล้ว");
    let st = announcements::stats(w.app.deployment(), &id).await.unwrap();
    assert_eq!((st.acked, st.in_audience), (1, 3));
    let fake_round = st.rounds.iter().find(|r| r.channel == "fake").unwrap();
    assert_eq!(
        (fake_round.sent, fake_round.failed, fake_round.skipped),
        (2, 0, 0)
    );

    // The service function the Mini App calls says the same.
    let d = w.app.deployment();
    assert_eq!(
        announcements::ack(d, &id, "s1").await.unwrap(),
        AckOutcome::Already
    );
    assert_eq!(
        announcements::ack(d, &id, "s3").await.unwrap(),
        AckOutcome::Recorded
    );
    assert_eq!(
        announcements::ack(d, &id, "s2").await.unwrap(),
        AckOutcome::Unknown
    );
    // A malformed suffix is answered politely.
    let odd = press(&w, "fake", "s3", "announcements.ack.nope").await;
    assert_eq!(text(&odd[0]), "ไม่พบข่าวนี้");
    // The ack Action id fits Telegram's 64 bytes.
    assert!(announcements::ack_action(&id).unwrap().as_str().len() <= 64);
}

/// Seeds a sent announcement for s1 on `fake`: `at` orders them.
async fn seeded(w: &World, id: &str, at: i64, ack: bool, extra: &str) {
    run(
        &w.h,
        &format!(
            "INSERT INTO announcement (id, status, require_ack, created_at, updated_at, sent_at{}) \
             VALUES ('{id}', 'sent', {}, {at}, {at}, {at}{})",
            if extra.is_empty() { "" } else { ", expires_at" },
            i64::from(ack),
            if extra.is_empty() {
                String::new()
            } else {
                format!(", {extra}")
            },
        ),
    )
    .await;
    run(
        &w.h,
        &format!(
            "INSERT INTO announcement_text (announcement_id, locale, title, body) VALUES \
             ('{id}', 'th', 'ข่าว {id}', 'รายละเอียด {id}')"
        ),
    )
    .await;
    run(
        &w.h,
        &format!(
            "INSERT INTO announcement_delivery (id, announcement_id, round, student_id, link_id, \
             channel, channel_account_id, status) VALUES ('d-{id}', '{id}', 0, 's1', \
             'l-fake-s1', 'fake', 'a-fake-s1', 'sent')"
        ),
    )
    .await;
}

fn titles(m: &Message) -> Vec<String> {
    match m {
        Message::CardList(l) => l.0.iter().map(|c| c.title.clone()).collect(),
        other => vec![format!("{other:?}")],
    }
}

#[tokio::test]
async fn latest_lists_five_newest_unacknowledged_first_without_expired_or_retracted() {
    let w = boot().await;
    roster(&w).await;
    let empty = press(&w, "fake", "s1", "announcements.latest").await;
    assert_eq!(text(&empty[0]), "ยังไม่มีข่าวสำหรับคุณ");

    let now = w.h.clock().now_ms();
    for (i, ack) in [
        (1, false),
        (2, true),
        (3, false),
        (4, true),
        (5, false),
        (6, false),
    ] {
        seeded(&w, &format!("n{i}"), now - 10_000 + i * 100, ack, "").await;
    }
    seeded(&w, "expired", now, false, &format!("{}", now - 1)).await;
    seeded(&w, "gone", now, false, "").await;
    run(
        &w.h,
        "UPDATE announcement SET status = 'retracted' WHERE id = 'gone'",
    )
    .await;
    // n4 was acknowledged already.
    run(
        &w.h,
        "UPDATE announcement_delivery SET acked_at = 1 WHERE announcement_id = 'n4'",
    )
    .await;

    let seen = press(&w, "fake", "s1", "announcements.latest").await;
    // The 5 newest are n6..n2; n2 (ack wanted, not given) first.
    assert_eq!(
        titles(&seen[0]),
        ["ข่าว n2", "ข่าว n6", "ข่าว n5", "ข่าว n4", "ข่าว n3"]
    );
    let Message::CardList(list) = &seen[0] else {
        unreachable!()
    };
    let labels = |i: usize| -> Vec<String> {
        list.0[i]
            .buttons
            .iter()
            .map(|b| b.label().to_string())
            .collect()
    };
    assert_eq!(labels(0), ["รับทราบ", "เปิดดูรายละเอียด"]);
    assert_eq!(labels(3), ["เปิดดูรายละเอียด"], "n4 is acknowledged");
    assert_eq!(labels(4), ["เปิดดูรายละเอียด", "ดูทั้งหมด"]);
}

#[tokio::test]
async fn retract_hides_the_announcement_and_late_acks_are_told() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("ข่าวผิด", "ขออภัย", "&require_ack=1&do=confirm")).await;
    drain(&w).await;
    let listed = press(&w, "fake", "s1", "announcements.latest").await;
    assert_eq!(titles(&listed[0]), ["ข่าวผิด"]);

    b.get(&format!("/admin/announcements/{id}")).await;
    let v = version(&w, &id).await;
    let path = format!("/admin/announcements/{id}/retract");
    let ask = b.post(&path, &format!("version={v}")).await;
    assert_eq!(ask.status, 200, "a confirmation page first");
    assert_eq!(texts(&w, "SELECT status FROM announcement").await, ["sent"]);
    let r = b.post(&path, &format!("version={v}&confirm=yes")).await;
    assert_eq!(r.status, 303);
    drain(&w).await;

    let hidden = press(&w, "fake", "s1", "announcements.latest").await;
    assert_eq!(text(&hidden[0]), "ยังไม่มีข่าวสำหรับคุณ");
    let d = w.app.deployment();
    let th = harpseal::Locale::th();
    assert!(
        announcements::latest(d, "s1", &th, 20)
            .await
            .unwrap()
            .is_empty()
    );
    let late = press(&w, TG, "s1", &format!("announcements.ack.{id}")).await;
    assert_eq!(text(&late[0]), "ข่าวนี้ถูกถอนแล้ว");
    // Deleted where the Channel can delete (Telegram-like), not on LINE-like.
    assert_eq!(w.tg.deleted().len(), 2);
    assert!(w.line.deleted().is_empty());
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM announcement_delivery WHERE removal = 'deleted'"
        )
        .await,
        2
    );
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM audit_log WHERE action = 'announcement.retract'"
        )
        .await,
        1
    );
}
