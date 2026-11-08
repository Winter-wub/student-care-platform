use super::*;

#[test]
fn action_id_bounds() {
    assert!(ActionId::new("").is_err());
    assert!(ActionId::new("a".repeat(64)).is_ok());
    assert!(ActionId::new("a".repeat(65)).is_err());
}

#[test]
fn url_buttons_take_https_only() {
    let b = Button::url("ตารางสอบ", "https://reg.seal-bay.invalid/exam").unwrap();
    assert_eq!(b.label(), "ตารางสอบ");
    assert_eq!(b.safe_url(), Some("https://reg.seal-bay.invalid/exam"));
    assert_eq!(b.action_id(), None);
    let b = Button::url("\u{202E}gnp.exe\u{200B}", "https://reg.seal-bay.invalid/").unwrap();
    assert_eq!(b.label(), "gnp.exe");
    for blank in ["", " ", "\u{200B}", "\u{3164}\u{2800}"] {
        let b = Button::url(blank, "https://reg.seal-bay.invalid/exam").unwrap();
        assert_eq!(b.label(), "reg.seal-bay.invalid", "{blank:?}");
    }
    for bad in [
        "http://reg.seal-bay.invalid/",
        "javascript:alert(1)",
        "data:,x",
        "",
    ] {
        assert!(Button::url("x", bad).is_err(), "{bad}");
    }
    // Built by hand inside the Core: the adapters' check still refuses it.
    let raw = Button::Url {
        label: "x".into(),
        url: "javascript:alert(1)".into(),
        external: false,
    };
    assert_eq!(raw.safe_url(), None);
}

#[test]
fn menu_keys_from_locales() {
    let key = |tag: &str| MenuKey::for_locale(&Locale::new(tag)).map(|k| k.as_str().to_string());
    assert_eq!(key("th").unwrap(), "th");
    assert_eq!(key("zh-hant").unwrap(), "zhhant");
    assert_eq!(key("zh-Hant").unwrap(), "zhhant");
    assert_eq!(key("pt_BR").unwrap(), "ptbr");
    assert!(key("").is_err());
    assert!(key("locked").is_err());
    assert!(key("x-a-very-long-private-tag").is_err());
    assert!(key("th.u").is_err());
}

#[test]
fn menu_key_bounds() {
    assert!(MenuKey::new("th").is_ok());
    assert!(MenuKey::new("a".repeat(16)).is_ok());
    assert!(MenuKey::new("a".repeat(17)).is_err());
    assert!(MenuKey::new("").is_err());
    assert!(MenuKey::new("TH").is_err());
    assert!(MenuKey::new("zh-hant").is_err());
    assert!(MenuKey::new("locked").is_err(), "reserved");
    assert_eq!(MenuKey::locked().as_str(), MenuKey::LOCKED);
    let m = Menu::new(Vec::new());
    assert_eq!(m.key(), None);
    let m = m.with_key(MenuKey::locked());
    assert_eq!(m.key().map(MenuKey::as_str), Some("locked"));
    assert_eq!((m.revision(), m.salt()), (0, None));
    let m = m.with_revision(3).with_salt("0a1b2c3d");
    assert_eq!((m.revision(), m.salt()), (3, Some("0a1b2c3d")));
}
