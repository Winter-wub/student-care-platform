//! "ขอให้รับทราบใหม่" (Module migration `0005_reack`): an edit of a sent
//! announcement that asks for an acknowledgement can ask everyone to
//! acknowledge the edited version again; the counts read the latest
//! version, and the earlier versions stay in the history.

mod announcements_support;
mod app_support;

use announcements_support::notice::*;
use announcements_support::*;
use harpseal::modules::announcements::{AckOutcome, ack, ack_action};
use harpseal::{Button, Message};

const REACK: &str = "ข่าวนี้มีการแก้ไข กรุณากดรับทราบฉบับล่าสุดอีกครั้ง";

/// A sent announcement asking for an acknowledgement, acknowledged by s1
/// and s3 (s4 has not yet).
async fn acknowledged(w: &World, b: &harpseal_testing::Browser<'_>) -> String {
    let id = create(b, &form("เดิม", "ข้อความเดิม", "&require_ack=1&do=confirm")).await;
    drain(w).await;
    w.line.take_sent();
    w.tg.take_sent();
    let d = w.app.deployment();
    assert_eq!(ack(d, &id, "s1").await.unwrap(), AckOutcome::Recorded);
    assert_eq!(ack(d, &id, "s3").await.unwrap(), AckOutcome::Recorded);
    id
}

