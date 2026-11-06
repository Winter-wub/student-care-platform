//! Unreachable Links (#20): a send refused as unreachable marks the Link,
//! Pushes skip a blocked account, and its next Inbound welcomes it back.

mod support;

use harpseal::{Error, Inbound, Locale, Message};
use harpseal_channel_fake::FakeProfile;
use support::{Chat, boot, text_of};

const HELLO: &str = "สวัสดีจากโมดูล";
const WELCOME_BACK: &str = "ยินดีต้อนรับกลับมา บัญชีของคุณยังเชื่อมอยู่ ใช้เมนูได้ตามปกติ";

fn text(rows: &harpseal::Rows, col: &str) -> Option<String> {
    rows.get(0, col)
        .and_then(|v| v.as_text())
        .map(str::to_string)
}

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
    assert_eq!(text(&link, "status").as_deref(), Some("unreachable"));

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
    assert_eq!(text(&link, "status").as_deref(), Some("active"));
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
