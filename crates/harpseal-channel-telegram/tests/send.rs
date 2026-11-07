//! Sending, the Menu, edit, delete and the Mini App through a
//! `RecordingHttp`: the request JSON per degrade rule and the error
//! mapping.

mod common;

use std::sync::Arc;

use common::*;
use harpseal::fixtures;
use harpseal::render::action_ids;
use harpseal::{
    ActionId, Button, Capability, Card, Channel, Choices, Delivery, Error, Locale, Menu, MenuEntry,
    Message, Metering, PlatformMessageId,
};
use harpseal_channel_telegram::TelegramConfig;
use harpseal_testing::ManualClock;
use serde_json::{Value, json};

fn id(s: &str) -> ActionId {
    ActionId::new(s).unwrap()
}

async fn send(msg: &Message) -> Vec<(String, Value)> {
    let (tg, http) = channel();
    let r = tg
        .send(&chat(), &Delivery::Push, msg, &Locale::th())
        .await
        .unwrap();
    assert_eq!(r.metering, Metering::Free);
    assert!(r.pushed);
    calls(&http)
}

fn callback_data(calls: &[(String, Value)]) -> Vec<String> {
    let mut out = Vec::new();
    for (_, b) in calls {
        for row in b["reply_markup"]["inline_keyboard"]
            .as_array()
            .into_iter()
            .flatten()
        {
            for btn in row.as_array().unwrap() {
                if let Some(d) = btn["callback_data"].as_str() {
                    out.push(d.to_string());
                }
            }
        }
    }
    out
}

#[tokio::test]
async fn a_text_is_one_plain_send_message_with_the_message_id() {
    let (tg, http) = channel();
    http.respond_with(
        200,
        ok(json!({ "message_id": 77, "chat": { "id": 7_012_345_678_i64 } })),
    );
    let r = tg
        .send(
            &chat(),
            &Delivery::Push,
            &Message::text("a < b"),
            &Locale::th(),
        )
        .await
        .unwrap();
    assert_eq!(r.message_id, Some(PlatformMessageId("77".into())));
    assert_eq!(
        calls(&http),
        vec![(
            "sendMessage".into(),
            json!({ "chat_id": CHAT, "text": "a < b" })
        )]
    );
}

#[tokio::test]
async fn a_card_is_escaped_html_with_a_keyboard() {
    let card = Message::Card(Card {
        title: "ปิด <อาคาร> & ลานจอด".into(),
        body: "วันที่ 5 > 4".into(),
        buttons: vec![Button::action("รับทราบ", id("news.ack.1042"))],
    });
    let sent = send(&card).await;
    assert_eq!(sent.len(), 1);
    let b = &sent[0].1;
    assert_eq!(b["parse_mode"], "HTML");
    assert_eq!(
        b["text"],
        "<b>ปิด &lt;อาคาร&gt; &amp; ลานจอด</b>\nวันที่ 5 &gt; 4"
    );
    assert_eq!(
        b["reply_markup"],
        json!({ "inline_keyboard": [[{ "text": "รับทราบ", "callback_data": "news.ack.1042" }]] })
    );
}

#[tokio::test]
async fn a_card_list_is_one_message_per_card() {
    let sent = send(&fixtures::news_list_15()).await;
    assert_eq!(sent.len(), 15);
    assert!(sent.iter().all(|(m, _)| m == "sendMessage"));
    assert_eq!(callback_data(&sent), action_ids(&fixtures::news_list_15()));
}

#[tokio::test]
async fn long_text_splits_with_the_keyboard_on_the_last_part() {
    let msg = Message::Choices(Choices {
        text: "ข้อความยาว ".repeat(600),
        options: vec![Button::action("ตกลง", id("contract.ok"))],
    });
    let sent = send(&msg).await;
    assert_eq!(sent.len(), 2);
    assert!(sent[0].1.get("reply_markup").is_none());
    assert_eq!(callback_data(&sent[1..]), vec!["contract.ok"]);
    assert!(
        sent.iter()
            .all(|(_, b)| b["text"].as_str().unwrap().chars().count() <= 4096)
    );
}

#[tokio::test]
async fn over_100_buttons_go_on_follow_ups_and_none_is_dropped() {
    let msg = fixtures::degrade_choices_120();
    let sent = send(&msg).await;
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].1["text"], "ตัวเลือกเพิ่มเติม (2/2)");
    assert_eq!(callback_data(&sent), action_ids(&msg));
}

