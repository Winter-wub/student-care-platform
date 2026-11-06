//! Renders every first-slice fixture, checks LINE's limits and compares
//! the JSON with the committed snapshot
//! `docs/prototypes/message-render/line/<fixture>.json` (read-only here;
//! print the output with `cargo test -- --nocapture`).

use harpseal::fixtures::{self, Sample};
use harpseal::render::{RenderCx, action_ids, char_len};
use harpseal::{Locale, Message};
use harpseal_channel_line::render::{self, *};
use serde_json::Value;

fn cx() -> RenderCx {
    RenderCx {
        locale: Locale::th(),
        mini_app_base_url: Some("https://liff.line.me/2000000000-SealBay0".into()),
    }
}

fn no_mini_app() -> RenderCx {
    RenderCx {
        mini_app_base_url: None,
        ..cx()
    }
}

/// Every value under `key` anywhere in `v`, in document order.
fn find<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                if k == key {
                    out.push(x);
                }
                find(x, key, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| find(x, key, out)),
        _ => {}
    }
}

fn all<'a>(v: &'a Value, key: &str) -> Vec<&'a Value> {
    let mut out = Vec::new();
    find(v, key, &mut out);
    out
}

fn len(v: &Value) -> usize {
    char_len(v.as_str().expect("string"))
}

/// The LINE limits every rendered message object must meet.
fn assert_limits(m: &Value) {
    match m["type"].as_str().unwrap() {
        "text" => assert!(len(&m["text"]) <= TEXT_MAX),
        "flex" => {
            assert!(len(&m["altText"]) <= ALT_TEXT_MAX);
            let c = &m["contents"];
            if c["type"] == "carousel" {
                assert!(c["contents"].as_array().unwrap().len() <= CAROUSEL_MAX);
                assert!(c.to_string().len() <= CAROUSEL_JSON_MAX);
            } else {
                assert!(c.to_string().len() <= BUBBLE_JSON_MAX);
            }
            for b in all(m, "action") {
                assert!(len(&b["label"]) <= FLEX_BUTTON_LABEL_MAX);
            }
        }
        "image" => {}
        t => panic!("unexpected message type {t}"),
    }
    if let Some(items) = m.get("quickReply").map(|q| q["items"].as_array().unwrap()) {
        assert!(items.len() <= QUICK_REPLY_MAX);
        for i in items {
            assert!(len(&i["action"]["label"]) <= QUICK_REPLY_LABEL_MAX);
        }
    }
    for d in all(m, "data") {
        assert!(len(d) <= POSTBACK_DATA_MAX);
    }
}

/// The rendered JSON equals the committed snapshot.
fn snapshot(name: &str, v: &Value) {
    let pretty = serde_json::to_string_pretty(v).unwrap();
    println!("── line/{name}.json ──\n{pretty}");
    let path = format!(
        "{}/../../docs/prototypes/message-render/line/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("snapshot {path} is missing: {e}"));
    let committed: Value = serde_json::from_str(&committed).expect("snapshot is JSON");
    assert_eq!(v, &committed, "{name}: differs from {path}");
}

#[test]
fn renders_every_fixture_within_limits() {
    for (name, sample) in fixtures::all() {
        let out = match &sample {
            Sample::Message(m) => {
                let msgs = render::message(m, &cx());
                assert!(!msgs.is_empty(), "{name}: nothing rendered");
                msgs.iter().for_each(assert_limits);
                // No Action is dropped or duplicated.
                let data: Vec<String> = msgs
                    .iter()
                    .flat_map(|v| all(v, "data"))
                    .map(|d| d.as_str().unwrap().to_string())
                    .collect();
                assert_eq!(data, action_ids(m), "{name}: Actions differ");
                Value::Array(msgs)
            }
            Sample::Menu(m) => {
                let v = render::menu(m);
                let areas = v["areas"].as_array().unwrap();
                assert_eq!(areas.len(), m.entries.len());
                assert!(areas.len() <= RICH_MENU_AREAS_MAX);
                assert!(len(&v["chatBarText"]) <= CHAT_BAR_TEXT_MAX);
                for a in areas {
                    assert!(len(&a["action"]["label"]) <= RICH_MENU_LABEL_MAX);
                    let b = &a["bounds"];
                    assert!(b["x"].as_u64().unwrap() + b["width"].as_u64().unwrap() <= 2500);
                }
                let (w, h) = (
                    v["size"]["width"].as_f64().unwrap(),
                    v["size"]["height"].as_f64().unwrap(),
                );
                assert!(w / h >= 1.45, "rich menu aspect ratio");
                v
            }
            _ => panic!("{name}: a Sample kind this test does not know"),
        };
        snapshot(name, &out);
    }
}

#[test]
fn card_list_of_15_splits_into_12_and_3() {
    let msgs = render::message(&fixtures::news_list_15(), &cx());
    let sizes: Vec<usize> = msgs
        .iter()
        .map(|m| m["contents"]["contents"].as_array().unwrap().len())
        .collect();
    assert_eq!(sizes, vec![12, 3]);
    assert!(
        msgs[0]["altText"]
            .as_str()
            .unwrap()
            .starts_with("12 รายการ (1/2)")
    );
}

#[test]
fn choices_over_13_become_a_bubble_with_every_option() {
    let msgs = render::message(&fixtures::degrade_choices_15(), &cx());
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["type"], "flex");
    assert_eq!(all(&msgs[0], "data").len(), 15);
    assert!(msgs[0].get("quickReply").is_none());
}

