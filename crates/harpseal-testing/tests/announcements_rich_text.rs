//! The body format (#108) end to end on the Fake Channels: the pushed
//! Card (plain text, up to 2 URL buttons), the "ข่าวล่าสุด" list (plain
//! text), the Mini App page (formatted, links open outside), and the
//! editor's preview (the very markup the Mini App shows).

mod announcements_support;
mod app_support;

use announcements_support::*;
use app_support::*;
use harpseal::{Button, Card, Message};

const A: &str = "https://reg.seal-bay.invalid/exam";
const B: &str = "https://map.seal-bay.invalid/room/3105";
const C: &str = "https://c.seal-bay.invalid/";

fn body() -> String {
    format!(
        "**ด่วน** สอบย้ายห้อง *ทุกวิชา*\n- ดู [ตารางสอบ]({A})\n- แผนที่ {B}\n1. ซ้ำ [อีกครั้ง]({A})\n\
         2. [ที่สาม]({C})\n[อันตราย](javascript:alert(1)) [เว็บเก่า](http://old.seal-bay.invalid/) \
         <script>alert(1)</script>"
    )
}

const PLAIN: &str = "ด่วน สอบย้ายห้อง ทุกวิชา\n- ดู ตารางสอบ\n- แผนที่ ";

fn card_of(msgs: Vec<Message>) -> Card {
    msgs.into_iter()
        .find_map(|m| match m {
            Message::Card(c) => Some(c),
            _ => None,
        })
        .expect("a Card was pushed")
}

fn urls(c: &Card) -> Vec<(String, String)> {
    c.buttons
        .iter()
        .filter_map(|b| match b {
            Button::Url { label, url, .. } => Some((label.clone(), url.clone())),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn the_pushed_card_is_plain_with_the_first_two_links_as_buttons() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("ย้ายห้องสอบ", &body(), "&require_ack=1&do=confirm")).await;
    drain(&w).await;

    for card in [
        card_of(w.line.sent_to(&fake_account("fake", "s1"))),
        card_of(w.tg.sent_to(&fake_account(TG, "s1"))),
    ] {
        assert!(
            card.body
                .starts_with(&format!("{PLAIN}{B}\n1. ซ้ำ อีกครั้ง\n2. ที่สาม")),
            "{}",
            card.body
        );
        assert!(
            card.body.contains("[อันตราย](javascript:alert(1))"),
            "kept as text"
        );
        let labels: Vec<&str> = card.buttons.iter().map(Button::label).collect();
        assert_eq!(labels, ["รับทราบ", "เปิดดูรายละเอียด", "ตารางสอบ", B]);
        assert_eq!(
            urls(&card),
            [("ตารางสอบ".into(), A.into()), (B.into(), B.into())]
        );
    }

    // "ข่าวล่าสุด": plain text, no URL buttons.
    let latest = press(&w, "fake", "s1", "announcements.latest").await;
    let Message::CardList(list) = &latest[0] else {
        panic!("{latest:?}")
    };
    assert!(list.0[0].body.starts_with(PLAIN), "{}", list.0[0].body);
    assert!(urls(&list.0[0]).is_empty());

    // The Mini App page: formatted, escaped, links open outside.
    let app = open(&w, "fake", "s1").await;
    let page = app.get(&format!("/app/announcements/{id}")).await.body;
    let shown = harpseal::rich_text::html(&body()).into_string();
    assert!(page.contains(&shown), "{page}");
    assert!(page.contains("<strong>ด่วน</strong>") && page.contains("<em>ทุกวิชา</em>"));
    assert!(page.contains(&format!(
        r#"<a href="{A}" target="_blank" rel="noopener noreferrer" data-ext>ตารางสอบ</a>"#
    )));
    assert!(page.contains("<ul><li>") && page.contains("<ol><li>"));
    assert!(page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!page.contains("href=\"javascript:") && !page.contains("href=\"http://"));
    assert!(page.contains("a[data-ext]"), "the external-open script");
    // The list on /app clamps the plain text and expands to the format.
    let home = app.get("/app").await.body;
    assert!(
        home.contains(&format!("<p class=\"clamp\">{PLAIN}")),
        "{home}"
    );
    assert!(home.contains(&shown));

    // The editor's preview is the same markup.
    let preview = b
        .post(
            "/admin/announcements/preview",
            &form("ย้ายห้องสอบ", &body(), ""),
        )
        .await;
    assert_eq!(preview.status, 200);
    assert!(preview.body.contains(&shown), "{}", preview.body);
    let edit = b.get(&format!("/admin/announcements/{id}/edit")).await.body;
    assert!(edit.contains("data-for=\"body_th\"") && edit.contains("data-for=\"body_en\""));
    assert!(edit.contains(&shown), "the preview rendered with the page");
}

#[tokio::test]
async fn a_plain_body_renders_as_before_everywhere() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let plain = "งดเรียนวันศุกร์ที่ 13 พ.ย.\n\nเรียนชดเชยวันเสาร์ ห้อง 3105 (อาคาร 2) & ห้อง 3106";
    let id = create(&b, &form("งดเรียน", plain, "&do=confirm")).await;
    drain(&w).await;
    let card = card_of(w.line.sent_to(&fake_account("fake", "s1")));
    assert_eq!(card.body, plain);
    let labels: Vec<&str> = card.buttons.iter().map(Button::label).collect();
    assert_eq!(labels, ["เปิดดูรายละเอียด"]);
    let app = open(&w, "fake", "s1").await;
    let page = app.get(&format!("/app/announcements/{id}")).await.body;
    // Exactly the markup of a body before #108.
    let before = maud::html! { p class="app-body" { (plain) } }.into_string();
    assert!(page.contains(&before), "{page}");
    assert!(!page.contains("a[data-ext]"), "no link, no script");
}
