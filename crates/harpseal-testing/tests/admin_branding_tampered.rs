//! `/admin/branding` (#114) against rows it did not write: a backup follows
//! only the branding file settings with a value of their own shape (a
//! Module's `..._file_key` text or a value pointing at another object is
//! data, never a file); a tampered colour, logo key or name never reaches
//! the page head or another object; a form shown before another save is
//! refused.

mod branding_support;

use branding_support::*;
use harpseal::backup::{self, LoadOptions, MemoryBackup};
use harpseal_testing::Harness;

#[tokio::test]
async fn a_backup_follows_only_branding_file_settings_of_their_own_shape() {
    let h = Harness::new();
    let app = boot(&h).await;
    let files = &h.services().files;
    files
        .put(
            "backups/20261107T190000000Z/manifest.json",
            b"{}".to_vec(),
            "application/json",
        )
        .await
        .unwrap();
    // A Module's text setting that happens to end in `_file_key`, holding a
    // URL (no FileStore key at all), and branding settings pointing at
    // another object or in a Locale.
    put_setting(
        &h,
        "dues.receipt_file_key",
        "",
        "https://pay.sealbay.example/r?x=1",
    )
    .await;
    put_setting(
        &h,
        "dues.archive_file_key",
        "",
        "backups/20261107T190000000Z/manifest.json",
    )
    .await;
    put_setting(
        &h,
        "kernel.branding.logo_file_key",
        "",
        "backups/20261107T190000000Z/manifest.json",
    )
    .await;
    let h64 = "b".repeat(64);
    put_setting(
        &h,
        "kernel.branding.menu_unlocked_file_key",
        "th",
        &format!("branding/menu/{h64}.png"),
    )
    .await;
    let mut sink = MemoryBackup::new();
    backup::dump(app.deployment(), &mut sink).await.unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&sink.entries["manifest.json"]).unwrap();
    assert_eq!(manifest["files"], serde_json::json!([]), "{manifest}");
    // The rows themselves are data and are backed up.
    let fresh = Harness::new();
    let d = fresh.builder().config(config()).build().unwrap();
    backup::load(&d, &sink, &LoadOptions::new()).await.unwrap();
    assert_eq!(settings(&fresh).await.len(), 2);
}

#[tokio::test]
async fn tampered_rows_never_reach_the_page_head_or_another_object() {
    let h = Harness::new();
    let app = boot(&h).await;
    h.services()
        .files
        .put(
            "backups/x/manifest.json",
            b"secret".to_vec(),
            "application/json",
        )
        .await
        .unwrap();
    put_setting(
        &h,
        "kernel.branding.primary_color",
        "",
        "red;}body{display:none",
    )
    .await;
    put_setting(
        &h,
        "kernel.branding.logo_file_key",
        "",
        "backups/x/manifest.json",
    )
    .await;
    put_setting(&h, "kernel.branding.name", "", "").await;
    let page = app.get("/notice").await;
    assert!(page.body.contains("--brand:#0b5cad"), "the TOML colour");
    assert!(!page.body.contains("display:none") && !page.body.contains("/branding/logo"));
    assert!(
        page.body.contains("Seal Bay University (branding)"),
        "the TOML name"
    );
    assert_eq!(app.get("/branding/logo?v=x").await.status, 404);
    // A saved colour too light for white text is refused on read as well.
    put_setting(&h, "kernel.branding.primary_color", "", "#ffff00").await;
    h.clock().advance_minutes(1);
    assert!(app.get("/notice").await.body.contains("--brand:#0b5cad"));
}

#[tokio::test]
async fn a_form_shown_before_another_save_is_refused() {
    let h = Harness::new();
    let app = boot(&h).await;
    let admin = signed_in(&h, &app, "Administrator").await;
    let old = seen_in(&admin.get(PAGE).await.body).unwrap();
    // The same Administrator in another tab.
    let other = app.browser();
    let login = other.login("administrator@sealbay.example", PASSWORD).await;
    assert_eq!(login.status, 303);
    let r = post(
        &other,
        SAVE,
        &[
            ("op", b"save"),
            ("name", b"Seal Bay A"),
            ("color", b"#1b4f72"),
        ],
    )
    .await;
    assert_eq!(r.status, 303);
    admin.get(PAGE).await;
    let parts: [(&str, &[u8]); 4] = [
        ("seen", old.as_bytes()),
        ("op", b"save"),
        ("name", b"Seal Bay B"),
        ("color", b"#1b4f72"),
    ];
    let r = post(&admin, SAVE, &parts).await;
    assert_eq!(r.status, 303);
    assert!(
        r.header("set-cookie")
            .unwrap_or_default()
            .contains("conflict")
    );
    let names: Vec<String> = settings(&h).await.into_iter().map(|(_, v)| v).collect();
    assert!(
        names.contains(&"Seal Bay A".to_string()) && !names.contains(&"Seal Bay B".to_string())
    );
    let parts: [(&str, &[u8]); 2] = [("seen", old.as_bytes()), ("op", b"reset")];
    post(&admin, SAVE, &parts).await;
    assert_eq!(settings(&h).await.len(), 2, "a stale reset deletes nothing");
}
