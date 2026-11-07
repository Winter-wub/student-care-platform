//! The Menu key comes from the Menu (#64): a Deployment's unlocked Menu
//! keeps its Locale's key while `setting` overrides change its labels, so
//! LINE's superseded rich menus are found by key, not by content.

mod common;

use std::sync::Arc;

use common::{LineApi, SECRET, TOKEN, USER, png};
use harpseal::{
    ActionContext, ActionId, Channel, ChannelAccountId, Deployment, Locale, MenuEntrySpec, MenuKey,
    Message, Module, ModuleRegistrar,
};
use harpseal_channel_line::richmenu::{LOCKED_KEY, name_key};
use harpseal_channel_line::{KIND, LineChannel, LineConfig};
use harpseal_testing::Harness;

/// One Menu entry, labelled `news.menu`.
struct News;

impl Module for News {
    fn name(&self) -> &'static str {
        "news"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        let entry = MenuEntrySpec {
            label_key: "news.menu",
            action: ActionId::new("news.latest").unwrap(),
            order: 10,
        };
        r.messages(&[
            harpseal::CatalogEntry {
                key: "news.menu",
                locale: "th",
                text: "ข่าว",
            },
            harpseal::CatalogEntry {
                key: "news.menu",
                locale: "en",
                text: "News",
            },
        ])
        .menu_entry(entry, |_: ActionContext| async move {
            Ok(vec![Message::text("ข่าวล่าสุด")])
        });
    }
}

fn deployment(h: &Harness) -> Deployment {
    h.builder().module(News).build().unwrap()
}

/// One LINE Deployment of `generation` on the shared channel.
fn line(api: &LineApi, generation: u32) -> LineChannel {
    let locked = harpseal::fixtures::menu_locked();
    let cfg = LineConfig::new(SECRET, TOKEN, "")
        .with_locked_menu(locked)
        .with_menu_images(png(2500, 843), png(2500, 1686))
        .with_menu_generation(generation);
    LineChannel::new(cfg, Arc::new(api.clone()))
}

fn to(n: u8) -> ChannelAccountId {
    ChannelAccountId::new(KIND, format!("{}{n}", &USER[..USER.len() - 1]))
}

/// The `(key, generation)` of every live keyed rich menu, sorted.
fn tags(api: &LineApi) -> Vec<(String, u32)> {
    let mut t: Vec<(String, u32)> = api
        .live_names()
        .iter()
        .filter_map(|n| name_key(n).flatten().map(|(k, g)| (k.to_string(), g)))
        .collect();
    t.sort();
    t
}

#[tokio::test]
async fn a_label_edited_twice_keeps_its_menu_key_and_one_rich_menu_remains() {
    let h = Harness::new();
    let d = deployment(&h);
    let api = LineApi::default();
    let th = Locale::th();
    // Labels as a Staff member's `setting` overrides would leave them.
    let edited = |label: &'static str| d.menu_with(&th, move |_| label.to_string());

    // Generation 1: the catalog label, then two back-office edits.
    let ch = line(&api, 1);
    ch.set_menu(&to(1), None).await.unwrap();
    ch.set_menu(&to(2), Some(&d.menu_for(&Locale::en())))
        .await
        .unwrap();
    for menu in [d.menu_for(&th), edited("ข่าวสาร"), edited("ข่าวประจำวัน")]
    {
        assert_eq!(
            menu.key().map(MenuKey::as_str),
            Some("th"),
            "the key ignores the labels"
        );
        ch.set_menu(&to(3), Some(&menu)).await.unwrap();
    }
    let th1 = |t: &(String, u32)| t == &("th".to_string(), 1);
    assert_eq!(
        tags(&api).iter().filter(|t| th1(t)).count(),
        3,
        "within a generation the edits leak, but keep the key: {:#?}",
        api.live_names()
    );
    assert!(
        api.live_names()
            .iter()
            .all(|n| matches!(name_key(n), Some(Some(_)))),
        "no keyless rich menu: {:#?}",
        api.live_names()
    );

    // Generation 2 with the last edit: the older versions go.
    let ch = line(&api, 2);
    ch.set_menu(&to(1), None).await.unwrap();
    ch.set_menu(&to(2), Some(&d.menu_for(&Locale::en())))
        .await
        .unwrap();
    ch.set_menu(&to(3), Some(&edited("ข่าวประจำวัน")))
        .await
        .unwrap();
    let want: Vec<(String, u32)> = ["en", LOCKED_KEY, "th"]
        .iter()
        .map(|k| (k.to_string(), 2))
        .collect();
    assert_eq!(tags(&api), want, "{:#?}", api.live_names());
    assert_eq!(api.live_names().len(), 3, "{:#?}", api.live_names());
}

#[tokio::test]
async fn stray_locale_tags_share_the_default_locales_rich_menu() {
    let h = Harness::new();
    let d = deployment(&h);
    let api = LineApi::default();
    let ch = line(&api, 1);
    for (n, tag) in [(1, "th"), (2, "TH"), (3, "th-TH"), (4, "locked")] {
        let menu = d.menu_for(&Locale::new(tag));
        assert_eq!(menu.key().map(MenuKey::as_str), Some("th"), "{tag}");
        ch.set_menu(&to(n), Some(&menu)).await.unwrap();
    }
    assert_eq!(
        tags(&api),
        [("th".to_string(), 1)],
        "{:#?}",
        api.live_names()
    );
}

#[tokio::test]
async fn a_menu_without_a_key_still_works() {
    let api = LineApi::default();
    let ch = line(&api, 2);
    let menu = harpseal::fixtures::menu_unlocked();
    assert_eq!(menu.key(), None);
    ch.set_menu(&to(1), Some(&menu)).await.unwrap();
    let names = api.live_names();
    assert_eq!(names.len(), 1);
    assert_eq!(
        name_key(&names[0]),
        Some(None),
        "harpseal-<hash>: {names:?}"
    );
    assert!(api.linked(&to(1).external_id).is_some());
}
