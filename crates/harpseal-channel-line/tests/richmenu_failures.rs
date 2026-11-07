//! Cleanup never fails `set_menu` (#36): a delete LINE refuses, a second
//! list that fails, and a re-link onto a racing twin that fails all leave
//! the account linked and the call `Ok`.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use common::{LineApi, SECRET, TOKEN, USER, png};
use harpseal::{ActionId, Channel, ChannelAccountId, HttpClient, Menu, MenuEntry, Result};
use harpseal_channel_line::richmenu::rich_menu;
use harpseal_channel_line::{KIND, LineChannel, LineConfig};
use serde_json::json;

fn locked() -> Menu {
    Menu::new(vec![MenuEntry {
        label: "เริ่มใช้งาน".into(),
        action: ActionId::new("link.start").unwrap(),
    }])
}

fn channel(http: Arc<dyn HttpClient>, generation: u32) -> LineChannel {
    let cfg = LineConfig::new(SECRET, TOKEN, "")
        .with_locked_menu(locked())
        .with_menu_images(png(2500, 843), png(2500, 1686))
        .with_menu_generation(generation);
    LineChannel::new(cfg, http)
}

fn to() -> ChannelAccountId {
    ChannelAccountId::new(KIND, USER)
}

/// The name of the locked Menu's rich menu in `generation`.
fn locked_name(generation: u32) -> String {
    rich_menu(
        Some((&harpseal::MenuKey::locked(), generation)),
        &locked(),
        &png(2500, 843),
    )
    .unwrap()
    .0
}

const OLD: &str = "richmenu-0000000000000000000000000000000f";

fn calls(api: &LineApi) -> Vec<String> {
    api.take()
        .iter()
        .map(|r| format!("{} {}", r.method, r.uri))
        .collect()
}

#[tokio::test]
async fn a_refused_delete_still_sets_the_menu() {
    for (status, body) in [
        (500, r#"{"message":"Internal server error"}"#),
        (
            429,
            r#"{"message":"The API rate limit has been exceeded. Try again later."}"#,
        ),
    ] {
        let api = LineApi::default();
        api.preexisting(OLD, json!({ "name": locked_name(1) }));
        let ch = channel(Arc::new(api.clone()), 2);
        for _ in 0..5 {
            api.http().respond_with(200, "{}"); // list, create, upload, link, list
        }
        api.http().respond_with(status, body); // delete generation 1
        ch.set_menu(&to(), None).await.unwrap();
        let got = calls(&api);
        assert_eq!(
            got.last().unwrap(),
            &format!("DELETE https://api.line.me/v2/bot/richmenu/{OLD}")
        );
        assert!(api.linked(USER).is_some_and(|id| id != OLD));
        assert!(
            api.live_names().contains(&locked_name(1)),
            "{status}: left for later"
        );
    }
}

#[tokio::test]
async fn a_failed_second_list_still_sets_the_menu_and_deletes_nothing() {
    let api = LineApi::default();
    api.preexisting(OLD, json!({ "name": locked_name(1) }));
    let ch = channel(Arc::new(api.clone()), 2);
    for _ in 0..4 {
        api.http().respond_with(200, "{}"); // list, create, upload, link
    }
    api.http()
        .respond_with(500, r#"{"message":"Internal server error"}"#);
    ch.set_menu(&to(), None).await.unwrap();
    let got = calls(&api);
    assert_eq!(got.len(), 5, "{got:#?}");
    assert!(got.iter().all(|c| !c.starts_with("DELETE")));
    assert!(api.linked(USER).is_some());
}

/// Before the second list, another isolate's twin (smaller id) appears;
/// the request after that list (the re-link onto the twin) fails.
struct TwinThenFail {
    api: LineApi,
    lists: AtomicUsize,
}

const TWIN: &str = "richmenu-00000000000000000000000000000000";

#[async_trait]
impl HttpClient for TwinThenFail {
    async fn send(&self, req: http::Request<Vec<u8>>) -> Result<http::Response<Vec<u8>>> {
        let list = req.uri().path().ends_with("/richmenu/list");
        let second = list && self.lists.fetch_add(1, Ordering::SeqCst) == 1;
        if second {
            self.api
                .preexisting(TWIN, json!({ "name": locked_name(1) }));
        }
        let resp = self.api.send(req).await;
        if second {
            self.api
                .http()
                .respond_with(500, r#"{"message":"Internal server error"}"#);
        }
        resp
    }
}

#[tokio::test]
async fn a_failed_relink_onto_the_twin_keeps_ours_and_deletes_nothing() {
    let api = LineApi::default();
    let http = TwinThenFail {
        api: api.clone(),
        lists: AtomicUsize::new(0),
    };
    let ch = channel(Arc::new(http), 1);
    ch.set_menu(&to(), None).await.unwrap();

    let got = calls(&api);
    let ours = api.linked(USER).unwrap();
    assert_ne!(ours, TWIN);
    assert_eq!(
        got.last().unwrap(),
        &format!("POST https://api.line.me/v2/bot/user/{USER}/richmenu/{TWIN}"),
        "the re-link was tried: {got:#?}"
    );
    assert!(got.iter().all(|c| !c.starts_with("DELETE")));
    assert_eq!(
        api.live_names().len(),
        2,
        "both twins left for a later call"
    );
}
