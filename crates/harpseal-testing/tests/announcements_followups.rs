//! Announcements follow-ups (#49): edit and retract in paced chunks that
//! resume after a rate limit, no Card left behind by a send that raced a
//! retract, and an ack that reveals nothing to a Student outside the
//! Audience.

mod announcements_support;

use std::collections::BTreeSet;

use announcements_support::*;
use harpseal::Message;
use harpseal::modules::announcements::{self, AckOutcome};

/// `n` Thai Students linked on the Telegram-like Channel only.
async fn telegram_students(w: &World, n: usize) {
    for i in 0..n {
        let id = format!("t{i:02}");
        student(&w.h, &id, "th", "active", r#"{"programme":"MSC","year":2}"#).await;
        link(&w.h, &id, TG, "active").await;
    }
}

/// Moves the clock and runs every Job then due.
async fn later(w: &World, minutes: i64) {
    w.h.clock().advance_minutes(minutes);
    drain(w).await;
}

#[tokio::test]
async fn a_rate_limit_in_the_middle_of_an_edit_resumes_on_the_next_tick() {
    let w = boot().await;
    telegram_students(&w, 30).await;
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("เดิม", "ข้อความเดิม", "&do=confirm")).await;
    drain(&w).await;
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM announcement_delivery WHERE status = 'sent'"
        )
        .await,
        30
    );
    // The 11th edit is refused with a 429.
    w.tg.fail_next_maintenance_at(10);
    let edit = format!("/admin/announcements/{id}/edit");
    b.get(&edit).await;
    let v = version(&w, &id).await;
    let r = b
        .post(
            &edit,
            &format!("{}&version={v}", form("ใหม่", "ข้อความใหม่", "")),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    drain(&w).await;
    // Two chunks (25 + 5): the first stopped after 10, the second ran.
    assert_eq!(w.tg.edited().len(), 15);
    assert_eq!(
        texts(&w, "SELECT edit_report ->> 'edited' FROM announcement").await,
        ["15"]
    );
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM job WHERE kind = 'announcements.edit.chunk' \
             AND state = 'queued' AND unique_key LIKE '%:tg:0:1'"
        )
        .await,
        1,
        "the continuation waits for retry_after"
    );
    later(&w, 1).await;
    let edited = w.tg.edited();
    let ids: BTreeSet<_> = edited.iter().map(|(pid, _)| pid.0.clone()).collect();
    assert_eq!((edited.len(), ids.len()), (30, 30), "each Card edited once");
    assert!(
        edited
            .iter()
            .all(|(_, m)| matches!(m, Message::Card(c) if c.title == "ใหม่"))
    );
    assert_eq!(
        texts(
            &w,
            "SELECT (edit_report ->> 'edited') || '/' || (edit_report ->> 'failed') \
             FROM announcement"
        )
        .await,
        ["30/0"]
    );
}

