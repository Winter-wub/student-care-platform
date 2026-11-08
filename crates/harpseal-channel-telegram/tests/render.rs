//! Renders every first-slice fixture, checks Telegram's limits and compares
//! the JSON with the committed snapshot in
//! `docs/prototypes/message-render/telegram/<fixture>.json` (read-only:
//! the snapshots are the spec; print the output with `-- --nocapture`).

use harpseal::Locale;
use harpseal::fixtures::{self, Sample};
use harpseal::render::{RenderCx, action_ids, char_len};
use harpseal_channel_telegram::render::{self, *};
use serde_json::Value;

fn cx() -> RenderCx {
    RenderCx {
        locale: Locale::th(),
        mini_app_base_url: Some("https://app.sealbay.example/tg".into()),
    }
}

/// What Telegram counts: text after HTML entity parsing.
fn visible_len(m: &Value) -> usize {
    let t = m["text"].as_str().unwrap();
    if m["parse_mode"] != "HTML" {
        return char_len(t);
    }
    let plain = t.replace("<b>", "").replace("</b>", "");
    char_len(
        &plain
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&"),
    )
}

fn buttons(m: &Value) -> Vec<&Value> {
    m["reply_markup"]["inline_keyboard"]
        .as_array()
        .map(|rows| rows.iter().flat_map(|r| r.as_array().unwrap()).collect())
        .unwrap_or_default()
}

fn assert_limits(m: &Value) {
    match m["method"].as_str().unwrap() {
        "sendMessage" => assert!((1..=TEXT_MAX).contains(&visible_len(m))),
        "sendPhoto" | "sendDocument" => {
            assert!(char_len(m["caption"].as_str().unwrap()) <= CAPTION_MAX)
        }
        x => panic!("unexpected method {x}"),
    }
    let bs = buttons(m);
    assert!(bs.len() <= KEYBOARD_BUTTONS_MAX);
    for b in bs {
        if let Some(d) = b.get("callback_data") {
            assert!(
                (1..=CALLBACK_DATA_MAX).contains(&d.as_str().unwrap().len()),
                "callback_data bytes"
            );
        }
    }
}

fn matches_snapshot(name: &str, v: &Value) {
    println!(
        "── telegram/{name}.json ──\n{}",
        serde_json::to_string_pretty(v).unwrap()
    );
    let path = format!(
        "{}/../../docs/prototypes/message-render/telegram/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let committed: Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}")))
            .unwrap();
    assert_eq!(v, &committed, "{name} differs from its committed snapshot");
}

#[test]
fn renders_every_fixture_within_limits_and_matches_its_snapshot() {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/prototypes/message-render/telegram"
    );
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    let mut names: Vec<String> = fixtures::all()
        .iter()
        .map(|(n, _)| format!("{n}.json"))
        .collect();
    names.sort();
    assert_eq!(files, names, "every snapshot has a fixture and back");
    for (name, sample) in fixtures::all() {
        let out = match &sample {
            Sample::Message(m) => {
                let msgs = render::message(m, &cx());
                assert!(!msgs.is_empty(), "{name}: nothing rendered");
                msgs.iter().for_each(assert_limits);
                // No Action is dropped or duplicated.
                let data: Vec<String> = msgs
                    .iter()
                    .flat_map(buttons)
                    .filter_map(|b| {
                        b.get("callback_data")
                            .map(|d| d.as_str().unwrap().to_string())
                    })
                    .collect();
                assert_eq!(data, action_ids(m), "{name}: Actions differ");
                Value::Array(msgs)
            }
            Sample::Menu(m) => {
                let v = render::menu(m);
                let cmds = v["setMyCommands"]["commands"].as_array().unwrap();
                assert_eq!(cmds.len(), m.entries.len());
                assert!(cmds.len() <= COMMANDS_MAX);
                for c in cmds {
                    let name = c["command"].as_str().unwrap();
                    assert!((1..=COMMAND_MAX).contains(&name.len()));
                    assert!(
                        name.chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                    );
                    assert!(
                        (1..=COMMAND_DESCRIPTION_MAX)
                            .contains(&char_len(c["description"].as_str().unwrap()))
                    );
                }
                v
            }
            _ => panic!("{name}: a Sample kind this test does not know"),
        };
        matches_snapshot(name, &out);
    }
}

