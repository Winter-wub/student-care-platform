//! The chat flow (#19) at the Deployment seam: Consent, student id, email
//! OTP and Link, asserting what the Student sees and the Menu state.

mod support;

use harpseal::{Message, Statement};
use harpseal_channel_fake::FakeProfile;
use harpseal_testing::{Harness, START_MS};
use support::{Chat, actions_of, boot, text_of};

/// Every message of the happy path, per profile.
async fn happy_path(profile: FakeProfile) {
    let (h, app) = boot(profile).await;
    let mut c = Chat::new(&h, &app, "u1");

    let seen = c.follow().await;
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert!(text_of(&seen[0]).starts_with("สวัสดีครับ ยินดีต้อนรับ"));
    let Message::Card(card) = &seen[1] else {
        panic!("{seen:?}")
    };
    assert_eq!(card.title, "ประกาศความเป็นส่วนตัว");
    assert_eq!(actions_of(&seen[1]), ["kernel.consent.accept"]);
    assert!(
        card.buttons
            .iter()
            .any(|b| matches!(b, harpseal::Button::MiniApp { path, .. } if path == "/notice")),
        "read in full opens the Mini App"
    );
    assert!(matches!(c.menu(), Some(None)), "locked Menu before Link");

    let seen = c.press("kernel.consent.accept").await;
    assert!(matches!(&seen[..], [Message::Text(t)] if t == "ขอบคุณครับ กรุณาพิมพ์รหัสนักศึกษาของคุณ"));

    let seen = c.text(" 6801 0001 ").await;
    let [Message::Choices(ch)] = &seen[..] else {
        panic!("{seen:?}")
    };
    assert_eq!(
        ch.text,
        "ส่งรหัสยืนยันไปที่ s******1@student.sealbay.example แล้ว (ใช้ได้ 5 นาที) พิมพ์รหัสที่ได้รับในแชทนี้"
    );
    assert_eq!(
        actions_of(&seen[0]),
        ["kernel.otp.resend", "kernel.otp.cancel"]
    );
    let email = h.email().last().unwrap();
    assert_eq!(email.to, "s68010001@student.sealbay.example");
    let code = c.code();
    assert_eq!(code.len(), 6);
    assert!(email.text.contains(&code) && email.text.contains("Nok Thale"));

    let seen = c.text(&format!("{}-{}", &code[..3], &code[3..])).await;
    assert_eq!(seen.len(), 2, "{seen:?}");
    let Message::Card(ok) = &seen[0] else {
        panic!("{seen:?}")
    };
    assert_eq!(ok.title, "เชื่อมต่อสำเร็จ");
    assert_eq!(ok.body, "สวัสดี Nok Thale บัญชีของคุณเชื่อมเรียบร้อยแล้ว");
    let Message::Choices(menu) = &seen[1] else {
        panic!("{seen:?}")
    };
    assert_eq!(menu.text, "เลือกบริการจากเมนูได้เลย");
    assert_eq!(actions_of(&seen[1]), ["hello.say", "kernel.link.unlink"]);
    let unlocked = c.menu().unwrap().expect("Menu unlocked");
    let labels: Vec<&str> = unlocked.entries.iter().map(|e| e.label.as_str()).collect();
    assert_eq!(labels, ["ทักทาย", "ยกเลิกการเชื่อมบัญชี"]);

    // One batch wrote the Link, the Consent, the state and the audit row.
    let link = app
        .query("SELECT student_id, status, method FROM link")
        .await;
    assert_eq!(link.get(0, "student_id").unwrap().as_text(), Some("s1"));
    assert_eq!(link.get(0, "status").unwrap().as_text(), Some("active"));
    assert_eq!(link.get(0, "method").unwrap().as_text(), Some("email_otp"));
    let consent = app
        .query("SELECT student_id, notice_version FROM consent")
        .await;
    assert_eq!(
        consent.get(0, "notice_version").unwrap().as_text(),
        Some("1")
    );
    assert_eq!(
        app.count("SELECT count(*) FROM channel_account WHERE flow_state = 'linked'")
            .await,
        1
    );
    let audit = app
        .query("SELECT actor_kind, actor_id, action FROM audit_log")
        .await;
    assert_eq!(
        audit.get(0, "action").unwrap().as_text(),
        Some("link.create")
    );
    assert_eq!(audit.get(0, "actor_id").unwrap().as_text(), Some("s1"));

    // The Menu Choices are listed once; then Actions reach the Module.
    let seen = c.press("hello.say").await;
    assert!(matches!(&seen[..], [Message::Text(t)] if t == "สวัสดีจากโมดูล"));
}

