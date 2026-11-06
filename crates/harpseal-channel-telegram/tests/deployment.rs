//! The Telegram adapter at the Deployment seam: a real webhook call through
//! the Router, the Kernel's send path and its Outbound Message rows, with
//! every Bot API call caught by the harness's `RecordingHttp`.

use std::sync::Arc;

use axum::body::Body;
use harpseal::{
    ActionContext, ActionId, CatalogEntry, Locale, MenuEntrySpec, Message, Module, ModuleRegistrar,
    Statement,
};
use harpseal_channel_telegram::{SECRET_HEADER, TelegramChannel, TelegramConfig};
use harpseal_testing::{Harness, TestApp, TestResponse};
use serde_json::json;

const SECRET: &str = "seal-bay-webhook-secret";
const CHAT: i64 = 7_012_345_678;

/// Answers `news.latest` with one Text; on the Menu or as a plain Action.
struct News {
    on_menu: bool,
}

fn latest() -> ActionId {
    ActionId::new("news.latest").unwrap()
}

impl Module for News {
    fn name(&self) -> &'static str {
        "news"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        let h = |_: ActionContext| async move {
            Ok(vec![Message::text("ประกาศล่าสุด: ปิดอาคารเรียนรวม 2")])
        };
        r.messages(&[
            CatalogEntry::new("news.menu.latest", "th", "ประกาศล่าสุด"),
            CatalogEntry::new("news.menu.latest", "en", "Latest news"),
        ]);
        if self.on_menu {
            let spec = MenuEntrySpec {
                label_key: "news.menu.latest",
                action: latest(),
                order: 1,
            };
            r.menu_entry(spec, h);
        } else {
            r.action(latest(), h);
        }
    }
}

/// A Deployment with the News Module and the Telegram adapter, whose
/// command map is the Deployment's Menu (as the README shows).
async fn boot(on_menu: bool, linked: bool) -> (Harness, TestApp) {
    let h = Harness::new();
    let cfg = TelegramConfig::new("8000000001:SEALBAY", SECRET, "");
    let tg = TelegramChannel::new(cfg, Arc::new(h.http().clone()));
    let d = h
        .builder()
        .channel(tg.clone())
        .module(News { on_menu })
        .build()
        .unwrap();
    tg.with_menu(&d.menu_for(&Locale::th()));
    let app = h.boot(d).await;
    if linked {
        for sql in [
            "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
             VALUES ('s1', '68010004', 'Karn Bai-ruea', 'th', 1, 1)",
            "INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) \
             VALUES ('a1', 'telegram', '7012345678', 1, 1)",
            "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, linked_at, updated_at) \
             VALUES ('l1', 's1', 'a1', 'telegram', 'active', 'email_otp', 1, 1)",
        ] {
            h.db().execute(Statement::new(sql)).await.unwrap();
        }
    }
    (h, app)
}

async fn post_text(app: &TestApp, text: &str) -> TestResponse {
    let body = json!({ "update_id": 900_000_010, "message": {
        "message_id": 10, "date": 1_793_836_800,
        "from": { "id": CHAT, "is_bot": false, "first_name": "Karn" },
        "chat": { "id": CHAT, "type": "private" }, "text": text } });
    let req = http::Request::post("/webhook/telegram")
        .header(SECRET_HEADER, SECRET)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    app.request(req).await
}

fn send_messages(h: &Harness) -> Vec<serde_json::Value> {
    h.http_requests()
        .iter()
        .filter(|r| r.uri.ends_with("/sendMessage"))
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

const OUTBOUND: &str = "SELECT count(*) FROM outbound_message";

#[tokio::test]
async fn a_text_update_is_accepted_and_answers_nothing_yet() {
    // The Kernel answers free text only with the chat flow (#19).
    let (h, app) = boot(true, true).await;
    assert_eq!(post_text(&app, "สอบถามค่าเทอมครับ").await.status, 200);
    assert!(h.http_requests().is_empty());
    assert_eq!(app.count(OUTBOUND).await, 0);
}

#[tokio::test]
async fn a_menu_command_from_a_linked_student_runs_its_action() {
    let (h, app) = boot(true, true).await;
    assert_eq!(post_text(&app, "/news_latest").await.status, 200);
    let sent = send_messages(&h);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["chat_id"], "7012345678");
    assert_eq!(sent[0]["text"], "ประกาศล่าสุด: ปิดอาคารเรียนรวม 2");
    let rows = app
        .query("SELECT channel, kind, metering, module FROM outbound_message")
        .await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows.get(0, "channel").unwrap().as_text(), Some("telegram"));
    assert_eq!(rows.get(0, "metering").unwrap().as_text(), Some("free"));
    assert_eq!(rows.get(0, "module").unwrap().as_text(), Some("news"));
    let usage = app
        .query("SELECT metered, free FROM usage_daily WHERE channel = 'telegram'")
        .await;
    assert_eq!(usage.get(0, "free").unwrap().as_i64(), Some(1));
    assert_eq!(usage.get(0, "metered").unwrap().as_i64(), Some(0));
}

#[tokio::test]
async fn a_command_not_on_the_menu_stays_text() {
    let (h, app) = boot(false, true).await;
    assert_eq!(post_text(&app, "/news_latest").await.status, 200);
    assert!(send_messages(&h).is_empty());
    assert_eq!(app.count(OUTBOUND).await, 0);
}

#[tokio::test]
async fn an_unlinked_student_is_asked_to_link_first() {
    let (h, app) = boot(true, false).await;
    assert_eq!(post_text(&app, "/news_latest").await.status, 200);
    let sent = send_messages(&h);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["text"], "กรุณายืนยันตัวตนก่อนใช้งาน");
    assert_eq!(app.count(OUTBOUND).await, 1);
}

#[tokio::test]
async fn a_bad_secret_is_401_and_garbage_is_400() {
    let (h, app) = boot(true, true).await;
    let req = http::Request::post("/webhook/telegram")
        .header(SECRET_HEADER, "wrong")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(app.request(req).await.status, 401);
    let req = http::Request::post("/webhook/telegram")
        .header(SECRET_HEADER, SECRET)
        .body(Body::from("{not json"))
        .unwrap();
    assert_eq!(app.request(req).await.status, 400);
    assert!(h.http_requests().is_empty());
}
