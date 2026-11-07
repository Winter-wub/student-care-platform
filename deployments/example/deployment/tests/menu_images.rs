//! The bundled rich menu images match the Menus they are drawn for, the
//! native override loads files, and a LINE Channel with real credentials
//! but no usable image refuses to start (naming the settings).

use example_deployment::menu_images::{
    LOCKED_CELLS, LOCKED_PNG, LOCKED_VAR, UNLOCKED_CELLS, UNLOCKED_PNG, UNLOCKED_VAR, load,
    locked_menu,
};
use example_deployment::{Secrets, StartError, builder, deployment};
use harpseal::Locale;
use harpseal_channel_line::render;
use harpseal_channel_line::richmenu::png_size;
use harpseal_testing::Harness;

/// The first 24 bytes of a PNG of `w` x `h` (all the adapter reads).
fn png(w: u32, h: u32) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    v.extend(w.to_be_bytes());
    v.extend(h.to_be_bytes());
    v.extend([8, 6, 0, 0, 0]);
    v
}

/// Secrets with the Mini App ids, a Fake secret and a LINE Official Account.
fn line_secrets() -> Secrets {
    Secrets::from_lookup(|k| {
        Some(match k {
            "LINE_CHANNEL_SECRET" => "seal-bay-secret".into(),
            "LINE_ACCESS_TOKEN" => "seal-bay-token".into(),
            "LINE_LIFF_ID" => "1234567890-AbCdEfGh".into(),
            "TELEGRAM_WEB_APP_URL" => "https://app.seal-bay.invalid/tg".into(),
            "FAKE_SECRET" => "s3cret".into(),
            _ => return None,
        })
    })
}

fn no_files(_: &str) -> std::io::Result<Vec<u8>> {
    Err(std::io::Error::other("no files in this test"))
}

#[test]
fn bundled_images_are_the_size_the_grid_wants_for_the_cells_they_were_drawn_for() {
    let h = Harness::new();
    let d = builder(h.services(), line_secrets())
        .channel(h.fake_channel())
        .build()
        .unwrap();
    let unlocked = [Locale::th(), Locale::en()].map(|l| d.menu_for(&l));
    let cases = [
        ("locked", LOCKED_PNG, LOCKED_CELLS, vec![locked_menu()]),
        ("unlocked", UNLOCKED_PNG, UNLOCKED_CELLS, unlocked.to_vec()),
    ];
    for (name, image, cells, menus) in cases {
        let size = png_size(image).unwrap_or_else(|e| panic!("{name}: {e}"));
        for menu in menus {
            let rich = render::menu(&menu);
            assert_eq!(
                rich["areas"].as_array().unwrap().len(),
                cells,
                "{name}.png is drawn with {cells} cells but its Menu has {} entries: \
                 run deployments/example/assets/richmenu/make.sh and update menu_images.rs",
                menu.entries.len()
            );
            assert_eq!(
                rich["size"]["height"].as_u64().unwrap() as usize,
                size.1,
                "{name}.png must be as high as the Core's grid for {cells} cells"
            );
        }
    }
}

#[test]
fn the_locked_menu_is_one_start_entry() {
    let menu = locked_menu();
    let [entry] = &menu.entries[..] else {
        panic!("{menu:?}")
    };
    assert_eq!(entry.action.as_str(), "kernel.link.start");
    assert_eq!(entry.label, "เริ่มใช้งาน");
}

#[test]
fn unset_or_empty_image_variables_are_the_bundled_images() {
    for env in [None, Some("")] {
        let (locked, unlocked) = load(|_| env.map(str::to_string), no_files).unwrap();
        assert_eq!(
            (locked, unlocked),
            (LOCKED_PNG.to_vec(), UNLOCKED_PNG.to_vec())
        );
    }
}

#[test]
fn a_named_file_replaces_its_image_only() {
    let mine = png(2500, 1686);
    let read = |p: &str| match p {
        "/srv/menus/mine.png" => Ok(mine.clone()),
        _ => Err(std::io::Error::other("unexpected path")),
    };
    let get = |k: &str| (k == UNLOCKED_VAR).then(|| "/srv/menus/mine.png".to_string());
    let (locked, unlocked) = load(get, read).unwrap();
    assert_eq!(locked, LOCKED_PNG);
    assert_eq!(unlocked, mine);
}

#[test]
fn an_unreadable_file_names_the_variable_and_the_path() {
    let get = |k: &str| (k == LOCKED_VAR).then(|| "/srv/menus/gone.png".to_string());
    let read = |_: &str| Err(std::io::Error::from(std::io::ErrorKind::NotFound));
    let err = load(get, read).unwrap_err();
    assert!(
        err.contains("LINE_MENU_IMAGE_LOCKED=/srv/menus/gone.png"),
        "{err}"
    );
    let secrets = Secrets::default().with_menu_image_files(get, read);
    assert!(matches!(secrets, Err(StartError::MenuImages(_))));
}

#[test]
fn real_line_credentials_with_the_bundled_images_start() {
    let h = Harness::new();
    deployment(h.services(), line_secrets()).expect("bundled images are enough");
}

#[test]
fn real_line_credentials_without_images_refuse_to_start() {
    let h = Harness::new();
    let mut secrets = line_secrets();
    secrets.line = secrets.line.with_menu_images(vec![], vec![]);
    let err = deployment(h.services(), secrets).unwrap_err();
    assert!(matches!(err, StartError::MenuImages(_)), "{err}");
    let msg = err.to_string();
    assert!(
        msg.contains("LINE_MENU_IMAGE_LOCKED") && msg.contains("LINE_MENU_IMAGE_UNLOCKED"),
        "{msg}"
    );
    assert!(msg.contains("not configured"), "{msg}");
}

#[test]
fn a_named_file_of_the_wrong_size_refuses_to_start() {
    let h = Harness::new();
    let secrets = line_secrets()
        .with_menu_image_files(
            |k| (k == UNLOCKED_VAR).then(|| "small.png".to_string()),
            |_| Ok(png(1200, 810)),
        )
        .unwrap();
    let msg = deployment(h.services(), secrets).unwrap_err().to_string();
    assert!(
        msg.contains("1200 x 810") && msg.contains(UNLOCKED_VAR),
        "{msg}"
    );
}

#[test]
fn without_a_line_token_nothing_is_checked() {
    let h = Harness::new();
    let mut secrets = line_secrets();
    secrets.line = harpseal_channel_line::LineConfig::new("", "", "1234567890-AbCdEfGh");
    deployment(h.services(), secrets).expect("the Fake Channel needs no rich menus");
}
