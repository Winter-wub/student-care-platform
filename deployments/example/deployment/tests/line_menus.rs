//! The example's unlocked Menus carry their Locale as Menu key, so their
//! LINE rich menus are named by Locale and older generations get cleaned
//! up, with no registration.

use std::sync::Arc;

use example_deployment::{Secrets, builder};
use harpseal::{Channel, ChannelAccountId, Locale};
use harpseal_channel_line::{KIND, LineChannel, LineConfig};
use harpseal_testing::Harness;

/// The first 24 bytes of a PNG of `w` x `h` (all the adapter reads).
fn png(w: u32, h: u32) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    v.extend(w.to_be_bytes());
    v.extend(h.to_be_bytes());
    v.extend([8, 6, 0, 0, 0]);
    v
}

#[tokio::test]
async fn each_locale_menu_gets_a_keyed_rich_menu() {
    let h = Harness::new();
    let secrets = Secrets::from_lookup(|k| match k {
        "LINE_LIFF_ID" => Some("1234567890-AbCdEfGh".into()),
        "TELEGRAM_WEB_APP_URL" => Some("https://app.seal-bay.invalid/tg".into()),
        _ => None,
    });
    let d = builder(h.services(), secrets)
        .channel(h.fake_channel())
        .build()
        .unwrap();
    let cfg = LineConfig::new("seal-bay-secret", "seal-bay-token", "")
        .with_menu_images(png(2500, 843), png(2500, 1686));
    let line = LineChannel::new(cfg, Arc::new(h.http().clone()));

    let to = ChannelAccountId::new(KIND, "U5ea1ba7000000000000000000000c0de");
    for (locale, key) in [(Locale::th(), "th"), (Locale::en(), "en")] {
        h.http().respond_with(200, r#"{"richmenus":[]}"#); // list
        h.http().respond_with(
            200,
            r#"{"richMenuId":"richmenu-0000000000000000000000000000c0de"}"#,
        );
        line.set_menu(&to, Some(&d.menu_for(&locale)))
            .await
            .unwrap();
        let created = h
            .http()
            .take_requests()
            .into_iter()
            .find(|r| r.uri.ends_with("/v2/bot/richmenu"))
            .unwrap();
        let body = String::from_utf8_lossy(&created.body);
        let want = format!(r#""name":"harpseal-{key}-1-"#);
        assert!(body.contains(&want), "{body}");
    }
}
