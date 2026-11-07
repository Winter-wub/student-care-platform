//! Announcement follow-ups (#77): the edit notice on Channels that cannot
//! edit, `[announcements] latest`, and expired items closed to Students.

mod announcements_support;
mod app_support;

use announcements_support::notice::*;
use announcements_support::*;
use app_support::*;
use harpseal::modules::announcements::{AckOutcome, LATEST, ack, ack_action};
use harpseal::{Button, DeploymentConfig, Message};

const GONE: &str = "ข่าวนี้ไม่เปิดให้ดูแล้ว";

#[tokio::test]
async fn the_edit_notice_goes_once_per_line_recipient_and_telegram_edits_instead() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = sent(&w, &b).await;
    let before = metered(&w, "fake").await;
    // The form offers it once sent.
    let page = b.get(&format!("/admin/announcements/{id}/edit")).await.body;
    assert!(page.contains("name=\"notify\""), "{page}");

    edit(&w, &b, &id, "ใหม่", true).await;
    drain(&w).await;
    // LINE-like: s1 and s3, one notice each, linking to the item.
    for s in ["s1", "s3"] {
        assert_eq!(notices(&w, s), ["ใหม่"], "{s}");
    }
    let card = w
        .line
        .sent_to(&fake_account("fake", "s1"))
        .into_iter()
        .find_map(|m| match m {
            Message::Card(c) => Some(c),
            _ => None,
        })
        .unwrap();
    assert!(
        card.buttons
            .iter()
            .any(|x| matches!(x, Button::MiniApp { path, .. }
        if *path == format!("/app/announcements/{id}")))
    );
    // Telegram-like: edited in place, nothing new sent.
    assert!(w.tg.sent().is_empty());
    assert_eq!(w.tg.edited().len(), 2);
    // Metered like any Push; the report, the audit row with the count.
    assert_eq!(metered(&w, "fake").await - before, 2);
    assert_eq!(
        texts(
            &w,
            "SELECT notice_report ->> 'sent' || '/' || (notice_report ->> 'total') \
             || '/' || (notice_report ->> 'failed') FROM announcement"
        )
        .await,
        ["2/2/0"]
    );
    assert_eq!(
        texts(
            &w,
            "SELECT after ->> 'recipients' FROM audit_log WHERE action = 'announcement.notice'"
        )
        .await,
        ["2"]
    );
    let detail = b.get(&format!("/admin/announcements/{id}")).await.body;
    assert!(detail.contains("แจ้งการแก้ไข"), "{detail}");

    // Running the Jobs again sends nothing more; an edit without the
    // notice sends none either.
    drain(&w).await;
    edit(&w, &b, &id, "ใหม่อีก", false).await;
    drain(&w).await;
    assert_eq!(notices(&w, "s1").len(), 1);
    assert_eq!(metered(&w, "fake").await - before, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_later_edit_supersedes_a_running_notice() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = sent(&w, &b).await;
    edit(&w, &b, &id, "แก้ครั้งที่หนึ่ง", true).await;
    // Right before the first notice Push, a later save takes the report
    // (what a save with "notify" writes): the run stops after that Push.
    let db = w.h.db().clone();
    let later = format!(
        "UPDATE announcement SET notice_report = json_object('version', version + 1, \
         'at', 0, 'sent', 0, 'failed', 0, 'total', NULL) WHERE id = '{id}'"
    );
    w.line.before_next_send(move || {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(db.execute(harpseal::Statement::new(later)))
                .unwrap();
        });
    });
    drain(&w).await;
    let first: usize = ["s1", "s3"].iter().map(|s| notices(&w, s).len()).sum();
    assert_eq!(first, 1, "only the Push already on its way");

    // A real second edit with the notice reaches everyone once more.
    edit(&w, &b, &id, "แก้ครั้งที่สอง", true).await;
    drain(&w).await;
    for s in ["s1", "s3"] {
        assert_eq!(notices(&w, s).last().map(String::as_str), Some("แก้ครั้งที่สอง"));
    }
    let total: usize = ["s1", "s3"].iter().map(|s| notices(&w, s).len()).sum();
    assert_eq!(total, 3);
}

