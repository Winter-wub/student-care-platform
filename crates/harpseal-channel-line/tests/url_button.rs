//! `Button::Url` (#108): a Flex `uri` action and a quick-reply `uri`
//! action; a URL that is not https is refused and shows as a visible line,
//! never as a button. No Action is dropped.

use harpseal::render::{RenderCx, action_ids};
use harpseal::{ActionId, Button, Card, Choices, Locale, Message};
use harpseal_channel_line::render::{self, FLEX_BUTTON_LABEL_MAX, QUICK_REPLY_LABEL_MAX};
use serde_json::{Value, json};

const URL: &str = "https://reg.seal-bay.invalid/exam?term=2";

fn cx() -> RenderCx {
    RenderCx {
        locale: Locale::th(),
        mini_app_base_url: Some("https://liff.line.me/2000000000-SealBay0".into()),
    }
}

fn ack() -> Button {
    Button::action("รับทราบ", ActionId::new("news.ack.9").unwrap())
}

fn footer(v: &Value) -> &Vec<Value> {
    v["contents"]["footer"]["contents"].as_array().unwrap()
}

#[test]
fn card_url_buttons_are_uri_actions() {
    let long = "ตารางสอบปลายภาคเรียนที่ 2 ปีการศึกษา 2569 ทุกคณะ ฉบับปรับปรุงล่าสุด";
    let msg = Message::Card(Card {
        title: "ตารางสอบ".into(),
        body: "ดูตารางสอบได้ที่ลิงก์".into(),
        buttons: vec![
            ack(),
            Button::mini_app("เปิดดูรายละเอียด", "/app/announcements/9"),
            Button::url(long, URL).unwrap(),
        ],
    });
    let out = render::message(&msg, &cx());
    assert_eq!(out.len(), 1);
    let f = footer(&out[0]);
    assert_eq!(f.len(), 3);
    let url = &f[2]["action"];
    assert_eq!(url["type"], "uri");
    assert_eq!(url["uri"], URL);
    let label = url["label"].as_str().unwrap();
    assert_eq!(label.chars().count(), FLEX_BUTTON_LABEL_MAX);
    assert!(label.ends_with('…'));
    assert_eq!(f[0]["action"]["data"], json!("news.ack.9"));
}

#[test]
fn quick_reply_url_is_a_uri_item() {
    let msg = Message::Choices(Choices {
        text: "เลือก".into(),
        options: vec![ack(), Button::url("เว็บคณะ", URL).unwrap()],
    });
    let out = render::message(&msg, &cx());
    let items = out[0]["quickReply"]["items"].as_array().unwrap();
    assert_eq!(items[1]["action"]["type"], "uri");
    assert_eq!(items[1]["action"]["uri"], URL);
    assert!(
        items[1]["action"]["label"]
            .as_str()
            .unwrap()
            .chars()
            .count()
            <= QUICK_REPLY_LABEL_MAX
    );
}

#[test]
fn a_non_https_url_is_refused_visibly() {
    for bad in [
        "javascript:alert(1)",
        "http://reg.seal-bay.invalid/",
        "data:,x",
    ] {
        // Only the Core can build one by hand; Button::url refuses it.
        assert!(Button::url("x", bad).is_err());
        let raw = raw_url("ลิงก์อันตราย", bad);
        let msg = Message::Card(Card {
            title: "t".into(),
            body: "b".into(),
            buttons: vec![ack(), raw.clone()],
        });
        let out = render::message(&msg, &cx());
        let text = out[0].to_string();
        assert!(!text.contains(bad), "{bad} reached the payload");
        assert!(
            text.contains("(ลิงก์ “ลิงก์อันตราย” เปิดไม่ได้)"),
            "its own visible line stands in"
        );
        assert_eq!(action_ids(&msg), vec!["news.ack.9".to_string()]);
        assert_eq!(footer(&out[0])[0]["action"]["data"], "news.ack.9");
        let msg = Message::Choices(Choices {
            text: "เลือก".into(),
            options: vec![ack(), raw],
        });
        let out = render::message(&msg, &cx())
            .iter()
            .map(Value::to_string)
            .collect::<String>();
        assert!(!out.contains(bad) && out.contains("news.ack.9"));
    }
}

/// A `Button::Url` with any URL, as only code inside the Core could build
/// it (the variant is `#[non_exhaustive]`: no struct syntax out here).
fn raw_url(label: &str, url: &str) -> Button {
    harpseal::fixtures::raw_url_button(label, url)
}

#[test]
fn format_characters_leave_url_labels() {
    let raw = harpseal::fixtures::raw_url_button("\u{202E}ตาราง\u{200B}สอบ", URL);
    let msg = Message::Card(Card {
        title: "t".into(),
        body: "b".into(),
        buttons: vec![raw],
    });
    let out = render::message(&msg, &cx());
    assert_eq!(footer(&out[0])[0]["action"]["label"], "ตารางสอบ");
}
