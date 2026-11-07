//! The rich menu set stays bounded and fresh on the recorded Messaging API:
//! superseded versions of a registered Menu are deleted, a cached id LINE
//! no longer knows is recreated once, and two cold isolates racing on the
//! same Menu end with one rich menu (#36).

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use common::{LineApi, SECRET, TOKEN, USER, png};
use harpseal::{ActionId, Channel, ChannelAccountId, Error, HttpClient, Menu, MenuEntry, Result};
use harpseal_channel_line::richmenu::{LOCKED_KEY, name_key, rich_menu};
use harpseal_channel_line::{KIND, LineChannel, LineConfig};
use serde_json::json;

fn menu(labels: &[&str]) -> Menu {
    Menu {
        entries: labels
            .iter()
            .enumerate()
            .map(|(i, l)| MenuEntry {
                label: (*l).into(),
                action: ActionId::new(format!("news.m{i}")).unwrap(),
            })
            .collect(),
    }
}

fn locked() -> Menu {
    menu(&["เริ่มใช้งาน"])
}

fn config() -> LineConfig {
    LineConfig::new(SECRET, TOKEN, "")
        .with_locked_menu(locked())
        .with_menu_images(png(2500, 843), png(2500, 1686))
}

fn channel(http: Arc<dyn HttpClient>) -> LineChannel {
    LineChannel::new(config(), http)
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

/// The Menu keys of the live `harpseal-*` rich menus, sorted.
fn keys(api: &LineApi) -> Vec<String> {
    let mut k: Vec<String> = api
        .live_names()
        .iter()
        .filter_map(|n| name_key(n).flatten().map(str::to_string))
        .collect();
    k.sort();
    k
}

#[tokio::test]
async fn a_menu_changed_twice_leaves_one_rich_menu_per_menu_key() {
    let api = LineApi::default();
    // Not ours to delete: made by hand, unkeyed, another Deployment's key.
    api.preexisting(
        "richmenu-0000000000000000000000000000aaaa",
        json!({ "name": "spring-fair" }),
    );
    api.preexisting(
        "richmenu-0000000000000000000000000000bbbb",
        json!({ "name": "harpseal-0a1b" }),
    );
    api.preexisting(
        "richmenu-0000000000000000000000000000cccc",
        json!({ "name": "harpseal-fr-0a1b" }),
    );
    let en = menu(&["News"]);
    let versions = [
        menu(&["ข่าว"]),
        menu(&["ข่าวล่าสุด"]),
        menu(&["ข่าวล่าสุด", "ติดต่อ"]),
    ];

    let ch = channel(Arc::new(api.clone()));
    ch.set_menu(&to(1), None).await.unwrap();
    for th in &versions {
        ch.with_menus([("th", th.clone()), ("en", en.clone())])
            .unwrap();
        ch.set_menu(&to(2), Some(th)).await.unwrap();
        ch.set_menu(&to(3), Some(&en)).await.unwrap();
    }

    assert_eq!(
        keys(&api),
        ["en", "fr", LOCKED_KEY, "th"],
        "{:#?}",
        api.live_names()
    );
    let (th_now, _) = rich_menu(Some("th"), &versions[2], &png(2500, 1686)).unwrap();
    assert!(
        api.live_names().contains(&th_now),
        "the current version stays"
    );
    for kept in ["spring-fair", "harpseal-0a1b"] {
        assert!(api.live_names().iter().any(|n| n == kept), "{kept} kept");
    }
    let deletes = calls(&api)
        .into_iter()
        .filter(|c| c.starts_with("DELETE https://api.line.me/v2/bot/richmenu/"))
        .count();
    assert_eq!(deletes, 2, "the two superseded th versions");
}

#[tokio::test]
async fn an_unregistered_menu_is_never_cleaned_up() {
    let api = LineApi::default();
    let ch = channel(Arc::new(api.clone()));
    ch.set_menu(&to(1), Some(&menu(&["ข่าว"]))).await.unwrap();
    ch.set_menu(&to(1), Some(&menu(&["ข่าวล่าสุด"])))
        .await
        .unwrap();
    assert_eq!(api.live_names().len(), 2);
    assert!(keys(&api).is_empty(), "unkeyed: {:#?}", api.live_names());
}

#[tokio::test]
async fn a_stale_cached_id_is_recreated_once() {
    let api = LineApi::default();
    let ch = channel(Arc::new(api.clone()));
    ch.set_menu(&to(1), None).await.unwrap();
    let old = api.linked(&to(1).external_id).unwrap();
    api.delete_in_console(&old);
    api.take();

    ch.set_menu(&to(1), None).await.unwrap();
    let new = "richmenu-00000000000000000000000000000002";
    let user = &to(1).external_id;
    assert_eq!(
        calls(&api),
        [
            format!("POST https://api.line.me/v2/bot/user/{user}/richmenu/{old}"),
            "GET https://api.line.me/v2/bot/richmenu/list".into(),
            "POST https://api.line.me/v2/bot/richmenu".into(),
            format!("POST https://api-data.line.me/v2/bot/richmenu/{new}/content"),
            format!("POST https://api.line.me/v2/bot/user/{user}/richmenu/{new}"),
            "GET https://api.line.me/v2/bot/richmenu/list".into(),
        ]
    );
    assert_eq!(api.linked(user).as_deref(), Some(new));

    ch.set_menu(&to(2), None).await.unwrap();
    assert_eq!(calls(&api).len(), 1, "the new id is cached: link only");
}

#[tokio::test]
async fn a_second_404_on_link_is_a_platform_error() {
    let api = LineApi::default();
    let ch = channel(Arc::new(api.clone()));
    ch.set_menu(&to(1), None).await.unwrap();
    api.delete_in_console(&api.linked(&to(1).external_id).unwrap());
    api.take();
    let gone = r#"{"message":"Not found"}"#;
    api.http().respond_with(404, gone); // link, cached id
    api.http().respond_with(200, "{}"); // list
    api.http().respond_with(200, "{}"); // create
    api.http().respond_with(200, "{}"); // upload
    api.http().respond_with(404, gone); // link, new id
    let got = ch.set_menu(&to(1), None).await;
    assert!(matches!(got, Err(Error::Platform(_))), "{got:?}");
    let links = calls(&api)
        .iter()
        .filter(|c| c.contains("/v2/bot/user/"))
        .count();
    assert_eq!(links, 2, "one recreate, no loop");
}

/// Hands control back to the runtime before every request, so two
/// `set_menu` calls joined on one task interleave request by request.
struct Yielding(LineApi);

#[async_trait]
impl HttpClient for Yielding {
    async fn send(&self, req: http::Request<Vec<u8>>) -> Result<http::Response<Vec<u8>>> {
        tokio::task::yield_now().await;
        self.0.send(req).await
    }
}

#[tokio::test]
async fn two_cold_isolates_racing_on_one_menu_keep_one_rich_menu() {
    let api = LineApi::default();
    let a = channel(Arc::new(Yielding(api.clone())));
    let b = channel(Arc::new(Yielding(api.clone())));
    let (one, two) = (to(1), to(2));
    let (ra, rb) = tokio::join!(a.set_menu(&one, None), b.set_menu(&two, None));
    ra.unwrap();
    rb.unwrap();

    let creates = api
        .requests()
        .iter()
        .filter(|r| r.method == "POST" && r.uri.ends_with("/v2/bot/richmenu"))
        .count();
    assert_eq!(creates, 2, "both listed before either created: a real race");
    assert_eq!(api.live_names().len(), 1, "{:#?}", api.live_names());
    let one = api.linked(&to(1).external_id);
    assert!(one.is_some());
    assert_eq!(one, api.linked(&to(2).external_id), "both on the survivor");
}
