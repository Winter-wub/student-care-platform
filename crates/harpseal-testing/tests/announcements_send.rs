//! Announcements (#25) at the Deployment seam: sending through the Jobs to
//! a LineLike Channel with multicast and a TelegramLike one without.

mod announcements_support;

use announcements_support::*;
use harpseal::{Message, Metering};

fn card_title(m: &Message) -> String {
    match m {
        Message::Card(c) => c.title.clone(),
        other => format!("{other:?}"),
    }
}

#[tokio::test]
async fn send_now_reaches_three_students_on_two_channels() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let f = form(
        "ปิดคลินิก",
        "คลินิกปิดวันศุกร์",
        "&title_en=Clinic+closed&body_en=Closed+on+Friday&require_ack=1&do=confirm",
    );
    let id = create(&b, &f).await;
    assert_eq!(
        texts(&w, "SELECT status FROM announcement").await,
        ["sending"]
    );
    drain(&w).await;

    // One row per live Link of an active Student: s1 twice, s3, s4; never s2 (left).
    let rows = texts(
        &w,
        "SELECT student_id || ':' || channel || ':' || status FROM announcement_delivery \
         ORDER BY student_id, channel",
    )
    .await;
    assert_eq!(
        rows,
        ["s1:fake:sent", "s1:tg:sent", "s3:fake:sent", "s4:tg:sent"]
    );
    assert_eq!(texts(&w, "SELECT status FROM announcement").await, ["sent"]);

    // LineLike: one multicast request to 2 recipients, 2 metered units.
    let calls = w.line.calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].recipients, 2);
    assert_eq!(calls[0].metering, Metering::Metered);
    // TelegramLike: one Push per account, free.
    let tg_calls = w.tg.calls();
    assert_eq!(tg_calls.len(), 2);
    assert!(
        tg_calls
            .iter()
            .all(|c| c.recipients == 1 && c.metering == Metering::Free)
    );

    // Each Student sees the Card in their Locale, with both buttons.
    let s4 = w.tg.sent_to(&fake_account(TG, "s4"));
    assert_eq!(s4.len(), 1);
    assert_eq!(card_title(&s4[0]), "Clinic closed");
    let s3 = w.line.sent_to(&fake_account("fake", "s3"));
    assert_eq!(card_title(&s3[0]), "ปิดคลินิก");
    let ack = format!("announcements.ack.{id}");
    let ids: Vec<String> = s3[0]
        .buttons()
        .iter()
        .map(|b| b.label().to_string())
        .collect();
    assert_eq!(ids, ["รับทราบ", "เปิดดูรายละเอียด"]);
    assert_eq!(s3[0].buttons()[0].action_id().unwrap().as_str(), ack);
    assert!(ack.len() <= harpseal::ActionId::MAX_BYTES);

    // The Kernel recorded one outbound row per recipient, usage per Channel.
    assert_eq!(
        texts(
            &w,
            "SELECT channel || ':' || count(*) || ':' || sum(metering = 'metered') FROM \
             outbound_message WHERE module = 'announcements' GROUP BY channel ORDER BY channel"
        )
        .await,
        ["fake:2:2", "tg:2:0"]
    );
    assert_eq!(
        texts(
            &w,
            "SELECT channel || ':' || metered || ':' || free FROM usage_daily ORDER BY channel"
        )
        .await,
        ["fake:2:0", "tg:0:2"]
    );
    // Audit of the send.
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM audit_log WHERE action = 'announcement.send'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn an_unreachable_link_is_skipped_and_never_sent() {
    let w = boot().await;
    roster(&w).await;
    run(
        &w.h,
        "UPDATE link SET status = 'unreachable' WHERE id = 'l-fake-s3'",
    )
    .await;
    let b = signed_in(&w, "Officer").await;
    create(&b, &form("ข่าว", "รายละเอียด", "&unreachable=1&do=confirm")).await;
    drain(&w).await;
    assert_eq!(
        texts(
            &w,
            "SELECT status || ':' || error FROM announcement_delivery WHERE student_id = 's3'"
        )
        .await,
        ["skipped:unreachable"]
    );
    assert!(w.line.sent_to(&fake_account("fake", "s3")).is_empty());
    // Without the option the unreachable Link is not even counted.
    create(&b, &form("ข่าว 2", "รายละเอียด", "&do=confirm")).await;
    drain(&w).await;
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM announcement_delivery WHERE student_id = 's3'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn a_scheduled_send_fires_only_when_the_clock_passes_send_at() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    // START is 2026-11-05 07:00 in +07:00; schedule for 09:30.
    let id = create(
        &b,
        &form(
            "นัดหมาย",
            "พบอาจารย์",
            "&when=later&send_at=2026-11-05T09%3A30&do=confirm",
        ),
    )
    .await;
    assert_eq!(
        texts(&w, "SELECT status FROM announcement").await,
        ["scheduled"]
    );
    w.h.clock().advance_minutes(149);
    drain(&w).await;
    assert!(
        w.line.sent().is_empty() && w.tg.sent().is_empty(),
        "too early"
    );
    assert_eq!(
        count(&w, "SELECT count(*) FROM announcement_delivery").await,
        0
    );
    w.h.clock().advance_minutes(1);
    drain(&w).await;
    assert_eq!(texts(&w, "SELECT status FROM announcement").await, ["sent"]);
    assert_eq!(w.line.sent().len() + w.tg.sent().len(), 4);
    let _ = id;
}