#[tokio::test]
async fn happy_path_on_the_full_fake_channel() {
    happy_path(FakeProfile::Full).await;
}

#[tokio::test]
async fn happy_path_line_like_answers_every_step_by_reply() {
    happy_path(FakeProfile::LineLike).await;
    let (h, app) = boot(FakeProfile::LineLike).await;
    Chat::new(&h, &app, "u1").link("68010001").await;
    assert!(
        h.fake_channel()
            .sent()
            .iter()
            .all(|s| s.metering == harpseal::Metering::Free)
    );
}

#[tokio::test]
async fn happy_path_telegram_like_pushes_for_free() {
    happy_path(FakeProfile::TelegramLike).await;
}

#[tokio::test]
async fn the_card_says_where_to_read_without_a_mini_app() {
    let h = Harness::new();
    let fake = h.fake_channel().without_mini_app();
    let d = harpseal::Deployment::builder()
        .config(Harness::config())
        .runtime(h.services())
        .channel(fake)
        .verification(harpseal::EmailOtp::default())
        .build()
        .unwrap();
    let app = h.boot(d).await;
    let seen = Chat::new(&h, &app, "u1").follow().await;
    let Message::Card(card) = &seen[1] else {
        panic!("{seen:?}")
    };
    assert_eq!(card.buttons.len(), 1);
    assert!(card.body.contains("/notice"));
}

#[tokio::test]
async fn any_other_input_resends_the_card_and_english_students_get_english() {
    let (h, app) = boot(FakeProfile::Full).await;
    let mut c = Chat::new(&h, &app, "u1");
    let seen = c.text("hello?").await;
    assert_eq!(seen.len(), 2, "first contact: welcome + Card");
    let seen = c.text("68010004").await;
    assert!(matches!(&seen[..], [Message::Card(_)]), "{seen:?}");
    let seen = c.press("hello.say").await;
    assert!(
        matches!(&seen[..], [Message::Card(_)]),
        "Modules never see pre-Link Actions"
    );
    c.press("kernel.consent.accept").await;
    c.text("68010004").await;
    assert!(
        h.email()
            .last()
            .unwrap()
            .subject
            .starts_with("Your verification code")
    );
    let code = c.code();
    let seen = c.text(&code).await;
    assert_eq!(
        text_of(&seen[0]),
        "Linked\nHello Karn Bai-ruea, your account is now linked."
    );
    assert_eq!(text_of(&seen[1]), "Pick a service from the Menu.");
}

#[tokio::test]
async fn each_student_id_failure_has_its_text() {
    let (h, app) = boot(FakeProfile::Full).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.follow().await;
    c.press("kernel.consent.accept").await;
    let cases = [
        ("6801!", "รูปแบบรหัสนักศึกษาไม่ถูกต้อง กรุณาตรวจสอบแล้วพิมพ์ใหม่"),
        (
            "69999999",
            "ไม่พบรหัสนักศึกษานี้ในทะเบียน กรุณาตรวจสอบ หรือติดต่อเจ้าหน้าที่",
        ),
        (
            "68010002",
            "ไม่พบรหัสนักศึกษานี้ในทะเบียน กรุณาตรวจสอบ หรือติดต่อเจ้าหน้าที่",
        ),
        (
            "68010003",
            "ทะเบียนไม่มีอีเมลของรหัสนี้ จึงส่งรหัสยืนยันไม่ได้ กรุณาติดต่อเจ้าหน้าที่เพื่อเชื่อมบัญชีให้",
        ),
    ];
    for (typed, expected) in cases {
        let seen = c.text(typed).await;
        assert!(
            matches!(&seen[..], [Message::Text(t)] if t == expected),
            "{typed}: {seen:?}"
        );
    }
    assert!(h.emails().is_empty());
}

#[tokio::test]
async fn the_toml_pattern_decides_the_format() {
    let h = Harness::new();
    let toml = format!(
        "{}\n[student]\nid_pattern = \"########\"\n",
        harpseal_testing::TEST_CONFIG
    );
    let config = harpseal::DeploymentConfig::from_toml(&toml).unwrap();
    let d = h.builder().config(config).build().unwrap();
    let app = h.boot(d).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.follow().await;
    c.press("kernel.consent.accept").await;
    let seen = c.text("B6801000").await;
    assert!(text_of(&seen[0]).starts_with("รูปแบบรหัสนักศึกษาไม่ถูกต้อง"));
}