/// The reack notice Cards `student` got on the LINE-like Channel.
fn reack_cards(w: &World, student: &str) -> Vec<harpseal::Card> {
    w.line
        .sent_to(&fake_account("fake", student))
        .into_iter()
        .filter_map(|m| match m {
            Message::Card(c) if c.title == REACK => Some(c),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_new_acknowledgement_starts_the_counts_over_and_keeps_the_old_ones() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = acknowledged(&w, &b).await;
    let acked = format!(
        "SELECT count(DISTINCT student_id) FROM announcement_delivery \
         WHERE announcement_id = '{id}' AND acked_at IS NOT NULL"
    );
    assert_eq!(count(&w, &acked).await, 2);

    // Offered next to the notice box.
    let page = b.get(&format!("/admin/announcements/{id}/edit")).await.body;
    assert!(
        page.contains("name=\"notify\"") && page.contains("name=\"reack\""),
        "{page}"
    );
    assert!(page.contains("ขอให้รับทราบใหม่"), "{page}");

    let r = post_edit(&w, &b, &id, &form("ใหม่", "ข้อความใหม่", "&notify=1&reack=1")).await;
    assert_eq!(r.status, 303, "{}", r.body);
    drain(&w).await;

    // Everyone acknowledges again: no acknowledgement left, version 2.
    assert_eq!(count(&w, &acked).await, 0);
    assert_eq!(
        count(
            &w,
            &format!("SELECT ack_round FROM announcement WHERE id = '{id}'")
        )
        .await,
        1
    );
    // The closed version: its counts and texts, who acknowledged it.
    assert_eq!(
        texts(
            &w,
            &format!(
                "SELECT ack_round || ' ' || students || ' ' || acked || ' ' || \
                 (texts -> 0 ->> 'title') FROM announcement_ack_round WHERE announcement_id = '{id}'"
            )
        )
        .await,
        ["0 3 2 เดิม"]
    );
    assert_eq!(
        texts(
            &w,
            &format!(
                "SELECT student_id FROM announcement_ack_history \
                 WHERE announcement_id = '{id}' ORDER BY student_id"
            )
        )
        .await,
        ["s1", "s3"]
    );
    // Audited with the save.
    assert_eq!(
        texts(
            &w,
            "SELECT after ->> 'reack' FROM audit_log WHERE action = 'announcement.update'"
        )
        .await,
        ["1"]
    );

    // The notice asks to acknowledge again, with the button.
    for s in ["s1", "s3"] {
        let cards = reack_cards(&w, s);
        assert_eq!(cards.len(), 1, "{s}");
        assert!(
            cards[0]
                .buttons
                .iter()
                .any(|x| matches!(x, Button::Action { action, .. }
                if *action == ack_action(&id).unwrap())),
            "{:?}",
            cards[0].buttons
        );
    }
    // A tap now counts for the new version.
    let d = w.app.deployment();
    assert_eq!(ack(d, &id, "s1").await.unwrap(), AckOutcome::Recorded);
    assert_eq!(ack(d, &id, "s1").await.unwrap(), AckOutcome::Already);
    assert_eq!(count(&w, &acked).await, 1);

    // The detail page names the version and links the history.
    let detail = b.get(&format!("/admin/announcements/{id}")).await.body;
    assert!(detail.contains("ฉบับที่ 2"), "{detail}");
    assert!(
        detail.contains(&format!("/admin/announcements/{id}/acks")),
        "{detail}"
    );
    assert!(detail.contains("1 / 3"), "{detail}");
    // The history: the latest version's counts, then version 1's.
    let history = b.get(&format!("/admin/announcements/{id}/acks")).await;
    assert_eq!(history.status, 200);
    let h = history.body;
    assert!(h.contains("ฉบับที่ 1") && h.contains("ฉบับที่ 2"), "{h}");
    assert!(h.contains("2 / 3") && h.contains("ข้อความเดิม"), "{h}");
    let who = b
        .get(&format!("/admin/announcements/{id}/acks?round=0"))
        .await
        .body;
    assert!(who.contains("Name s1") && who.contains("code-s3"), "{who}");
    assert!(!who.contains("Name s4"), "{who}");

    // A second request keeps both earlier versions.
    let r = post_edit(&w, &b, &id, &form("ใหม่อีก", "ข้อความใหม่อีก", "&reack=1")).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(
        texts(
            &w,
            &format!(
                "SELECT ack_round || ':' || acked FROM announcement_ack_round \
                 WHERE announcement_id = '{id}' ORDER BY ack_round"
            )
        )
        .await,
        ["0:2", "1:1"]
    );
    assert_eq!(count(&w, &acked).await, 0);
}

#[tokio::test]
async fn with_a_new_acknowledgement_the_notice_goes_even_without_a_text_change() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = acknowledged(&w, &b).await;
    // The same text: refused with the notice alone...
    let same = form("เดิม", "ข้อความเดิม", "&notify=1");
    let r = post_edit(&w, &b, &id, &same).await;
    assert_eq!(r.status, 422, "{}", r.body);
    // ...accepted with a new acknowledgement: everyone on LINE is told.
    let r = post_edit(&w, &b, &id, &form("เดิม", "ข้อความเดิม", "&notify=1&reack=1")).await;
    assert_eq!(r.status, 303, "{}", r.body);
    drain(&w).await;
    for s in ["s1", "s3"] {
        assert_eq!(reack_cards(&w, s).len(), 1, "{s}");
    }
    assert!(notices(&w, "s1").is_empty(), "not the plain edit notice");
}

#[tokio::test]
async fn an_edit_without_it_keeps_the_acknowledgements() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = acknowledged(&w, &b).await;
    edit(&w, &b, &id, "ใหม่", true).await;
    drain(&w).await;
    assert_eq!(
        count(
            &w,
            &format!(
                "SELECT count(*) FROM announcement_delivery \
                 WHERE announcement_id = '{id}' AND acked_at IS NOT NULL"
            )
        )
        .await,
        3,
        "s1 on two Channels, s3"
    );
    assert_eq!(
        count(&w, "SELECT count(*) FROM announcement_ack_round").await,
        0
    );
    let history = b.get(&format!("/admin/announcements/{id}/acks")).await.body;
    assert!(history.contains("ยังไม่เคยขอให้รับทราบใหม่"), "{history}");
}

#[tokio::test]
async fn it_is_refused_without_an_acknowledgement_or_once_expired() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    // No acknowledgement asked for: not offered, and refused if posted.
    let id = sent(&w, &b).await;
    let page = b.get(&format!("/admin/announcements/{id}/edit")).await.body;
    assert!(!page.contains("name=\"reack\""), "{page}");
    let r = post_edit(&w, &b, &id, &form("ใหม่", "ข้อความใหม่", "&reack=1")).await;
    assert_eq!(r.status, 422, "{}", r.body);
    assert!(r.body.contains("ข่าวนี้ไม่ได้ขอให้กดรับทราบ"), "{}", r.body);

    // Expired: Students could not acknowledge it.
    let id = acknowledged(&w, &b).await;
    run(
        &w.h,
        &format!("UPDATE announcement SET expires_at = 1 WHERE id = '{id}'"),
    )
    .await;
    let r = post_edit(&w, &b, &id, &form("ใหม่", "ข้อความใหม่", "&reack=1")).await;
    assert_eq!(r.status, 422, "{}", r.body);
    assert!(r.body.contains("ขอให้รับทราบใหม่ไม่ได้"), "{}", r.body);
    assert_eq!(
        count(&w, "SELECT count(*) FROM announcement_ack_round").await,
        0
    );
}
