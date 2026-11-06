//! Webhook calls: the secret token, each Update kind, commands as Actions,
//! callback answers and file downloads.

mod common;

use common::*;
use harpseal::{
    ActionId, Channel, ChannelAccountId, Error, Inbound, IncomingFile, Menu, MenuEntry,
};
use harpseal_channel_telegram::{DOWNLOAD_MAX, KIND};
use serde_json::json;

fn id(s: &str) -> ActionId {
    ActionId::new(s).unwrap()
}

fn news_menu() -> Menu {
    Menu {
        entries: vec![MenuEntry {
            label: "ประกาศล่าสุด".into(),
            action: id("news.latest"),
        }],
    }
}

#[tokio::test]
async fn a_missing_or_wrong_secret_is_bad_signature_before_parsing() {
    let (tg, http) = channel();
    let body = fixture("text").body;
    for req in [
        signed(&body, None),
        signed(&body, Some("wrong-secret")),
        signed(b"{not json", Some("wrong-secret")),
        signed(&body, Some("")),
    ] {
        assert!(matches!(
            tg.parse_webhook(req).await,
            Err(Error::BadSignature)
        ));
    }
    assert!(http.requests().is_empty());
}

#[tokio::test]
async fn a_good_secret_over_garbage_is_invalid() {
    let (tg, _) = channel();
    for body in [&b"{not json"[..], b"{}", b"[]"] {
        let got = tg.parse_webhook(signed(body, Some(SECRET))).await;
        assert!(matches!(got, Err(Error::Invalid(_))), "{got:?}");
    }
}

#[tokio::test]
async fn an_empty_configured_secret_refuses_everything() {
    let (tg, _) = channel_with(harpseal_channel_telegram::TelegramConfig::new(
        TOKEN, "", "",
    ));
    let got = tg
        .parse_webhook(signed(&fixture("text").body, Some("")))
        .await;
    assert!(matches!(got, Err(Error::BadSignature)));
}

#[tokio::test]
async fn each_fixture_becomes_its_event() {
    let (tg, _) = channel();
    let one = |evs: Vec<Inbound>| {
        assert_eq!(evs.len(), 1);
        evs.into_iter().next().unwrap()
    };
    let start = one(tg.parse_webhook(fixture("start")).await.unwrap());
    assert!(matches!(start, Inbound::Follow { from, reply: None } if from == chat()));
    match one(tg.parse_webhook(fixture("text")).await.unwrap()) {
        Inbound::Text { text, .. } => assert_eq!(text, "สอบถามค่าเทอมครับ"),
        other => panic!("{other:?}"),
    }
    match one(tg.parse_webhook(fixture("document")).await.unwrap()) {
        Inbound::File { file, .. } => {
            assert_eq!(file.platform_file_id, "BQACAgUAAxkBAAIBSealBayFileId");
            assert_eq!(file.file_name.as_deref(), Some("ใบลาป่วย.pdf"));
            assert_eq!(file.content_type.as_deref(), Some("application/pdf"));
            assert_eq!(file.size_bytes, Some(2138));
        }
        other => panic!("{other:?}"),
    }
    let kicked = one(tg.parse_webhook(fixture("kicked")).await.unwrap());
    assert!(matches!(kicked, Inbound::Blocked { .. }));
    let member = one(tg.parse_webhook(fixture("member")).await.unwrap());
    assert!(matches!(member, Inbound::Unblocked { .. }));
}

#[tokio::test]
async fn a_press_is_its_action_and_is_answered() {
    let (tg, http) = channel();
    let evs = tg.parse_webhook(fixture("callback_query")).await.unwrap();
    assert!(
        matches!(&evs[..], [Inbound::Action { action, .. }] if action.as_str() == "news.ack.1042")
    );
    assert_eq!(
        calls(&http),
        vec![(
            "answerCallbackQuery".to_string(),
            json!({ "callback_query_id": "4382bfdwdsb323b2d9" })
        )]
    );
}

#[tokio::test]
async fn a_failed_callback_answer_does_not_lose_the_press() {
    let (tg, http) = channel();
    http.respond_with(
        400,
        r#"{"ok":false,"error_code":400,"description":"query is too old"}"#,
    );
    let evs = tg.parse_webhook(fixture("callback_query")).await.unwrap();
    assert!(matches!(&evs[..], [Inbound::Action { .. }]));
}

