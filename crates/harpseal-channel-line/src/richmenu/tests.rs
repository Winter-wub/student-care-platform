use super::*;

pub(crate) fn png(w: u32, h: u32) -> Vec<u8> {
    let mut v = PNG_MAGIC.to_vec();
    v.extend([0, 0, 0, 13]);
    v.extend(b"IHDR");
    v.extend(w.to_be_bytes());
    v.extend(h.to_be_bytes());
    v.extend([8, 6, 0, 0, 0]);
    v
}

#[test]
fn images_must_be_png_of_the_documented_size() {
    assert_eq!(png_size(&png(2500, 1686)).unwrap(), (2500, 1686));
    assert_eq!(png_size(&png(2500, 843)).unwrap(), (2500, 843));
    assert!(png_size(&png(1200, 810)).is_err());
    assert!(png_size(&[]).is_err());
    assert!(png_size(b"GIF89a................................").is_err());
    let mut big = png(2500, 843);
    big.resize(IMAGE_MAX_BYTES + 1, 0);
    assert!(png_size(&big).is_err());
}

#[test]
fn the_name_changes_with_the_menu_the_image_or_the_key() {
    let menu = harpseal::fixtures::menu_unlocked();
    let (a, obj) = rich_menu(None, &menu, &png(2500, 1686)).unwrap();
    assert_eq!(obj["name"], a);
    assert_eq!(obj["size"]["height"], 1686);
    let (b, _) = rich_menu(None, &menu, &png(2500, 843)).unwrap();
    let (c, _) = rich_menu(None, &harpseal::fixtures::menu_locked(), &png(2500, 1686)).unwrap();
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_eq!(rich_menu(None, &menu, &png(2500, 1686)).unwrap().0, a);
    let (th, _) = rich_menu(Some("th"), &menu, &png(2500, 1686)).unwrap();
    assert_eq!(th, a.replace("harpseal-", "harpseal-th-"));
    assert!(th.len() <= 300, "LINE's name limit");
    assert!(rich_menu(Some("TH"), &menu, &png(2500, 1686)).is_err());
}

#[test]
fn names_carry_their_menu_key() {
    assert_eq!(name_key("harpseal-th-0a1b"), Some(Some("th")));
    assert_eq!(name_key("harpseal-locked-0a1b"), Some(Some("locked")));
    assert_eq!(name_key("harpseal-0a1b"), Some(None));
    assert_eq!(name_key("harpseal-Th-0a1b"), None);
    assert_eq!(name_key("spring-menu"), None);
}
