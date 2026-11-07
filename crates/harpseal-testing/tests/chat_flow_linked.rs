//! After Link (#19): free text and files, the hourly fallback, re-consent
//! with a parked Action, self-unlink, Blocked / Unblocked.

mod support;

use harpseal::{Inbound, Message, Statement};
use harpseal_channel_fake::FakeProfile;
use support::{Chat, actions_of, boot, text_of, upload};

const FALLBACK: &str = "ขออภัย ระบบยังตอบข้อความพิมพ์เองไม่ได้ เราเก็บข้อความไว้ให้เจ้าหน้าที่แล้ว กรุณาเลือกจากเมนู";

#[tokio::test]
async fn free_text_is_stored_and_answered_once_an_hour() {
    let (h, app) = boot(FakeProfile::LineLike).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;

    let seen = c.text("ขอใบรับรองครับ").await;
    assert_eq!(text_of(&seen[0]), FALLBACK);
    assert_eq!(actions_of(&seen[1]), ["hello.say", "kernel.link.unlink"]);
    assert!(c.text("ด่วนครับ").await.is_empty(), "stored silently");
    h.clock().advance_minutes(59);
    assert!(c.text("ยังอยู่ไหม").await.is_empty());
    h.clock().advance_minutes(1);
    assert_eq!(text_of(&c.text("สวัสดี").await[0]), FALLBACK);

    let rows = app
        .query("SELECT kind, text, student_id FROM inbound_message ORDER BY rowid")
        .await;
    assert_eq!(rows.len(), 4);
    assert_eq!(rows.get(0, "text").unwrap().as_text(), Some("ขอใบรับรองครับ"));
    assert_eq!(rows.get(3, "student_id").unwrap().as_text(), Some("s1"));
}

#[tokio::test]
async fn files_are_copied_at_once_and_oversized_ones_refused() {
    let (h, app) = boot(FakeProfile::TelegramLike).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;

    let pdf = b"%PDF-1.7 seal bay".to_vec();
    let seen = c.file(upload(&h, "f1", pdf.clone(), None)).await;
    assert_eq!(text_of(&seen[0]), FALLBACK);
    let rows = app
        .query("SELECT kind, file_key, file_name, content_type, size_bytes FROM inbound_message")
        .await;
    let key = rows
        .get(0, "file_key")
        .unwrap()
        .as_text()
        .unwrap()
        .to_string();
    assert!(key.starts_with("inbound/fake/"), "{key}");
    assert_eq!(
        rows.get(0, "file_name").unwrap().as_text(),
        Some("ใบลา.pdf")
    );
    assert_eq!(
        rows.get(0, "size_bytes").unwrap().as_i64(),
        Some(pdf.len() as i64)
    );
    let stored = h.services().files.get(&key).await.unwrap();
    assert_eq!(stored, Some(pdf));

    // Declared over 20 MB: not downloaded, kept as a row, the Student told.
    let seen = c
        .file(upload(&h, "f2", vec![0; 8], Some(21 * 1024 * 1024)))
        .await;
    assert_eq!(text_of(&seen[0]), "ไฟล์ใหญ่เกิน 20 MB จึงไม่ได้บันทึกไว้");
    assert_eq!(
        app.count("SELECT count(*) FROM inbound_message WHERE file_key IS NULL")
            .await,
        1
    );
    assert_eq!(h.files().keys().len(), 1);
}

#[tokio::test]
async fn a_new_notice_version_parks_the_action_and_replays_it_after_accept() {
    let (h, app) = boot(FakeProfile::Full).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    h.db()
        .execute(Statement::new(
            "INSERT INTO setting (key, locale, value, updated_at) \
             VALUES ('kernel.consent.version', '', '2', 1)",
        ))
        .await
        .unwrap();

    let seen = c.press("hello.say").await;
    let [Message::Card(card)] = &seen[..] else {
        panic!("{seen:?}")
    };
    assert_eq!(card.title, "ประกาศความเป็นส่วนตัวฉบับใหม่");
    let state = app
        .query("SELECT flow_state, json_extract(flow_data, '$.parked') AS p FROM channel_account")
        .await;
    assert_eq!(
        state.get(0, "flow_state").unwrap().as_text(),
        Some("awaiting_reconsent")
    );
    assert_eq!(state.get(0, "p").unwrap().as_text(), Some("hello.say"));

    // Pushes from the system still reach the Student meanwhile.
    let d = app.deployment();
    d.send(
        &c.who,
        None,
        &Message::text("ประกาศ: ปิดอาคาร 2"),
        &harpseal::Locale::th(),
    )
    .await
    .unwrap();
    assert_eq!(text_of(c.seen_since().last().unwrap()), "ประกาศ: ปิดอาคาร 2");

    let seen = c.press("kernel.consent.accept").await;
    assert!(
        matches!(&seen[..], [Message::Text(t)] if t == "สวัสดีจากโมดูล"),
        "{seen:?}"
    );
    let versions = app
        .query("SELECT notice_version FROM consent ORDER BY notice_version")
        .await;
    assert_eq!(versions.len(), 2);
    assert_eq!(
        versions.get(1, "notice_version").unwrap().as_text(),
        Some("2")
    );
    let seen = c.press("hello.say").await;
    assert_eq!(text_of(&seen[0]), "สวัสดีจากโมดูล", "no Card once accepted");
}