#[test]
fn unlink_choices_are_quick_replies() {
    let msgs = render::message(&fixtures::unlink_confirm(), &cx());
    assert_eq!(msgs[0]["type"], "text");
    assert_eq!(msgs[0]["quickReply"]["items"].as_array().unwrap().len(), 2);
}

#[test]
fn long_text_splits_at_5000() {
    let msgs = render::message(&fixtures::degrade_long_text(), &cx());
    assert!(msgs.len() >= 2);
    let Message::Text(src) = fixtures::degrade_long_text() else {
        unreachable!()
    };
    let joined: usize = msgs.iter().map(|m| len(&m["text"])).sum();
    assert!(joined <= char_len(&src) && joined + msgs.len() >= char_len(&src));
}

#[test]
fn long_button_label_is_cut_but_display_text_is_full() {
    let msgs = render::message(&fixtures::degrade_long_labels(), &cx());
    let a = all(&msgs[0], "action")[0];
    assert_eq!(len(&a["label"]), FLEX_BUTTON_LABEL_MAX);
    assert!(a["label"].as_str().unwrap().ends_with('…'));
    assert!(len(&a["displayText"]) > FLEX_BUTTON_LABEL_MAX);
}

#[test]
fn mini_app_button_is_a_liff_uri_or_a_visible_notice() {
    let with = render::message(&fixtures::news_card(), &cx());
    let uris = all(&with[0], "uri");
    assert_eq!(
        uris[0],
        "https://liff.line.me/2000000000-SealBay0/news/1042"
    );

    let without = render::message(&fixtures::news_card(), &no_mini_app());
    assert!(all(&without[0], "uri").is_empty());
    assert!(without[0].to_string().contains("ยังเปิดในช่องทางนี้ไม่ได้"));
    assert_eq!(all(&without[0], "data").len(), 1, "acknowledge button kept");
}

#[test]
fn attachments_degrade_to_links() {
    for m in [fixtures::degrade_pdf_30mb(), fixtures::degrade_image_3mb()] {
        let msgs = render::message(&m, &cx());
        assert_eq!(msgs[0]["type"], "text");
        assert!(
            msgs[0]["text"]
                .as_str()
                .unwrap()
                .contains("https://files.sealbay.example/")
        );
    }
}

#[test]
fn every_snapshot_has_a_fixture() {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/prototypes/message-render/line"
    );
    let names: Vec<&str> = fixtures::all().iter().map(|(n, _)| *n).collect();
    for f in std::fs::read_dir(dir).unwrap() {
        let file = f.unwrap().file_name().into_string().unwrap();
        let stem = file.trim_end_matches(".json");
        assert!(names.contains(&stem), "snapshot {file} has no fixture");
    }
}