#[tokio::test]
async fn a_command_in_the_menu_is_an_action_otherwise_text() {
    let (tg, _) = channel();
    let got = tg.parse_webhook(text_update("/news_latest")).await.unwrap();
    assert!(matches!(&got[..], [Inbound::Text { text, .. }] if text == "/news_latest"));

    tg.with_menu(&news_menu());
    for typed in ["/news_latest", "/news_latest@SealBayBot"] {
        let got = tg.parse_webhook(text_update(typed)).await.unwrap();
        assert!(
            matches!(&got[..], [Inbound::Action { action, .. }] if action.as_str() == "news.latest"),
            "{typed}: {got:?}"
        );
    }
    let got = tg.parse_webhook(text_update("/faq_list")).await.unwrap();
    assert!(matches!(&got[..], [Inbound::Text { .. }]));
}

#[tokio::test]
async fn set_menu_overlays_the_construction_time_map_per_account() {
    let (tg, _) = channel();
    let faq = Menu {
        entries: vec![MenuEntry {
            label: "คำถามที่พบบ่อย".into(),
            action: id("faq.list"),
        }],
    };
    tg.set_menu(&chat(), Some(&faq)).await.unwrap();
    let got = tg.parse_webhook(text_update("/faq_list")).await.unwrap();
    assert!(matches!(&got[..], [Inbound::Action { action, .. }] if action.as_str() == "faq.list"));
    let other = ChannelAccountId::new(KIND, "7099999999");
    assert_eq!(tg.command_action(&other, "faq_list"), None);
    // A clone (the handle given to the Deployment) shares the map.
    tg.clone().with_menu(&news_menu());
    assert_eq!(
        tg.command_action(&other, "news_latest")
            .map(|a| a.to_string()),
        Some("news.latest".into())
    );
}

#[tokio::test]
async fn start_with_a_payload_is_still_the_follow() {
    let (tg, _) = channel();
    let got = tg
        .parse_webhook(text_update("/start sealbay"))
        .await
        .unwrap();
    assert!(matches!(&got[..], [Inbound::Follow { .. }]));
}

fn file(size: Option<u64>) -> IncomingFile {
    IncomingFile {
        platform_file_id: "BQACAgUAAxkBAAIBSealBayFileId".into(),
        file_name: Some("ใบลาป่วย.pdf".into()),
        content_type: Some("application/pdf".into()),
        size_bytes: size,
    }
}

#[tokio::test]
async fn download_goes_through_get_file() {
    let (tg, http) = channel();
    http.respond_with(
        200,
        ok(json!({ "file_id": "x", "file_size": 3, "file_path": "documents/file_1.pdf" })),
    );
    http.respond_with(200, b"PDF".to_vec());
    assert_eq!(tg.download(&file(Some(3))).await.unwrap(), b"PDF");
    let reqs = http.requests();
    assert_eq!(
        reqs[0].uri,
        format!("https://api.telegram.org/bot{TOKEN}/getFile")
    );
    assert_eq!(
        body(&reqs[0]),
        json!({ "file_id": "BQACAgUAAxkBAAIBSealBayFileId" })
    );
    assert_eq!(reqs[1].method, http::Method::GET);
    assert_eq!(
        reqs[1].uri,
        format!("https://api.telegram.org/file/bot{TOKEN}/documents/file_1.pdf")
    );
}

#[tokio::test]
async fn files_over_20_mb_are_refused() {
    let (tg, http) = channel();
    let got = tg.download(&file(Some(DOWNLOAD_MAX + 1))).await;
    assert!(
        matches!(got, Err(Error::Invalid(m)) if m.contains("core.telegram.org/bots/api#getfile"))
    );
    assert!(http.requests().is_empty(), "refused before any call");
    http.respond_with(
        200,
        ok(json!({ "file_id": "x", "file_size": DOWNLOAD_MAX + 1, "file_path": "a" })),
    );
    assert!(matches!(
        tg.download(&file(None)).await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(
        http.requests().len(),
        1,
        "no download after getFile says too big"
    );
}

#[tokio::test]
async fn malformed_callback_data_is_answered_then_refused() {
    let (tg, http) = channel();
    let body = json!({ "update_id": 2, "callback_query": { "id": "bad-press",
        "from": { "id": 7_012_345_678_i64 }, "chat_instance": "-1", "data": "x".repeat(65),
        "message": { "message_id": 9, "date": 1, "chat": { "id": 7_012_345_678_i64, "type": "private" } } } });
    let got = tg
        .parse_webhook(signed(body.to_string().as_bytes(), Some(SECRET)))
        .await;
    assert!(matches!(got, Err(Error::Invalid(_))), "{got:?}");
    assert_eq!(
        calls(&http),
        vec![(
            "answerCallbackQuery".to_string(),
            json!({ "callback_query_id": "bad-press" })
        )]
    );
}