#[tokio::test]
async fn unlink_round_trip_keeps_consent() {
    let (h, app) = boot(FakeProfile::Full).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;

    let seen = c.press("kernel.link.unlink").await;
    assert_eq!(
        text_of(&seen[0]),
        "ยืนยันยกเลิกการเชื่อมบัญชีนี้หรือไม่ หลังยกเลิกจะไม่ได้รับข่าวทางช่องทางนี้"
    );
    assert_eq!(
        actions_of(&seen[0]),
        ["kernel.link.unlink_confirm", "kernel.link.unlink_keep"]
    );
    let seen = c.press("kernel.link.unlink_keep").await;
    assert_eq!(text_of(&seen[0]), "เลือกบริการจากเมนูได้เลย");
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status = 'active'")
            .await,
        1
    );

    c.press("kernel.link.unlink").await;
    let seen = c.press("kernel.link.unlink_confirm").await;
    assert_eq!(
        text_of(&seen[0]),
        "ยกเลิกการเชื่อมบัญชีแล้ว หากต้องการเชื่อมใหม่ พิมพ์รหัสนักศึกษาได้ทุกเมื่อ"
    );
    assert!(matches!(c.menu(), Some(None)), "Menu locked again");
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status = 'revoked' AND revoked_at IS NOT NULL")
            .await,
        1
    );
    assert_eq!(
        app.count(
            "SELECT count(*) FROM audit_log WHERE action = 'link.revoke' \
             AND actor_kind = 'student' AND actor_id = 's1'"
        )
        .await,
        1
    );
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE revoked_by IS NULL")
            .await,
        1,
        "revoked_by names Staff only"
    );
    assert_eq!(app.count("SELECT count(*) FROM consent").await, 1);

    // Straight back to the student id, no Card: Consent still holds.
    let seen = c.press("hello.say").await;
    assert_eq!(text_of(&seen[0]), "ขอบคุณครับ กรุณาพิมพ์รหัสนักศึกษาของคุณ");
    c.text("68010001").await;
    let code = c.code();
    let seen = c.text(&code).await;
    assert!(text_of(&seen[0]).starts_with("เชื่อมต่อสำเร็จ"));
    assert_eq!(
        app.count("SELECT count(*) FROM link").await,
        2,
        "history kept"
    );
}

#[tokio::test]
async fn blocked_then_unblocked_welcomes_back_without_a_new_code() {
    let (h, app) = boot(FakeProfile::LineLike).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    let emails = h.emails().len();

    assert!(c.send(Inbound::blocked(c.who.clone())).await.is_empty());
    let link = app.query("SELECT status FROM link").await;
    assert_eq!(
        link.get(0, "status").unwrap().as_text(),
        Some("unreachable")
    );
    assert_eq!(
        app.count("SELECT count(*) FROM channel_account WHERE blocked_at IS NOT NULL")
            .await,
        1
    );

    let seen = c.follow().await;
    assert_eq!(
        text_of(&seen[0]),
        "ยินดีต้อนรับกลับมา บัญชีของคุณยังเชื่อมอยู่ ใช้เมนูได้ตามปกติ"
    );
    assert_eq!(seen.len(), 2, "welcome back + Menu Choices");
    assert!(matches!(c.menu(), Some(Some(_))));
    let link = app.query("SELECT status FROM link").await;
    assert_eq!(link.get(0, "status").unwrap().as_text(), Some("active"));
    assert_eq!(
        app.count("SELECT count(*) FROM channel_account WHERE blocked_at IS NOT NULL")
            .await,
        0
    );
    assert_eq!(h.emails().len(), emails, "no new OTP");

    // Blocked again, then any Inbound (an Action) also recovers and runs.
    c.send(Inbound::blocked(c.who.clone())).await;
    let seen = c.press("hello.say").await;
    assert_eq!(
        text_of(&seen[0]),
        "ยินดีต้อนรับกลับมา บัญชีของคุณยังเชื่อมอยู่ ใช้เมนูได้ตามปกติ"
    );
    assert_eq!(text_of(&seen[1]), "สวัสดีจากโมดูล");
}

#[tokio::test]
async fn a_blocked_account_before_link_starts_over_when_back() {
    let (h, app) = boot(FakeProfile::Full).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.follow().await;
    c.send(Inbound::blocked(c.who.clone())).await;
    let seen = c.send(Inbound::unblocked(c.who.clone(), None)).await;
    assert_eq!(seen.len(), 2, "welcome + Card again");
}