#[tokio::test]
async fn a_rate_limit_in_the_middle_of_a_retract_resumes_where_it_stopped() {
    let w = boot().await;
    telegram_students(&w, 30).await;
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("ข่าว", "ผิด", "&do=confirm")).await;
    drain(&w).await;
    w.tg.fail_next_maintenance_at(3);
    b.get(&format!("/admin/announcements/{id}")).await;
    let v = version(&w, &id).await;
    let path = format!("/admin/announcements/{id}/retract");
    let r = b.post(&path, &format!("version={v}&confirm=yes")).await;
    assert_eq!(r.status, 303);
    drain(&w).await;
    assert_eq!(
        w.tg.deleted().len(),
        8,
        "3 of the first chunk, all 5 of the second"
    );
    later(&w, 1).await;
    let deleted = w.tg.deleted();
    let ids: BTreeSet<_> = deleted.iter().map(|p| p.0.clone()).collect();
    assert_eq!((deleted.len(), ids.len()), (30, 30));
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM announcement_delivery WHERE removal = 'deleted'"
        )
        .await,
        30
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_send_racing_a_retract_stops_and_the_late_pass_deletes_what_went_out() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let english = "&title_en=News&body_en=Wrong&do=confirm";
    let id = create(&b, &form("ข่าว", "ผิด", english)).await;
    // The Telegram-like child has two Locale groups (s4 en, then s1 th).
    // The retract lands while the first group is being sent.
    let db = w.h.db();
    let now = w.h.clock().now_ms();
    let retract_sql = format!(
        "UPDATE announcement SET status = 'retracted', retracted_at = {now}, \
         version = version + 1 WHERE id = '{id}'"
    );
    w.tg.before_next_send(move || {
        let st = harpseal::Statement::new(retract_sql);
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(db.execute(st))
                .unwrap();
        });
    });
    drain(&w).await;
    let s1 = fake_account(TG, "s1");
    let s4 = fake_account(TG, "s4");
    assert_eq!(w.tg.sent_to(&s4).len(), 1, "the group already on its way");
    assert!(
        w.tg.sent_to(&s1).is_empty(),
        "the next group re-read the status"
    );
    assert_eq!(
        texts(
            &w,
            "SELECT status FROM announcement_delivery WHERE channel = 'tg' ORDER BY student_id"
        )
        .await,
        ["pending", "sent"]
    );
    // The retract Job's first pass runs; a child of the round that wrote
    // its outcome only after that pass read the rows is simulated by
    // hiding s4's row from it.
    run(
        &w.h,
        "UPDATE announcement_delivery SET status = 'pending' WHERE channel = 'tg' \
         AND platform_message_id IS NOT NULL",
    )
    .await;
    let v = version(&w, &id).await;
    w.app
        .deployment()
        .enqueue_unique(
            &format!("announcements:{id}:retract:v{v}"),
            "announcements.retract",
            serde_json::json!({ "id": id, "version": v }),
            None,
        )
        .await
        .unwrap();
    drain(&w).await;
    assert!(
        w.tg.deleted().is_empty(),
        "nothing sent when the first pass ran"
    );
    run(
        &w.h,
        "UPDATE announcement_delivery SET status = 'sent' WHERE channel = 'tg' \
         AND platform_message_id IS NOT NULL",
    )
    .await;
    later(&w, 15).await;
    assert!(
        w.tg.deleted().is_empty(),
        "the late pass waits past the Job lock"
    );
    later(&w, 1).await;
    assert_eq!(w.tg.deleted().len(), 1, "the late pass deleted s4's Card");
    assert_eq!(
        texts(
            &w,
            "SELECT removal FROM announcement_delivery WHERE platform_message_id IS NOT NULL \
             AND channel = 'tg'"
        )
        .await,
        ["deleted"]
    );
}

#[tokio::test]
async fn an_ack_reveals_no_retracted_announcement_outside_the_audience() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("ข่าว", "ลับ", "&require_ack=1&do=confirm")).await;
    drain(&w).await;
    b.get(&format!("/admin/announcements/{id}")).await;
    let v = version(&w, &id).await;
    let path = format!("/admin/announcements/{id}/retract");
    assert_eq!(
        b.post(&path, &format!("version={v}&confirm=yes"))
            .await
            .status,
        303
    );
    drain(&w).await;
    let d = w.app.deployment();
    // s2 left before the send: no delivery row.
    assert_eq!(
        announcements::ack(d, &id, "s2").await.unwrap(),
        AckOutcome::Unknown
    );
    assert_eq!(
        announcements::ack(d, &id, "s1").await.unwrap(),
        AckOutcome::Retracted
    );
    let crafted = press(&w, "fake", "s2", &format!("announcements.ack.{id}")).await;
    let unknown_id = "00000000-0000-7000-8000-000000000000";
    let made_up = press(&w, "fake", "s2", &format!("announcements.ack.{unknown_id}")).await;
    assert_eq!(text(&crafted[0]), text(&made_up[0]), "like an unknown id");
    let member = press(&w, "fake", "s1", &format!("announcements.ack.{id}")).await;
    assert_eq!(text(&member[0]), "ข่าวนี้ถูกถอนแล้ว");
}
