//! A rich menu that exists without its image (#98) is repaired in the same
//! `set_menu`, and a failed upload leaves no imageless rich menu to reuse.

mod common;

use std::sync::Arc;

use common::{LineApi, SECRET, TOKEN, USER, png};
use harpseal::{ActionId, Channel, ChannelAccountId, Error, Menu, MenuEntry};
use harpseal_channel_line::richmenu::rich_menu;
use harpseal_channel_line::{KIND, LineChannel, LineConfig};
use serde_json::json;

const OLD: &str = "richmenu-0000000000000000000000000000000f";
const API: &str = "https://api.line.me/v2/bot";
const DATA: &str = "https://api-data.line.me/v2/bot";

fn locked() -> Menu {
    Menu::new(vec![MenuEntry {
        label: "เริ่มใช้งาน".into(),
        action: ActionId::new("link.start").unwrap(),
    }])
}

fn channel(api: &LineApi) -> LineChannel {
    let cfg = LineConfig::new(SECRET, TOKEN, "")
        .with_locked_menu(locked())
        .with_menu_images(png(2500, 843), png(2500, 1686));
    LineChannel::new(cfg, Arc::new(api.clone()))
}

fn to() -> ChannelAccountId {
    ChannelAccountId::new(KIND, USER)
}

fn locked_name() -> String {
    rich_menu(
        Some((&harpseal::MenuKey::locked(), 1)),
        &locked(),
        &png(2500, 843),
    )
    .unwrap()
    .0
}

fn calls(api: &LineApi) -> Vec<String> {
    api.take()
        .iter()
        .map(|r| format!("{} {}", r.method, r.uri))
        .collect()
}

/// The rich menu named for the locked Menu, found by name, never given its
/// image.
fn imageless(api: &LineApi) {
    api.preexisting_imageless(OLD, json!({ "name": locked_name() }));
}

#[tokio::test]
async fn an_imageless_menu_found_by_name_is_repaired_and_linked() {
    let api = LineApi::default();
    imageless(&api);
    channel(&api).set_menu(&to(), None).await.unwrap();

    assert_eq!(
        calls(&api),
        [
            format!("GET {API}/richmenu/list"),
            format!("POST {API}/user/{USER}/richmenu/{OLD}"), // 400
            format!("GET {DATA}/richmenu/{OLD}/content"),     // 404
            format!("POST {DATA}/richmenu/{OLD}/content"),
            format!("POST {API}/user/{USER}/richmenu/{OLD}"),
        ]
    );
    assert_eq!(api.linked(USER).as_deref(), Some(OLD));
    assert!(api.has_image(OLD));
    assert_eq!(api.live_names(), [locked_name()], "reused, not recreated");
}

#[tokio::test]
async fn the_repaired_menu_is_reused_by_the_next_call_without_a_check() {
    let api = LineApi::default();
    imageless(&api);
    let ch = channel(&api);
    ch.set_menu(&to(), None).await.unwrap();
    api.take();
    ch.set_menu(&to(), None).await.unwrap();
    assert_eq!(
        calls(&api),
        [format!("POST {API}/user/{USER}/richmenu/{OLD}")]
    );
}

#[tokio::test]
async fn a_failed_repair_upload_deletes_the_menu_and_the_next_call_creates_it() {
    let api = LineApi::default();
    imageless(&api);
    let ch = channel(&api);
    for _ in 0..3 {
        api.http().respond_with(200, "{}"); // list, link, content check
    }
    api.http()
        .respond_with(500, r#"{"message":"Internal server error"}"#); // upload
    let got = ch.set_menu(&to(), None).await;
    assert!(matches!(got, Err(Error::Platform(_))), "{got:?}");
    assert_eq!(
        calls(&api).pop().unwrap(),
        format!("DELETE {API}/richmenu/{OLD}")
    );
    assert!(api.live_names().is_empty(), "no imageless menu to reuse");

    ch.set_menu(&to(), None).await.unwrap();
    let id = api.linked(USER).unwrap();
    assert_ne!(id, OLD);
    assert!(api.has_image(&id));
    assert_eq!(api.live_names(), [locked_name()]);
}

#[tokio::test]
async fn a_failed_create_upload_leaves_no_menu_and_the_next_call_creates_it() {
    let api = LineApi::default();
    let ch = channel(&api);
    api.http().respond_with(200, "{}"); // list
    api.http().respond_with(200, "{}"); // create
    api.http()
        .respond_with(500, r#"{"message":"Internal server error"}"#); // upload
    assert!(ch.set_menu(&to(), None).await.is_err());
    assert!(api.live_names().is_empty());
    assert!(api.linked(USER).is_none());

    ch.set_menu(&to(), None).await.unwrap();
    assert!(api.has_image(&api.linked(USER).unwrap()));
    assert_eq!(api.live_names().len(), 1);
}

#[tokio::test]
async fn an_imageless_menu_left_by_a_failed_upload_and_delete_is_repaired() {
    let api = LineApi::default();
    let ch = channel(&api);
    api.http().respond_with(200, "{}"); // list
    api.http().respond_with(200, "{}"); // create
    api.http()
        .respond_with(500, r#"{"message":"Internal server error"}"#); // upload
    api.http()
        .respond_with(500, r#"{"message":"Internal server error"}"#); // delete
    assert!(ch.set_menu(&to(), None).await.is_err());
    let left = api.id_of(&locked_name()).expect("the delete failed");
    assert!(!api.has_image(&left));
    api.take();

    // Another isolate, cold: it finds the leftover by name.
    channel(&api).set_menu(&to(), None).await.unwrap();
    assert_eq!(api.linked(USER), Some(left.clone()));
    assert!(api.has_image(&left));
    assert!(calls(&api).iter().all(|c| !c.starts_with("DELETE")));
    assert_eq!(api.live_names().len(), 1, "no second menu");
}

#[tokio::test]
async fn a_refusal_that_is_not_a_missing_image_is_returned_without_an_upload() {
    let api = LineApi::default();
    api.preexisting(OLD, json!({ "name": locked_name() }));
    api.refuse_links(Some(("The rich menu cannot be linked", true)));
    let got = channel(&api).set_menu(&to(), None).await;
    assert!(matches!(got, Err(Error::Platform(ref m)) if m.contains("cannot be linked")));
    let got = calls(&api);
    assert!(got.contains(&format!("GET {DATA}/richmenu/{OLD}/content")));
    assert!(got.iter().all(|c| !c.starts_with(&format!("POST {DATA}"))));
}

#[tokio::test]
async fn a_bad_user_id_does_not_check_the_image() {
    let api = LineApi::default();
    imageless(&api);
    api.refuse_links(Some((
        "The value for the 'userId' parameter is invalid",
        true,
    )));
    let got = channel(&api).set_menu(&to(), None).await;
    assert!(got.is_err());
    assert!(calls(&api).iter().all(|c| !c.contains(DATA)));
}

#[tokio::test]
async fn the_repair_runs_once_per_call() {
    let api = LineApi::default();
    imageless(&api);
    api.refuse_links(Some(("The rich menu cannot be linked", true)));
    let got = channel(&api).set_menu(&to(), None).await;
    assert!(got.is_err());
    let got = calls(&api);
    let count = |p: &str| got.iter().filter(|c| c.starts_with(p)).count();
    assert_eq!(count(&format!("POST {API}/user/")), 2, "{got:#?}");
    assert_eq!(count(&format!("POST {DATA}")), 1, "{got:#?}");
}
