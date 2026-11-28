//! A Menu variant's Menu (#182) is drawn on the variant's own image, and
//! the whole Menu on the unlocked one; a variant without an image falls
//! back to the unlocked one. `check_menu_images` checks the variant's.

mod common;

use std::sync::Arc;

use common::{LineApi, SECRET, TOKEN, USER, png};
use harpseal::{ActionId, Channel, ChannelAccountId, Menu, MenuEntry, MenuKey};
use harpseal_channel_line::richmenu::rich_menu;
use harpseal_channel_line::{KIND, LineChannel, LineConfig};

const DATA: &str = "https://api-data.line.me/v2/bot";

fn entry(a: &str) -> MenuEntry {
    MenuEntry {
        label: a.into(),
        action: ActionId::new(a).unwrap(),
    }
}

fn whole() -> Menu {
    Menu::keyed(
        MenuKey::new("th").unwrap(),
        vec![entry("a.news"), entry("a.fees")],
    )
}

fn grad() -> Menu {
    Menu::keyed(MenuKey::new("thgrad").unwrap(), vec![entry("a.news")]).with_variant("grad")
}

fn config() -> LineConfig {
    LineConfig::new(SECRET, TOKEN, "")
        .with_menu_images(png(2500, 843), png(2500, 1686))
        .with_variant_menu_image("grad", png(2500, 843))
}

/// The image bytes uploaded to rich menu `id`.
fn uploaded(api: &LineApi, id: &str) -> Vec<u8> {
    api.requests()
        .into_iter()
        .find(|r| r.method == "POST" && r.uri == format!("{DATA}/richmenu/{id}/content"))
        .map(|r| r.body)
        .expect("an image upload")
}

#[tokio::test]
async fn each_menu_is_drawn_on_its_own_image() {
    let api = LineApi::default();
    let ch = LineChannel::new(config(), Arc::new(api.clone()));
    let to = ChannelAccountId::new(KIND, USER);
    ch.set_menu(&to, Some(&grad())).await.unwrap();
    let name = rich_menu(Some((grad().key().unwrap(), 1)), &grad(), &png(2500, 843))
        .unwrap()
        .0;
    let id = api.id_of(&name).expect("the variant's rich menu");
    assert_eq!(uploaded(&api, &id), png(2500, 843));
    assert_eq!(api.linked(USER), Some(id));
    assert!(name.starts_with("harpseal-thgrad-1-"), "{name}");

    ch.set_menu(&to, Some(&whole())).await.unwrap();
    let name = rich_menu(
        Some((whole().key().unwrap(), 1)),
        &whole(),
        &png(2500, 1686),
    )
    .unwrap()
    .0;
    let id = api.id_of(&name).expect("the whole Menu's rich menu");
    assert_eq!(uploaded(&api, &id), png(2500, 1686));
}

#[test]
fn the_image_follows_the_variant() {
    let c = config();
    assert_eq!(c.menu_image(&grad()), png(2500, 843).as_slice());
    assert_eq!(c.menu_image(&whole()), png(2500, 1686).as_slice());
    let other = whole().with_variant("away");
    assert_eq!(
        c.menu_image(&other),
        png(2500, 1686).as_slice(),
        "no image: unlocked"
    );
}

#[test]
fn a_bad_variant_image_is_refused_at_startup() {
    let bad = config().with_variant_menu_image("grad", png(1200, 800));
    let e = bad.check_menu_images().unwrap_err();
    assert!(e.to_string().contains("variant `grad` Menu image"), "{e}");
    assert!(config().check_menu_images().is_ok());
}
