//! `LineConfig::check_menu_images`: what a Deployment calls at startup.

mod common;

use common::{SECRET, TOKEN, png};
use harpseal::{ActionId, Menu, MenuEntry};
use harpseal_channel_line::LineConfig;

fn locked() -> Menu {
    Menu::new(vec![MenuEntry {
        label: "เริ่มใช้งาน".into(),
        action: ActionId::new("kernel.link.start").unwrap(),
    }])
}

#[test]
fn real_credentials_without_images_are_refused_naming_the_menu() {
    let cfg = LineConfig::new(SECRET, TOKEN, "");
    let err = cfg.check_menu_images().unwrap_err().to_string();
    assert!(
        err.contains("unlocked Menu") && err.contains("not configured"),
        "{err}"
    );

    let cfg = cfg
        .with_locked_menu(locked())
        .with_menu_images(vec![], png(2500, 843));
    let err = cfg.check_menu_images().unwrap_err().to_string();
    assert!(
        err.contains("locked Menu") && err.contains("not configured"),
        "{err}"
    );
}

#[test]
fn a_wrong_size_or_a_non_png_is_refused() {
    let cfg = LineConfig::new(SECRET, TOKEN, "").with_menu_images(vec![], png(1200, 810));
    let err = cfg.check_menu_images().unwrap_err().to_string();
    assert!(err.contains("1200 x 810"), "{err}");
    let cfg = LineConfig::new(SECRET, TOKEN, "").with_menu_images(vec![], b"GIF89a".repeat(10));
    assert!(cfg.check_menu_images().is_err());
}

#[test]
fn good_images_pass_and_an_empty_locked_menu_needs_no_locked_image() {
    let cfg = LineConfig::new(SECRET, TOKEN, "").with_menu_images(vec![], png(2500, 1686));
    cfg.check_menu_images().unwrap();
    let cfg = cfg
        .with_locked_menu(locked())
        .with_menu_images(png(2500, 843), png(2500, 1686));
    cfg.check_menu_images().unwrap();
}

#[test]
fn without_an_access_token_nothing_is_checked() {
    let cfg = LineConfig::new(SECRET, "", "").with_locked_menu(locked());
    cfg.check_menu_images().unwrap();
}
