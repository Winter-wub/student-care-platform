//! Unreachable Links (#20): a send refused as unreachable marks the Link,
//! Pushes and Menu changes skip a blocked account, Staff can revoke or
//! Manual Link it, and its next Inbound welcomes it back.

mod support;

use harpseal::{Error, Inbound, Locale, Message, RequestCtx, links};
use harpseal_channel_fake::FakeProfile;
use support::{Chat, HELLO, WELCOME_BACK, boot, col, live_link, officer, text_of};

#[tokio::test]
async fn a_send_refused_as_unreachable_marks_the_link_and_pushes_skip_it() {
    let (h, app) = boot(FakeProfile::LineLike).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    let emails = h.emails().len();
    let d = app.deployment();
    let th = Locale::th();
    let push = Message::text("ประกาศ: ปิดอาคาร 2");

    h.fake_channel().set_unreachable(&c.who, true);
    let r = d.send(&c.who, None, &push, &th).await;
    assert!(matches!(r, Err(Error::Unreachable)), "{r:?}");
    let link = app.query("SELECT status FROM link").await;
    assert_eq!(col(&link, "status").as_deref(), Some("unreachable"));
    let menu = d.channel("fake").unwrap().set_menu(&c.who, None).await;
    assert!(matches!(menu, Err(Error::Unreachable)), "{menu:?}");

    // Skipped from now on, without a platform call or an outbound row.
    h.fake_channel().set_unreachable(&c.who, false);
    let calls = h.fake_channel().calls().len();
    let rows = app.count("SELECT count(*) FROM outbound_message").await;
    let r = d.send(&c.who, None, &push, &th).await;
    assert!(matches!(r, Err(Error::Unreachable)), "{r:?}");
    assert_eq!(h.fake_channel().calls().len(), calls);
    assert_eq!(
        app.count("SELECT count(*) FROM outbound_message").await,
        rows
    );

    // Any Inbound brings it back: welcome back, Menu, no new code.
    let seen = c.press("hello.say").await;
    assert_eq!(text_of(&seen[0]), WELCOME_BACK);
    assert_eq!(text_of(&seen[1]), HELLO);
    assert!(matches!(c.menu(), Some(Some(_))));
    let link = app.query("SELECT status FROM link").await;
    assert_eq!(col(&link, "status").as_deref(), Some("active"));
    assert_eq!(h.emails().len(), emails);
    d.send(&c.who, None, &push, &th).await.unwrap();
}

#[tokio::test]
async fn a_blocked_account_is_skipped_by_pushes_until_it_writes_again() {
    let (h, app) = boot(FakeProfile::TelegramLike).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    let d = app.deployment();
    c.send(Inbound::Blocked {
        from: c.who.clone(),
    })
    .await;
    let r = d
        .send(&c.who, None, &Message::text("x"), &Locale::th())
        .await;
    assert!(matches!(r, Err(Error::Unreachable)));
    assert!(c.seen_since().is_empty());

    let seen = c.text("กลับมาแล้ว").await;
    assert_eq!(text_of(&seen[0]), WELCOME_BACK);
    d.send(&c.who, None, &Message::text("x"), &Locale::th())
        .await
        .unwrap();
}

#[tokio::test]
async fn staff_can_revoke_an_unreachable_link() {
    let (h, app) = boot(FakeProfile::LineLike).await;
    let staff = officer(&h).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    c.send(Inbound::Blocked {
        from: c.who.clone(),
    })
    .await;
    let id = live_link(&h, "u1").await;
    let done = links::revoke_by_staff(app.deployment(), &id, &staff, &RequestCtx::default())
        .await
        .unwrap();
    assert!(!done.notified, "the notice was skipped");
    assert!(c.seen_since().is_empty());
    let audit = app
        .query("SELECT before FROM audit_log WHERE action = 'link.revoke'")
        .await;
    assert_eq!(
        col(&audit, "before").as_deref(),
        Some(r#"{"status":"unreachable"}"#)
    );
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status = 'revoked'")
            .await,
        1
    );
    // Back later: an ordinary unlinked account, no welcome back.
    let seen = c.press("hello.say").await;
    assert_eq!(text_of(&seen[0]), "ขอบคุณครับ กรุณาพิมพ์รหัสนักศึกษาของคุณ");
}

#[tokio::test]
async fn a_manual_link_to_a_blocked_account_is_unreachable_until_it_writes() {
    let (h, app) = boot(FakeProfile::Full).await;
    let staff = officer(&h).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.follow().await;
    c.send(Inbound::Blocked {
        from: c.who.clone(),
    })
    .await;
    let done = links::manual_link(
        app.deployment(),
        "s3",
        "fake",
        "u1",
        &staff,
        &RequestCtx::default(),
    )
    .await
    .unwrap();
    assert!(!done.notified);
    let link = app.query("SELECT status FROM link").await;
    assert_eq!(col(&link, "status").as_deref(), Some("unreachable"));

    // Back: welcome back, and the Card still stands before the Menu.
    let seen = c.press("hello.say").await;
    assert_eq!(text_of(&seen[0]), WELCOME_BACK);
    assert!(matches!(&seen[1], Message::Card(_)), "{seen:?}");
    let link = app.query("SELECT status FROM link").await;
    assert_eq!(col(&link, "status").as_deref(), Some("active"));
}
