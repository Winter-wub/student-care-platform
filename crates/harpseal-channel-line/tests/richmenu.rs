//! Rich menus on the wire: create, upload, link; reuse by hash through the
//! cache and through `GET /v2/bot/richmenu/list`; the locked Menu.

mod common;

use std::sync::Arc;

use common::{LineApi, SECRET, TOKEN, USER, json, png};
use harpseal::{ActionId, Channel, ChannelAccountId, Error, Menu, MenuEntry};
use harpseal_channel_line::richmenu::rich_menu;
use harpseal_channel_line::{KIND, LineChannel, LineConfig};

fn locked() -> Menu {
    Menu::new(vec![MenuEntry {
        label: "เริ่มใช้งาน".into(),
        action: ActionId::new("link.start").unwrap(),
    }])
}

fn channel(api: &LineApi, locked_menu: Menu) -> LineChannel {
    let cfg = LineConfig::new(SECRET, TOKEN, "")
        .with_locked_menu(locked_menu)
        .with_menu_images(png(2500, 843), png(2500, 1686));
    LineChannel::new(cfg, Arc::new(api.clone()))
}

fn to(n: u8) -> ChannelAccountId {
    ChannelAccountId::new(KIND, format!("{}{n}", &USER[..USER.len() - 1]))
}

fn calls(api: &LineApi) -> Vec<String> {
    api.take()
        .iter()
        .map(|r| format!("{} {}", r.method, r.uri))
        .collect()
}

#[tokio::test]
async fn the_first_set_menu_creates_uploads_and_links() {
    let api = LineApi::default();
    let ch = channel(&api, locked());
    let menu = harpseal::fixtures::menu_unlocked();
    ch.set_menu(&to(1), Some(&menu)).await.unwrap();
    let reqs = api.take();
    let id = "richmenu-00000000000000000000000000000001";
    let got: Vec<String> = reqs
        .iter()
        .map(|r| format!("{} {}", r.method, r.uri))
        .collect();
    assert_eq!(
        got,
        [
            "GET https://api.line.me/v2/bot/richmenu/list".to_string(),
            "POST https://api.line.me/v2/bot/richmenu".into(),
            format!("POST https://api-data.line.me/v2/bot/richmenu/{id}/content"),
            format!(
                "POST https://api.line.me/v2/bot/user/{}/richmenu/{id}",
                to(1).external_id
            ),
            // After a create: look for a twin and old versions to delete.
            "GET https://api.line.me/v2/bot/richmenu/list".into(),
        ]
    );
    let (_, want) = rich_menu(None, &menu, &png(2500, 1686)).unwrap();
    assert_eq!(json(&reqs[1].body), want, "the rich menu object");
    assert_eq!(want["areas"][0]["action"]["data"], "news.latest");
    assert_eq!(want["size"]["height"], 1686, "sized to the unlocked image");
    assert_eq!(reqs[2].headers["content-type"], "image/png");
    assert_eq!(reqs[2].body, png(2500, 1686));
    assert!(
        reqs.iter()
            .all(|r| r.headers["authorization"] == format!("Bearer {TOKEN}").as_str())
    );
}

#[tokio::test]
async fn two_set_menu_calls_with_the_same_menu_create_one_rich_menu() {
    let api = LineApi::default();
    let ch = channel(&api, locked());
    let menu = harpseal::fixtures::menu_unlocked();
    ch.set_menu(&to(1), Some(&menu)).await.unwrap();
    ch.set_menu(&to(2), Some(&menu)).await.unwrap();
    let got = calls(&api);
    let created = got
        .iter()
        .filter(|c| c.ends_with("/v2/bot/richmenu"))
        .count();
    assert_eq!(created, 1, "{got:#?}");
    let links = got.iter().filter(|c| c.contains("/v2/bot/user/")).count();
    assert_eq!(links, 2);
    assert_eq!(
        got.iter().filter(|c| c.ends_with("/list")).count(),
        2,
        "before and after the create; none on the cache hit"
    );
}

#[tokio::test]
async fn a_fresh_isolate_finds_the_rich_menu_through_the_list() {
    let api = LineApi::default();
    let menu = harpseal::fixtures::menu_unlocked();
    let (_, obj) = rich_menu(None, &menu, &png(2500, 1686)).unwrap();
    api.preexisting("richmenu-0000000000000000000000000000beef", obj);
    let ch = channel(&api, locked());
    ch.set_menu(&to(1), Some(&menu)).await.unwrap();
    let got = calls(&api);
    assert_eq!(got.len(), 2, "{got:#?}");
    assert!(got[1].ends_with("/richmenu/richmenu-0000000000000000000000000000beef"));
}

#[tokio::test]
async fn none_links_the_locked_menu_and_a_changed_menu_gets_a_new_rich_menu() {
    let api = LineApi::default();
    let ch = channel(&api, locked());
    ch.set_menu(&to(1), None).await.unwrap();
    let reqs = api.take();
    let created = json(&reqs[1].body);
    assert_eq!(created["areas"][0]["action"]["data"], "link.start");
    assert_eq!(created["size"]["height"], 843, "sized to the locked image");
    assert!(
        reqs[3]
            .uri
            .contains(&format!("/v2/bot/user/{}/richmenu/", to(1).external_id))
    );

    let mut menu = harpseal::fixtures::menu_unlocked();
    ch.set_menu(&to(1), Some(&menu)).await.unwrap();
    menu.entries.pop();
    ch.set_menu(&to(1), Some(&menu)).await.unwrap();
    let got = calls(&api);
    assert_eq!(
        got.iter()
            .filter(|c| c.ends_with("/v2/bot/richmenu"))
            .count(),
        2
    );
}

#[tokio::test]
async fn an_empty_locked_menu_unlinks() {
    let api = LineApi::default();
    let ch = channel(&api, Menu::default());
    ch.set_menu(&to(1), None).await.unwrap();
    assert_eq!(
        calls(&api),
        [format!(
            "DELETE https://api.line.me/v2/bot/user/{}/richmenu",
            to(1).external_id
        )]
    );
}

#[tokio::test]
async fn a_missing_or_wrong_image_is_refused_before_any_call() {
    let api = LineApi::default();
    let cfg = LineConfig::new(SECRET, TOKEN, "").with_locked_menu(locked());
    let ch = LineChannel::new(cfg.clone(), Arc::new(api.clone()));
    let got = ch.set_menu(&to(1), None).await;
    assert!(matches!(got, Err(Error::Invalid(ref m)) if m.contains("not configured")));
    let ch = LineChannel::new(
        cfg.with_menu_images(png(1200, 405), png(2500, 1686)),
        Arc::new(api.clone()),
    );
    let got = ch.set_menu(&to(1), None).await;
    assert!(matches!(got, Err(Error::Invalid(ref m)) if m.contains("1200 x 405")));
    assert!(api.take().is_empty());
}

#[tokio::test]
async fn a_failed_upload_deletes_the_rich_menu() {
    let api = LineApi::default();
    let ch = channel(&api, locked());
    api.http().respond_with(200, "{}"); // list
    api.http().respond_with(200, "{}"); // create (LineApi fills the id)
    api.http()
        .respond_with(400, r#"{"message":"invalid image"}"#);
    let got = ch.set_menu(&to(1), None).await;
    assert!(matches!(got, Err(Error::Platform(_))), "{got:?}");
    let last = calls(&api).pop().unwrap();
    assert_eq!(
        last,
        "DELETE https://api.line.me/v2/bot/richmenu/richmenu-00000000000000000000000000000001"
    );
}