#[tokio::test]
async fn a_mini_app_button_is_a_web_app_or_a_notice() {
    let (tg, http) = web_app_channel(TelegramConfig::new(TOKEN, SECRET, WEB_APP));
    let card = fixtures::news_card();
    tg.send(&chat(), &Delivery::Push, &card, &Locale::th())
        .await
        .unwrap();
    let sent = calls(&http);
    let kb = &sent[0].1["reply_markup"]["inline_keyboard"];
    assert_eq!(kb[1][0]["web_app"]["url"], format!("{WEB_APP}/news/1042"));
    let sent = send(&card).await;
    assert!(
        sent[0].1["text"]
            .as_str()
            .unwrap()
            .contains("ยังเปิดในช่องทางนี้ไม่ได้")
    );
    assert_eq!(callback_data(&sent), action_ids(&card));
}

#[tokio::test]
async fn attachments_are_not_offered() {
    let (tg, http) = channel();
    assert!(tg.attachments().is_none());
    let got = tg
        .send(
            &chat(),
            &Delivery::Push,
            &fixtures::degrade_pdf_30mb(),
            &Locale::th(),
        )
        .await;
    assert!(matches!(
        got,
        Err(Error::Unsupported(Capability::SendAttachment))
    ));
    assert!(http.requests().is_empty());
}