#[tokio::test]
async fn a_retracted_announcement_sends_no_notice() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = sent(&w, &b).await;
    edit(&w, &b, &id, "ใหม่", true).await;
    let v = version(&w, &id).await;
    let r = b
        .post(
            &format!("/admin/announcements/{id}/retract"),
            &format!("version={v}&confirm=yes"),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    drain(&w).await;
    assert!(notices(&w, "s1").is_empty() && notices(&w, "s3").is_empty());
}

#[tokio::test]
async fn latest_follows_the_config_and_keeps_see_all() {
    let src = format!("{CONFIG}\n[announcements]\nlatest = 4\n");
    let w = boot_config(&src, 500).await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    for n in 0..6 {
        create(&b, &form(&format!("ข่าว {n}"), "รายละเอียด", "&do=confirm")).await;
        w.h.clock().advance_ms(1_000);
    }
    drain(&w).await;
    let seen = press(&w, "fake", "s1", LATEST).await;
    let Some(Message::CardList(list)) = seen.first() else {
        panic!("{seen:?}");
    };
    assert_eq!(list.0.len(), 4);
    let all = &list.0[3].buttons;
    assert!(all.iter().any(|x| matches!(x, Button::MiniApp { path, .. }
        if path == "/app/announcements")));
    // Out of range: refused.
    for bad in ["0", "11"] {
        let src = format!("{CONFIG}\n[announcements]\nlatest = {bad}\n");
        assert!(DeploymentConfig::from_toml(&src).is_err(), "{bad}");
    }
}

#[tokio::test]
async fn an_expired_item_is_closed_to_students_but_staff_still_see_it() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("หมดเวลา", "ข้อความ", "&do=confirm&require_ack=1")).await;
    drain(&w).await;
    let now = w.h.clock().now_ms();
    run(
        &w.h,
        &format!(
            "UPDATE announcement SET expires_at = {} WHERE id = '{id}'",
            now - 1
        ),
    )
    .await;
    // Chat: the ack Action on the Card says it is gone, records nothing.
    let seen = press(&w, "fake", "s1", ack_action(&id).unwrap().as_str()).await;
    assert_eq!(seen.iter().map(text).collect::<Vec<_>>(), [GONE]);
    let d = w.app.deployment();
    assert_eq!(ack(d, &id, "s1").await.unwrap(), AckOutcome::Expired);
    // Mini App deep link: the "no longer available" view, no text, no ack.
    let m = open(&w, "fake", "s1").await;
    let csrf = csrf_of(&m.get("/app").await.body);
    let page = m.get(&format!("/app/announcements/{id}")).await;
    assert_eq!(page.status, 200);
    assert!(page.body.contains(GONE), "{}", page.body);
    assert!(!page.body.contains("ข้อความ</p>"));
    assert!(!page.body.contains("class=\"app-ack\""));
    let r = hx_post(
        &m,
        &format!("/app/announcements/{id}/ack"),
        &format!("_csrf={csrf}"),
    )
    .await;
    assert_eq!(r.status, 200);
    assert!(r.body.contains(GONE), "{}", r.body);
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM announcement_delivery WHERE acked_at IS NOT NULL"
        )
        .await,
        0
    );
    // Staff: still listed and shown, with the badge.
    let list = b.get("/admin/announcements").await.body;
    assert!(
        list.contains("หมดเวลา") && list.contains("หมดอายุแล้ว"),
        "{list}"
    );
    let detail = b.get(&format!("/admin/announcements/{id}")).await.body;
    assert!(detail.contains("ข้อความ") && detail.contains("หมดอายุแล้ว"));
}

#[tokio::test]
async fn a_revoked_link_gets_no_notice() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = sent(&w, &b).await;
    run(
        &w.h,
        "UPDATE link SET status = 'revoked' WHERE id = 'l-fake-s3'",
    )
    .await;
    edit(&w, &b, &id, "ใหม่", true).await;
    drain(&w).await;
    assert_eq!(notices(&w, "s1"), ["ใหม่"]);
    assert!(notices(&w, "s3").is_empty());
}