#[tokio::test]
async fn a_cancelled_schedule_never_fires_and_can_be_confirmed_again() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create(
        &b,
        &form(
            "นัดหมาย",
            "พบอาจารย์",
            "&when=later&send_at=2026-11-05T09%3A30&do=confirm",
        ),
    )
    .await;
    let v = version(&w, &id).await;
    let r = b
        .post(
            &format!("/admin/announcements/{id}/cancel"),
            &format!("version={v}"),
        )
        .await;
    assert_eq!(r.status, 303);
    assert_eq!(
        texts(&w, "SELECT status FROM announcement").await,
        ["draft"]
    );
    w.h.clock().advance_minutes(200);
    drain(&w).await;
    assert!(w.line.sent().is_empty(), "the cancelled Job is gone");
    // Confirming again (send_at now past) sends at once with a new Job key.
    b.get(&format!("/admin/announcements/{id}")).await;
    let v = version(&w, &id).await;
    let r = b
        .post(
            &format!("/admin/announcements/{id}/confirm"),
            &format!("version={v}"),
        )
        .await;
    assert_eq!(r.status, 303);
    drain(&w).await;
    assert_eq!(texts(&w, "SELECT status FROM announcement").await, ["sent"]);
}

#[tokio::test]
async fn a_failed_multicast_group_stays_pending_and_its_retry_reuses_the_key() {
    // Groups of 1: one child Job per recipient on the LineLike Channel.
    let w = boot_with(1).await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    w.line.fail_next_batch_at(0);
    create(&b, &form("ข่าว", "รายละเอียด", "&do=confirm")).await;
    drain(&w).await;
    // The group that ran first failed and waits; the other went out.
    let fake_rows = "SELECT student_id || ':' || status FROM announcement_delivery \
                     WHERE channel = 'fake' ORDER BY student_id";
    let mut first = texts(&w, fake_rows).await;
    first.sort_by_key(|r| r.ends_with("sent"));
    assert!(
        first[0].ends_with(":pending") && first[1].ends_with(":sent"),
        "{first:?}"
    );
    assert_eq!(
        texts(&w, "SELECT status FROM announcement").await,
        ["sending"]
    );
    w.h.clock().advance_minutes(1);
    drain(&w).await;
    assert_eq!(texts(&w, fake_rows).await, ["s1:sent", "s3:sent"]);
    assert_eq!(w.line.sent_to(&fake_account("fake", "s1")).len(), 1);
    assert_eq!(w.line.sent_to(&fake_account("fake", "s3")).len(), 1);
    // The retry reused the failed call's key (LINE drops a repeat).
    let keys = w.line.keys();
    assert_eq!(keys.len(), 3);
    assert_eq!(keys[0], keys[2]);
    assert_ne!(keys[0], keys[1]);
    assert_eq!(texts(&w, "SELECT status FROM announcement").await, ["sent"]);
}

#[tokio::test]
async fn send_many_stopped_at_the_second_group_resumes_with_the_rest() {
    let w = boot_with(1).await;
    let d = w.app.deployment();
    let to: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|x| fake_account("fake", x))
        .collect();
    let msgs = [Message::text("ข่าว")];
    w.line.fail_next_batch_at(1);
    let th = harpseal::Locale::th();
    let first = d
        .send_many_with_key("fake", &to, &msgs, &th, Some("announcements"), "k1")
        .await
        .unwrap();
    assert_eq!(first.results.len(), 1, "only the first group went out");
    assert!(matches!(
        first.stopped,
        Some(harpseal::Error::RateLimited { .. })
    ));
    let rest = &to[first.results.len()..];
    let second = d
        .send_many_with_key("fake", rest, &msgs, &th, Some("announcements"), "k1")
        .await
        .unwrap();
    assert_eq!(second.results.len(), 2);
    assert!(second.stopped.is_none());
    for a in &to {
        assert_eq!(w.line.sent_to(a).len(), 1, "{a:?} once");
    }
    assert_eq!(w.line.keys(), ["k1", "k1"]);
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM outbound_message WHERE module = 'announcements'"
        )
        .await,
        3
    );
    assert_eq!(count(&w, "SELECT metered FROM usage_daily").await, 3);
}

#[tokio::test]
async fn thai_is_the_fallback_for_a_student_in_english() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    create(&b, &form("ไทยเท่านั้น", "รายละเอียด", "&do=confirm")).await;
    drain(&w).await;
    let s4 = w.tg.sent_to(&fake_account(TG, "s4"));
    assert_eq!(card_title(&s4[0]), "ไทยเท่านั้น");
}

#[tokio::test]
async fn the_audience_filters_who_is_sent_to() {
    let w = boot().await;
    roster(&w).await;
    tag(&w.h, "s4", "clinic").await;
    tag(&w.h, "s3", "clinic").await;
    let b = signed_in(&w, "Officer").await;
    // programme DDS (s1, s4) AND Tag clinic (s3, s4) -> s4 only.
    create(
        &b,
        &form(
            "กลุ่ม",
            "รายละเอียด",
            "&attr.programme=DDS&tags=clinic&do=confirm",
        ),
    )
    .await;
    drain(&w).await;
    assert_eq!(
        texts(&w, "SELECT DISTINCT student_id FROM announcement_delivery").await,
        ["s4"]
    );
}
