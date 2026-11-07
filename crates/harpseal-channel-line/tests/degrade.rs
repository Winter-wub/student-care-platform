//! The LINE degrade rules of `docs/prototypes/message-render.md` that
//! `render.rs` does not already cover: a quick reply label over 20, a Card
//! body over 2000 (alone and in a carousel), altText over 1500 and a Menu
//! over 20 entries. No Action is ever dropped.

use harpseal::render::{MENU_MORE_ACTION, RenderCx, action_ids, char_len};
use harpseal::{ActionId, Button, Card, CardList, Choices, Locale, Menu, MenuEntry, Message};
use harpseal_channel_line::render::{self, *};
use serde_json::Value;

fn cx() -> RenderCx {
    RenderCx {
        locale: Locale::th(),
        mini_app_base_url: None,
    }
}

fn id(s: &str) -> ActionId {
    ActionId::new(s).unwrap()
}

fn data(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            if let Some(d) = o.get("data").and_then(Value::as_str) {
                out.push(d.into());
            }
            o.values().for_each(|x| data(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| data(x, out)),
        _ => {}
    }
}

fn all_data(msgs: &[Value]) -> Vec<String> {
    let mut out = Vec::new();
    msgs.iter().for_each(|m| data(m, &mut out));
    out
}

fn len(v: &Value) -> usize {
    char_len(v.as_str().unwrap())
}

#[test]
fn a_quick_reply_label_over_20_turns_the_choices_into_a_bubble() {
    let msg = Message::Choices(Choices {
        text: "เลือกคำร้อง".into(),
        options: vec![
            Button::action("ลา", id("form.leave")),
            Button::action("ขอเอกสารรับรองสถานะการเป็นนักศึกษา", id("form.cert")),
        ],
    });
    let out = render::message(&msg, &cx());
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["type"], "flex");
    assert!(out[0].get("quickReply").is_none());
    assert_eq!(out[0]["altText"], "เลือกคำร้อง");
    assert_eq!(all_data(&out), action_ids(&msg));
    let label = &out[0]["contents"]["footer"]["contents"][1]["action"]["label"];
    assert_eq!(len(label), 34, "a bubble button keeps up to 40 chars");
}

#[test]
fn a_card_body_over_2000_goes_first_as_text() {
    let body = "ก".repeat(FLEX_BODY_SOFT_MAX + 1);
    let msg = Message::Card(Card {
        title: "ระเบียบการแต่งกาย".into(),
        body: body.clone(),
        buttons: vec![Button::action("รับทราบ", id("news.ack.7"))],
    });
    let out = render::message(&msg, &cx());
    assert_eq!(out.len(), 2);
    assert_eq!(out[0]["type"], "text");
    assert!(out[0]["text"].as_str().unwrap().ends_with(&body));
    assert_eq!(out[1]["type"], "flex");
    let bubble = out[1]["contents"]["body"]["contents"].as_array().unwrap();
    assert_eq!(bubble.len(), 1, "the bubble keeps the title only");
    assert_eq!(all_data(&out), action_ids(&msg));
}

#[test]
fn a_card_body_in_a_carousel_is_cut_to_2000() {
    let card = Card {
        title: "ประกาศ".into(),
        body: "ข".repeat(3000),
        buttons: vec![Button::action("รับทราบ", id("news.ack.8"))],
    };
    let msg = Message::CardList(CardList(vec![card.clone(), card]));
    let out = render::message(&msg, &cx());
    assert_eq!(out.len(), 1);
    let body = &out[0]["contents"]["contents"][0]["body"]["contents"][1]["text"];
    assert_eq!(len(body), FLEX_BODY_SOFT_MAX);
    assert!(body.as_str().unwrap().ends_with('…'));
    assert_eq!(all_data(&out).len(), 2);
}

#[test]
fn alt_text_is_cut_to_1500() {
    let msg = Message::Card(Card {
        title: "ค".repeat(2000),
        body: "สั้น".into(),
        buttons: vec![],
    });
    let out = render::message(&msg, &cx());
    assert_eq!(len(&out[0]["altText"]), ALT_TEXT_MAX);
    assert!(out[0]["altText"].as_str().unwrap().ends_with('…'));
}

#[test]
fn a_menu_over_20_entries_keeps_19_and_a_more_entry() {
    let menu = Menu::new(
        (0..25)
            .map(|i| MenuEntry {
                label: format!("เมนู {i}"),
                action: id(&format!("menu.{i}")),
            })
            .collect(),
    );
    let v = render::menu(&menu);
    let areas = v["areas"].as_array().unwrap();
    assert_eq!(areas.len(), RICH_MENU_AREAS_MAX);
    assert_eq!(areas[18]["action"]["data"], "menu.18");
    assert_eq!(areas[19]["action"]["data"], MENU_MORE_ACTION);
    assert_eq!(areas[19]["action"]["label"], "เพิ่มเติม");
}

#[test]
fn the_sized_menu_scales_the_grid_to_the_image() {
    let menu = harpseal::fixtures::menu_unlocked();
    let v = render::menu_sized(&menu, 2500, 843);
    assert_eq!(v["size"]["height"], 843);
    for a in v["areas"].as_array().unwrap() {
        let b = &a["bounds"];
        let bottom = b["y"].as_u64().unwrap() + b["height"].as_u64().unwrap();
        assert!(bottom <= 843);
    }
    assert_eq!(render::menu(&menu)["size"]["height"], 1686);
}
