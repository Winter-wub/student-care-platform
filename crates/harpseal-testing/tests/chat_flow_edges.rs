//! Edge cases from the #19 review: failing email and downloads, `setting`
//! overrides reaching the Menu and Modules, `self_unlink = false`, input
//! during re-consent, a Link revoked by Staff, the duplicate warning cap,
//! English warnings and Pushes across two registered Channels.

mod support;

use std::sync::Arc;

use async_trait::async_trait;
use harpseal::{Email, EmailSender, Error, Message};
use harpseal_channel_fake::{FakeChannel, FakeProfile};
use harpseal_testing::Harness;
use support::{Chat, actions_of, boot, boot_with, sql, text_of, upload};

struct Bouncing;

#[async_trait]
impl EmailSender for Bouncing {
    async fn send(&self, _: Email) -> harpseal::Result<()> {
        Err(Error::Platform("provider said 503".into()))
    }
}

#[tokio::test]
async fn a_failed_email_answers_try_later_and_does_not_count() {
    let (h, app) = boot_with(FakeProfile::Full, "", |mut s| {
        s.email = Arc::new(Bouncing);
        s
    })
    .await;
    let mut c = Chat::new(&h, &app, "u1");
    c.follow().await;
    c.press("kernel.consent.accept").await;
    for _ in 0..4 {
        let seen = c.text("68010001").await;
        assert_eq!(text_of(&seen[0]), "ส่งอีเมลรหัสยืนยันไม่สำเร็จ กรุณาลองใหม่ภายหลัง");
    }
    assert_eq!(
        app.count("SELECT count(*) FROM verification_challenge")
            .await,
        0
    );
    assert_eq!(
        app.count("SELECT count(*) FROM security_event WHERE kind = 'otp.send_failed'")
            .await,
        4
    );
    let state = app.query("SELECT flow_state FROM channel_account").await;
    assert_eq!(
        state.get(0, "flow_state").unwrap().as_text(),
        Some("awaiting_student_id")
    );
}

#[tokio::test]
async fn a_failed_download_keeps_the_row_and_still_answers() {
    let (h, app) = boot(FakeProfile::Full).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    let mut gone = upload(&h, "present", vec![1], None);
    gone.platform_file_id = "missing".into();
    let seen = c.file(gone).await;
    assert!(
        text_of(&seen[0]).starts_with("ขออภัย"),
        "fallback still sent"
    );
    let rows = app
        .query("SELECT kind, file_key, file_error FROM inbound_message")
        .await;
    assert_eq!(rows.get(0, "kind").unwrap().as_text(), Some("file"));
    assert_eq!(rows.get(0, "file_key"), Some(&harpseal::Value::Null));
    let error = rows.get(0, "file_error").unwrap().as_text().unwrap();
    assert!(error.contains("missing"), "{error}");
}

#[tokio::test]
async fn setting_overrides_reach_the_menu_and_modules() {
    let (h, app) = boot(FakeProfile::Full).await;
    sql(&h, "INSERT INTO setting (key, locale, value, updated_at) VALUES ('hello.menu', 'th', 'ทักทายหน่อย', 1)").await;
    sql(&h, "INSERT INTO setting (key, locale, value, updated_at) VALUES ('hello.hi', 'th', 'หวัดดีจ้า', 1)").await;
    let mut c = Chat::new(&h, &app, "u1");
    let seen = c.link("68010001").await;
    let Message::Choices(menu) = &seen[1] else {
        panic!("{seen:?}")
    };
    assert_eq!(menu.options[0].label(), "ทักทายหน่อย");
    assert_eq!(c.menu().unwrap().unwrap().entries[0].label, "ทักทายหน่อย");
    assert_eq!(text_of(&c.press("hello.say").await[0]), "หวัดดีจ้า");
}

#[tokio::test]
async fn without_self_unlink_the_menu_has_no_unlink_and_the_action_is_refused() {
    let (h, app) = boot_with(FakeProfile::Full, "[student]\nself_unlink = false\n", |s| s).await;
    let mut c = Chat::new(&h, &app, "u1");
    let seen = c.link("68010001").await;
    assert_eq!(actions_of(&seen[1]), ["hello.say"]);
    let seen = c.press("kernel.link.unlink_confirm").await;
    assert!(text_of(&seen[0]).starts_with("ปุ่มนี้ใช้ไม่ได้แล้ว"));
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status = 'active'")
            .await,
        1
    );
}