#[tokio::test]
async fn the_code_is_stored_salted_and_hashed_and_expires() {
    let (h, app) = boot(FakeProfile::Full).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.follow().await;
    c.press("kernel.consent.accept").await;
    c.text("68010001").await;
    let code = c.code();
    let rows = app
        .query("SELECT secret_hash, expires_at FROM verification_challenge")
        .await;
    let hash = rows
        .get(0, "secret_hash")
        .unwrap()
        .as_text()
        .unwrap()
        .to_string();
    assert!(
        hash.starts_with("sha256$") && !hash.contains(&code),
        "{hash}"
    );
    assert_eq!(
        rows.get(0, "expires_at").unwrap().as_i64(),
        Some(START_MS + 5 * 60_000)
    );

    h.clock().advance_minutes(6);
    let seen = c.text(&code).await;
    assert_eq!(text_of(&seen[0]), "รหัสหมดอายุแล้ว กดส่งรหัสใหม่ได้เลย");
    assert_eq!(
        actions_of(&seen[0]),
        ["kernel.otp.resend", "kernel.otp.cancel"]
    );
    assert_eq!(
        app.count("SELECT count(*) FROM verification_challenge WHERE state = 'expired'")
            .await,
        1
    );
    assert_eq!(app.count("SELECT count(*) FROM link").await, 0);

    // Resend: a new code works; the old one no longer does.
    c.press("kernel.otp.resend").await;
    let fresh = c.code();
    assert_eq!(h.emails().len(), 2);
    let seen = c.text(&fresh).await;
    assert!(text_of(&seen[0]).starts_with("เชื่อมต่อสำเร็จ"));
}

#[tokio::test]
async fn non_digits_do_not_count_and_cancel_goes_back() {
    let (h, app) = boot(FakeProfile::Full).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.follow().await;
    c.press("kernel.consent.accept").await;
    c.text("68010001").await;
    let seen = c.text("abc").await;
    assert_eq!(text_of(&seen[0]), "กรุณาพิมพ์รหัสยืนยันเป็นตัวเลข 6 หลัก");
    assert_eq!(
        app.count("SELECT attempts FROM verification_challenge")
            .await,
        0
    );
    let seen = c.press("kernel.otp.cancel").await;
    assert_eq!(text_of(&seen[0]), "ขอบคุณครับ กรุณาพิมพ์รหัสนักศึกษาของคุณ");
    assert_eq!(
        app.count("SELECT count(*) FROM verification_challenge WHERE state = 'cancelled'")
            .await,
        1
    );
    let flow = app.query("SELECT flow_state FROM channel_account").await;
    assert_eq!(
        flow.get(0, "flow_state").unwrap().as_text(),
        Some("awaiting_student_id")
    );
}

#[tokio::test]
async fn more_than_three_codes_an_hour_must_wait() {
    let (h, app) = boot(FakeProfile::Full).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.follow().await;
    c.press("kernel.consent.accept").await;
    c.text("68010001").await;
    h.clock().advance_minutes(10);
    c.press("kernel.otp.resend").await;
    c.press("kernel.otp.resend").await;
    assert_eq!(h.emails().len(), 3);
    let seen = c.press("kernel.otp.resend").await;
    assert_eq!(text_of(&seen[0]), "ขอรหัสครบจำนวนที่กำหนดแล้ว กรุณารออีก 50 นาที");
    assert_eq!(h.emails().len(), 3);
    h.clock().advance_minutes(51);
    c.press("kernel.otp.resend").await;
    assert_eq!(h.emails().len(), 4);
}

#[tokio::test]
async fn a_setting_row_overrides_a_text() {
    let (h, app) = boot(FakeProfile::Full).await;
    h.db()
        .execute(Statement::new(
            "INSERT INTO setting (key, locale, value, updated_at) \
             VALUES ('kernel.flow.welcome', 'th', 'ยินดีต้อนรับสู่อ่าวแมวน้ำ', 1)",
        ))
        .await
        .unwrap();
    let seen = Chat::new(&h, &app, "u1").follow().await;
    assert_eq!(text_of(&seen[0]), "ยินดีต้อนรับสู่อ่าวแมวน้ำ");
}

#[tokio::test]
async fn the_locked_menus_start_entry_behaves_like_a_follow() {
    let (h, app) = boot(FakeProfile::LineLike).await;
    let mut c = Chat::new(&h, &app, "u1");
    let seen = c.press("kernel.link.start").await;
    assert_eq!(seen.len(), 2, "welcome + Consent Card: {seen:?}");
    assert!(matches!(&seen[1], Message::Card(_)));
}
