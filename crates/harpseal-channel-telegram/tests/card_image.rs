//! A Card image (#109) on Telegram: the large link preview above the
//! text, still one text message (so `editMessageText` replaces it). A
//! refused URL leaves it out.

use harpseal::render::RenderCx;
use harpseal::{Button, Card, CardImage, Locale, Message};
use harpseal_channel_telegram::render;
use serde_json::json;

const IMG: &str =
    "https://students.seal-bay.invalid/announcements/files/0123456789abcdef0123456789abcdef";

fn cx() -> RenderCx {
    RenderCx {
        locale: Locale::th(),
        mini_app_base_url: Some("https://dev.sealbay.invalid/tg".into()),
    }
}

fn card(body: &str, image: Option<CardImage>) -> Message {
    Message::Card(Card {
        title: "ย้ายห้องสอบ".into(),
        body: body.into(),
        buttons: vec![Button::mini_app("เปิดดูรายละเอียด", "/app/announcements/9")],
        image,
    })
}

#[test]
fn the_image_is_a_large_preview_above_the_text() {
    let img = CardImage::new(IMG, "ย้ายห้องสอบ").unwrap();
    let out = render::message(&card("ห้อง 3105", Some(img.clone())), &cx());
    assert_eq!(out.len(), 1, "one sendMessage, editable");
    assert_eq!(out[0]["method"], "sendMessage");
    assert_eq!(
        out[0]["link_preview_options"],
        json!({ "url": IMG, "prefer_large_media": true, "show_above_text": true })
    );
    // A body split in two: the preview stays on the first part.
    let long = format!("{}\n{}", "ก".repeat(3000), "ข".repeat(3000));
    let out = render::message(&card(&long, Some(img)), &cx());
    assert_eq!(out.len(), 2);
    assert_eq!(out[0]["link_preview_options"]["url"], IMG);
    assert!(out[1].get("link_preview_options").is_none());
}

#[test]
fn without_an_image_or_with_a_refused_one_nothing_changes() {
    let plain = render::message(&card("x", None), &cx());
    assert!(plain[0].get("link_preview_options").is_none());
    let mut img = CardImage::new(IMG, "").unwrap();
    img.url = "javascript:alert(1)".into();
    let out = render::message(&card("x", Some(img)), &cx());
    assert_eq!(out, plain);
}