#[tokio::test]
async fn text_and_files_during_reconsent_are_stored_and_get_the_card() {
    let (h, app) = boot(FakeProfile::Full).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    sql(&h, "INSERT INTO setting (key, locale, value, updated_at) VALUES ('kernel.consent.version', '', '2', 1)").await;
    let seen = c.text("ขอเอกสารครับ").await;
    assert!(matches!(&seen[..], [Message::Card(card)] if card.title == "ประกาศความเป็นส่วนตัวฉบับใหม่"));
    let seen = c.file(upload(&h, "f1", b"pdf".to_vec(), None)).await;
    assert!(matches!(&seen[..], [Message::Card(_)]), "{seen:?}");
    assert_eq!(app.count("SELECT count(*) FROM inbound_message").await, 2);
    assert_eq!(h.files().keys().len(), 1);
    let seen = c.press("kernel.consent.accept").await;
    assert_eq!(
        text_of(&seen[0]),
        "เลือกบริการจากเมนูได้เลย",
        "nothing parked: the Menu"
    );
}

#[tokio::test]
async fn consent_given_elsewhere_releases_a_parked_account() {
    let (h, app) = boot(FakeProfile::Full).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    sql(&h, "INSERT INTO setting (key, locale, value, updated_at) VALUES ('kernel.consent.version', '', '2', 1)").await;
    c.press("hello.say").await;
    // Accepted version 2 on another Channel meanwhile.
    sql(&h, "INSERT INTO consent (id, student_id, channel_account_id, notice_version, locale, accepted_at) \
             SELECT 'c2', 's1', id, '2', 'th', 2 FROM channel_account WHERE external_id = 'u1'").await;
    let seen = c.follow().await;
    assert_eq!(text_of(&seen[0]), "สวัสดีจากโมดูล", "parked Action replayed");
    let state = app.query("SELECT flow_state FROM channel_account").await;
    assert_eq!(
        state.get(0, "flow_state").unwrap().as_text(),
        Some("linked")
    );
}

#[tokio::test]
async fn a_link_revoked_by_staff_starts_over() {
    let (h, app) = boot(FakeProfile::Full).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    sql(&h, "UPDATE link SET status = 'revoked', revoked_at = 5").await;
    let seen = c.text("สวัสดี").await;
    assert_eq!(seen.len(), 2, "welcome + Card: {seen:?}");
    assert!(matches!(&seen[1], Message::Card(_)));
    assert!(matches!(c.menu(), Some(None)), "Menu locked");
}

#[tokio::test]
async fn the_duplicate_warning_goes_once_an_hour_in_the_owners_language() {
    let (h, app) = boot(FakeProfile::Full).await;
    let mut owner = Chat::new(&h, &app, "owner");
    owner.link("68010004").await;
    let mut other = Chat::new(&h, &app, "other");
    other.follow().await;
    other.press("kernel.consent.accept").await;
    other.text("68010004").await;
    let warned = owner.seen_since();
    assert_eq!(
        text_of(&warned[0]),
        "Someone tried to link your student ID from another account. If that was not you, please tell Staff."
    );
    h.fake_channel().take_sent();
    other.text("68010004").await;
    assert!(owner.seen_since().is_empty(), "no second warning this hour");
    assert_eq!(
        app.count("SELECT count(*) FROM security_event WHERE kind = 'link.duplicate_attempt'")
            .await,
        2
    );
    h.clock().advance_minutes(61);
    other.text("68010004").await;
    assert_eq!(owner.seen_since().len(), 1);
}

#[tokio::test]
async fn a_lockout_notifies_the_link_on_the_other_channel_in_english() {
    let h = Harness::new();
    let second = FakeChannel::new("second-secret")
        .unwrap()
        .with_kind("fake2");
    let d = h.builder().channel(second.clone()).build().unwrap();
    let app = h.boot(d).await;
    for q in support::ROSTER {
        sql(&h, q).await;
    }
    // Karn (English) is linked on the second Channel.
    sql(&h, "INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) VALUES ('a2', 'fake2', 'karn-tg', 1, 1)").await;
    sql(&h, "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, linked_at, updated_at) \
             VALUES ('l2', 's4', 'a2', 'fake2', 'active', 'manual', 1, 1)").await;
    let mut c = Chat::new(&h, &app, "u1");
    c.follow().await;
    c.press("kernel.consent.accept").await;
    c.text("68010004").await;
    let code = c.code();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for _ in 0..3 {
        c.text(wrong).await;
    }
    let seen = second.sent_to(&harpseal::ChannelAccountId::new(
        harpseal::ChannelKind("fake2"),
        "karn-tg",
    ));
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(
        text_of(&seen[0]),
        "Someone typed wrong codes for your student ID from another account. If that was not you, please tell Staff."
    );
}
