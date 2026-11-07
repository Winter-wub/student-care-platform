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
fn the_name_changes_with_the_menu_the_image_the_key_or_the_generation() {
    let menu = harpseal::fixtures::menu_unlocked();
    let (a, obj) = rich_menu(None, &menu, &png(2500, 1686)).unwrap();
    assert_eq!(obj["name"], a);
    assert_eq!(name_key(&a), Some(None));
    assert_eq!(obj["size"]["height"], 1686);
    let (b, _) = rich_menu(None, &menu, &png(2500, 843)).unwrap();
    let (c, _) = rich_menu(None, &harpseal::fixtures::menu_locked(), &png(2500, 1686)).unwrap();
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_eq!(rich_menu(None, &menu, &png(2500, 1686)).unwrap().0, a);
    let key = MenuKey::new("th").unwrap();
    let (th, _) = rich_menu(Some((&key, 7)), &menu, &png(2500, 1686)).unwrap();
    assert_eq!(th, a.replace("harpseal-", "harpseal-th-7-"));
    assert_eq!(name_key(&th), Some(Some(("th", 7))));
    assert!(th.len() <= 300, "LINE's name limit");
}
