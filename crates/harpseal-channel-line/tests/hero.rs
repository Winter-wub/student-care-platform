//! A Card image (#109) is the Flex bubble's hero: full width, 20:13,
//! cover. One message, so no extra quota; a refused URL leaves it out.

use harpseal::render::RenderCx;
use harpseal::{Button, Card, CardImage, CardList, Locale, Message};
use harpseal_channel_line::render::{self, FLEX_BODY_SOFT_MAX};
use serde_json::json;

const IMG: &str =
    "https://students.seal-bay.invalid/announcements/files/0123456789abcdef0123456789abcdef";

fn cx() -> RenderCx {
    RenderCx {
        locale: Locale::th(),
        mini_app_base_url: Some("https://liff.line.me/2000000000-SealBay0".into()),
    }
}

fn card(body: &str, image: Option<CardImage>) -> Card {
    Card {
        title: "ย้ายห้องสอบ".into(),
        body: body.into(),
        buttons: vec![Button::mini_app("เปิดดูรายละเอียด", "/app/announcements/9")],
        image,
    }
}

fn image() -> CardImage {
    CardImage::new(IMG, "ย้ายห้องสอบ").unwrap()
}

#[test]
fn the_image_is_the_bubble_hero() {
    let out = render::message(&Message::Card(card("ห้อง 3105", Some(image()))), &cx());
    assert_eq!(out.len(), 1, "one message");
    assert_eq!(
        out[0]["contents"]["hero"],
        json!({
            "type": "image", "url": IMG, "size": "full",
            "aspectRatio": "20:13", "aspectMode": "cover",
        })
    );
    // Without an image, the bubble is exactly as before.
    let plain = render::message(&Message::Card(card("ห้อง 3105", None)), &cx());
    assert!(plain[0]["contents"].get("hero").is_none());
    let mut with = out[0].clone();
    with["contents"].as_object_mut().unwrap().remove("hero");
    assert_eq!(with, plain[0]);
}

#[test]
fn a_long_body_keeps_the_hero_on_the_bubble_with_the_buttons() {
    let long = "ก".repeat(FLEX_BODY_SOFT_MAX + 1);
    let out = render::message(&Message::Card(card(&long, Some(image()))), &cx());
    assert_eq!(out[0]["type"], "text");
    let last = out.last().unwrap();
    assert_eq!(last["contents"]["hero"]["url"], IMG);
}

#[test]
fn carousel_cards_keep_their_heroes() {
    let list = CardList(vec![card("a", Some(image())), card("b", None)]);
    let out = render::message(&Message::CardList(list), &cx());
    let bubbles = out[0]["contents"]["contents"].as_array().unwrap();
    assert_eq!(bubbles[0]["hero"]["url"], IMG);
    assert!(bubbles[1].get("hero").is_none());
}

#[test]
fn a_refused_image_url_leaves_the_hero_out() {
    assert!(CardImage::new("http://students.seal-bay.invalid/x.png", "").is_err());
    for bad in [
        "javascript:alert(1)",
        "http://students.seal-bay.invalid/x.png",
        "https://students.seal-bay.invalid/\"onerror=\"x",
    ] {
        let mut img = image();
        img.url = bad.into();
        let out = render::message(&Message::Card(card("x", Some(img))), &cx());
        assert!(out[0]["contents"].get("hero").is_none(), "{bad}");
        assert!(!out[0].to_string().contains(bad), "{bad}");
    }
}