#[test]
fn card_is_bold_title_with_escaped_html() {
    let card = harpseal::Message::Card(harpseal::Card {
        image: None,
        title: "A <b> & co".into(),
        body: "x < y".into(),
        buttons: vec![],
    });
    let msgs = render::message(&card, &cx());
    assert_eq!(msgs[0]["text"], "<b>A &lt;b&gt; &amp; co</b>\nx &lt; y");
    assert_eq!(msgs[0]["parse_mode"], "HTML");
    assert!(msgs[0].get("reply_markup").is_none());
}

#[test]
fn card_list_is_one_message_per_card() {
    assert_eq!(render::message(&fixtures::news_list_15(), &cx()).len(), 15);
}

#[test]
fn news_card_has_ack_callback_and_web_app() {
    let msgs = render::message(&fixtures::news_card(), &cx());
    let bs = buttons(&msgs[0]);
    assert_eq!(bs[0]["callback_data"], "news.ack.1042");
    assert_eq!(
        bs[1]["web_app"]["url"],
        "https://app.sealbay.example/tg/news/1042"
    );
}

#[test]
fn no_mini_app_url_gives_a_visible_notice_and_keeps_the_action() {
    let cx = RenderCx {
        mini_app_base_url: None,
        ..cx()
    };
    let msgs = render::message(&fixtures::news_card(), &cx);
    assert_eq!(buttons(&msgs[0]).len(), 1);
    assert!(
        msgs[0]["text"]
            .as_str()
            .unwrap()
            .contains("ยังเปิดในช่องทางนี้ไม่ได้")
    );
}

#[test]
fn over_100_buttons_continue_on_a_second_message() {
    let msgs = render::message(&fixtures::degrade_choices_120(), &cx());
    assert_eq!(msgs.len(), 2);
    assert_eq!(buttons(&msgs[0]).len(), 100);
    assert_eq!(buttons(&msgs[1]).len(), 20);
    assert_eq!(msgs[1]["text"], "ตัวเลือกเพิ่มเติม (2/2)");
}

#[test]
fn long_text_splits_at_4096() {
    assert_eq!(
        render::message(&fixtures::degrade_long_text(), &cx()).len(),
        2
    );
}

#[test]
fn attachments_pick_url_upload_or_link_by_size() {
    let pdf = render::message(&fixtures::degrade_pdf_30mb(), &cx());
    assert_eq!(pdf[0]["method"], "sendDocument");
    assert_eq!(pdf[0]["document"], "attach://file");
    let img = render::message(&fixtures::degrade_image_3mb(), &cx());
    assert_eq!(img[0]["method"], "sendPhoto");
    assert_eq!(
        img[0]["photo"],
        "https://files.sealbay.example/s/map.png?sig=x"
    );
}

#[test]
fn command_names_are_valid() {
    assert_eq!(command_name("news.latest"), "news_latest");
    assert_eq!(command_name(&"A".repeat(64)).len(), 32);
}

#[test]
fn calls_add_chat_id_and_move_upload_out() {
    use harpseal::ChannelAccountId;
    let to = ChannelAccountId {
        channel: harpseal_channel_telegram::KIND,
        external_id: "42".into(),
    };
    let calls = harpseal_channel_telegram::calls(
        &to,
        render::message(&fixtures::degrade_pdf_30mb(), &cx()),
    );
    assert_eq!(calls[0].method, "sendDocument");
    assert_eq!(calls[0].body["chat_id"], "42");
    assert!(calls[0].body.get("method").is_none());
    assert_eq!(calls[0].upload.as_deref(), Some("files/2026/calendar.pdf"));
}
