//! `Button::Url` (#108): an inline keyboard `url` button; a URL that is
//! not https is refused and a visible line is appended to the text. No
//! Action is dropped.

use harpseal::render::RenderCx;
use harpseal::{ActionId, Button, Card, Choices, Locale, Message};
use harpseal_channel_telegram::render::{self, URL_LABEL_MAX};
use serde_json::Value;

const URL: &str = "https://reg.seal-bay.invalid/exam?term=2";

fn cx() -> RenderCx {
    RenderCx {
        locale: Locale::th(),
        mini_app_base_url: Some("https://dev.sealbay.invalid/tg".into()),
    }
}

fn ack() -> Button {
    Button::action("รับทราบ", ActionId::new("news.ack.9").unwrap())
}

fn keyboard(v: &Value) -> Vec<&Value> {
    v["reply_markup"]["inline_keyboard"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| &row[0])
        .collect()
}

#[test]
fn card_url_buttons_are_url_buttons() {
    let long = "ตารางสอบปลายภาคเรียนที่ 2 ปีการศึกษา 2569 ทุกคณะ ฉบับปรับปรุงล่าสุด";
    let msg = Message::Card(Card {
        title: "ตารางสอบ <ด่วน>".into(),
        body: "ดูตารางสอบได้ที่ลิงก์ & ปุ่ม".into(),
        buttons: vec![
            ack(),
            Button::mini_app("เปิดดูรายละเอียด", "/app/announcements/9"),
            Button::url(long, URL).unwrap(),
        ],
    });
    let out = render::message(&msg, &cx());
    assert_eq!(out.len(), 1);
    assert_eq!(
        out[0]["text"],
        "<b>ตารางสอบ &lt;ด่วน&gt;</b>\nดูตารางสอบได้ที่ลิงก์ &amp; ปุ่ม"
    );
    let k = keyboard(&out[0]);
    assert_eq!(k.len(), 3);
    assert_eq!(k[0]["callback_data"], "news.ack.9");
    assert_eq!(k[2]["url"], URL);
    assert_eq!(
        k[2]["text"].as_str().unwrap().chars().count(),
        URL_LABEL_MAX
    );
}

#[test]
fn choices_carry_url_buttons() {
    let msg = Message::Choices(Choices {
        text: "เลือก".into(),
        options: vec![ack(), Button::url("เว็บคณะ", URL).unwrap()],
    });
    let out = render::message(&msg, &cx());
    let k = keyboard(&out[0]);
    assert_eq!(
        (k[1]["text"].as_str(), k[1]["url"].as_str()),
        (Some("เว็บคณะ"), Some(URL))
    );
}

#[test]
fn a_non_https_url_is_refused_visibly() {
    for bad in [
        "javascript:alert(1)",
        "http://reg.seal-bay.invalid/",
        "data:,x",
    ] {
        let raw = harpseal::fixtures::raw_url_button("ลิงก์อันตราย", bad);
        for msg in [
            Message::Card(Card {
                title: "t".into(),
                body: "b".into(),
                buttons: vec![ack(), raw.clone()],
            }),
            Message::Choices(Choices {
                text: "เลือก".into(),
                options: vec![ack(), raw.clone()],
            }),
        ] {
            let out = render::message(&msg, &cx());
            let all: String = out.iter().map(Value::to_string).collect();
            assert!(!all.contains(bad), "{bad} reached the payload");
            assert!(out[0]["text"].as_str().unwrap().contains("ลิงก์อันตราย"));
            let k = keyboard(&out[0]);
            assert_eq!(k.len(), 1, "only the Action button");
            assert_eq!(k[0]["callback_data"], "news.ack.9");
        }
    }
}
