//! Contract checks on `set_menu`.

use harpseal::{Locale, Menu, MenuEntry, MenuKey};

use super::checks::id;
use super::{SeenKey, Target};

fn news() -> Vec<MenuEntry> {
    vec![MenuEntry {
        label: "ข่าวล่าสุด".into(),
        action: id("contract.menu.news"),
    }]
}

/// The Menu can be set and reset to the locked Menu.
pub async fn menu_set_and_unset(t: &Target) {
    let kind = t.channel.kind();
    let menu = Menu::new(news());
    let to = &t.fixtures.to;
    let set = t.channel.set_menu(to, Some(&menu)).await;
    assert!(set.is_ok(), "{kind}: setting a Menu failed: {set:?}");
    assert_eq!(t.wire.menu_entries(to), Some(1), "{kind}: the Menu is set");
    let unset = t.channel.set_menu(to, None).await;
    assert!(unset.is_ok(), "{kind}: locking the Menu failed: {unset:?}");
    assert_eq!(t.wire.menu_entries(to), None, "{kind}: the Menu is locked");
}

/// A Menu with a Menu key can be set, and the key reaches the platform
/// when the wire can show it ([`super::Wire::menu_key`]).
pub async fn keyed_menu_is_set(t: &Target) {
    let kind = t.channel.kind();
    let key = MenuKey::for_locale(&Locale::th()).expect("th is a valid key");
    let menu = Menu::keyed(key, news());
    let to = &t.fixtures.to;
    let set = t.channel.set_menu(to, Some(&menu)).await;
    assert!(set.is_ok(), "{kind}: setting a keyed Menu failed: {set:?}");
    assert_eq!(t.wire.menu_entries(to), Some(1), "{kind}: the Menu is set");
    let seen = t.wire.menu_key(to);
    if seen != SeenKey::Hidden {
        assert_eq!(seen, SeenKey::Key("th".into()), "{kind}: the Menu key");
    }
}