#[tokio::test]
async fn forbidden_is_unreachable_and_flood_control_is_rate_limited() {
    let (tg, http) = channel();
    let hi = Message::text("สวัสดี");
    http.respond_with(
        403,
        r#"{"ok":false,"error_code":403,"description":"Forbidden: bot was blocked by the user"}"#,
    );
    let got = tg.send(&chat(), &Delivery::Push, &hi, &Locale::th()).await;
    assert!(matches!(got, Err(Error::Unreachable)), "{got:?}");
    http.respond_with(429, r#"{"ok":false,"error_code":429,"description":"Too Many Requests: retry after 12","parameters":{"retry_after":12}}"#);
    let got = tg.send(&chat(), &Delivery::Push, &hi, &Locale::th()).await;
    assert!(
        matches!(
            got,
            Err(Error::RateLimited {
                retry_after_secs: 12
            })
        ),
        "{got:?}"
    );
    http.respond_with(500, "oops");
    let got = tg.send(&chat(), &Delivery::Push, &hi, &Locale::th()).await;
    assert!(matches!(got, Err(Error::Platform(_))), "{got:?}");
}

fn news_menu() -> Menu {
    Menu::new(vec![MenuEntry {
        label: "ประกาศล่าสุด".into(),
        action: id("news.latest"),
    }])
}

#[tokio::test]
async fn set_menu_sets_chat_commands_and_the_commands_button() {
    let (tg, http) = channel();
    tg.set_menu(&chat(), Some(&news_menu())).await.unwrap();
    assert_eq!(
        calls(&http),
        vec![
            (
                "setMyCommands".into(),
                json!({
                "commands": [{ "command": "news_latest", "description": "ประกาศล่าสุด" }],
                "scope": { "type": "chat", "chat_id": CHAT } })
            ),
            (
                "setChatMenuButton".into(),
                json!({
                "chat_id": CHAT, "menu_button": { "type": "commands" } })
            ),
        ]
    );
}

#[tokio::test]
async fn set_menu_uses_the_web_app_button_when_configured() {
    let cfg = TelegramConfig::new(TOKEN, SECRET, WEB_APP).with_menu_button_text("Seal Bay");
    let (tg, http) = web_app_channel(cfg);
    tg.set_menu(&chat(), Some(&news_menu())).await.unwrap();
    let sent = calls(&http);
    assert_eq!(
        sent[1].1["menu_button"],
        json!({ "type": "web_app", "text": "Seal Bay", "web_app": { "url": WEB_APP } })
    );
}

#[tokio::test]
async fn the_locked_menu_is_start_only() {
    let (tg, http) = channel_with(TelegramConfig::new(TOKEN, SECRET, WEB_APP));
    tg.set_menu(&chat(), None).await.unwrap();
    assert_eq!(
        calls(&http),
        vec![
            (
                "setMyCommands".into(),
                json!({
                "commands": [{ "command": "start", "description": "เริ่มใช้งาน" }],
                "scope": { "type": "chat", "chat_id": CHAT } })
            ),
            (
                "setChatMenuButton".into(),
                json!({
                "chat_id": CHAT, "menu_button": { "type": "commands" } })
            ),
        ]
    );
}

#[tokio::test]
async fn edit_replaces_the_text_and_keyboard() {
    let (tg, http) = channel();
    let editor = tg.editor().unwrap();
    let msg = Message::Choices(Choices {
        text: "รับทราบแล้ว".into(),
        options: vec![Button::action("ดูอีกครั้ง", id("news.open.1042"))],
    });
    editor
        .edit(
            &chat(),
            &PlatformMessageId("77".into()),
            &msg,
            &Locale::th(),
        )
        .await
        .unwrap();
    let sent = calls(&http);
    assert_eq!(sent[0].0, "editMessageText");
    assert_eq!(sent[0].1["message_id"], 77);
    assert_eq!(sent[0].1["chat_id"], CHAT);
    assert_eq!(sent[0].1["text"], "รับทราบแล้ว");
    assert_eq!(callback_data(&sent), vec!["news.open.1042"]);
    let long = fixtures::degrade_long_text();
    let got = editor
        .edit(
            &chat(),
            &PlatformMessageId("77".into()),
            &long,
            &Locale::th(),
        )
        .await;
    assert!(matches!(got, Err(Error::Invalid(_))));
    let got = editor
        .edit(&chat(), &PlatformMessageId("x".into()), &msg, &Locale::th())
        .await;
    assert!(matches!(got, Err(Error::Invalid(_))));
}

#[tokio::test]
async fn delete_within_48_hours_or_unsupported() {
    let (tg, http) = channel();
    let deleter = tg.deleter().unwrap();
    deleter
        .delete(&chat(), &PlatformMessageId("77".into()))
        .await
        .unwrap();
    assert_eq!(
        calls(&http),
        vec![(
            "deleteMessage".into(),
            json!({ "chat_id": CHAT, "message_id": 77 })
        )]
    );
    http.respond_with(
        400,
        r#"{"ok":false,"error_code":400,"description":"Bad Request: message can't be deleted"}"#,
    );
    let got = deleter
        .delete(&chat(), &PlatformMessageId("12".into()))
        .await;
    assert!(
        matches!(got, Err(Error::Unsupported(Capability::DeleteMessage))),
        "{got:?}"
    );
}

#[tokio::test]
async fn mini_app_only_with_a_web_app_url_and_a_clock() {
    let (tg, _) = channel();
    assert!(tg.mini_app().is_none());
    assert!(!tg.capabilities().mini_app);
    // A URL without a Clock is a missing Capability, and renders no
    // web_app button or menu button.
    let (tg, http) = channel_with(TelegramConfig::new(TOKEN, SECRET, WEB_APP));
    assert!(tg.mini_app().is_none());
    assert!(!tg.capabilities().mini_app);
    tg.set_menu(&chat(), Some(&news_menu())).await.unwrap();
    assert_eq!(calls(&http)[1].1["menu_button"]["type"], "commands");
    let (tg, _) = web_app_channel(TelegramConfig::new(TOKEN, SECRET, WEB_APP));
    assert!(tg.capabilities().mini_app);
    let app = tg.mini_app().unwrap();
    assert_eq!(app.launch_url("/news/1042"), format!("{WEB_APP}/news/1042"));
    assert_eq!(app.web_client(), Some(harpseal::WebClient::Telegram));
}

#[tokio::test]
async fn mini_app_sessions_check_init_data_with_the_clock() {
    // The vector from `web_app::tests`, signed with this token.
    const TOKEN: &str = "8000000001:AAFsealBayTestTokenNotReal0000000000";
    let init = "auth_date=1793836800&query_id=AAHdF6IQAAAAAN0XohDhrOrc\
        &user=%7B%22id%22%3A7012345678%2C%22first_name%22%3A%22Karn%22%2C%22language_code%22%3A%22th%22%7D\
        &hash=2e9f10864e5d25e3ed0571951dab659b75948576b076b8d92fc2b059b003b1b1";
    let cfg = TelegramConfig::new(TOKEN, SECRET, WEB_APP);
    let clock = ManualClock::new(1_793_836_800_000 + 60_000);
    let (tg, _) = channel_with(cfg);
    let tg = tg.with_clock(Arc::new(clock.clone()));
    let app = tg.mini_app().unwrap();
    assert_eq!(app.verify_session(init).await.unwrap(), chat());
    clock.advance_minutes(120);
    assert!(matches!(
        app.verify_session(init).await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test]
async fn an_unchanged_edit_and_a_vanished_message_are_done() {
    let (tg, http) = channel();
    http.respond_with(400, r#"{"ok":false,"error_code":400,"description":"Bad Request: message is not modified: specified new message content and reply markup are exactly the same as a current content and reply markup of the message"}"#);
    let msg = Message::text("รับทราบแล้ว");
    let id = PlatformMessageId("77".into());
    let editor = tg.editor().unwrap();
    editor
        .edit(&chat(), &id, &msg, &Locale::th())
        .await
        .unwrap();
    http.respond_with(
        400,
        r#"{"ok":false,"error_code":400,"description":"Bad Request: message to delete not found"}"#,
    );
    tg.deleter().unwrap().delete(&chat(), &id).await.unwrap();
}
