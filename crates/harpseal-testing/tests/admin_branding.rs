//! `/admin/branding` (#114) at the Deployment seam: only Staff holding
//! `kernel.setting.edit` reach it and every post needs the CSRF token;
//! bad values are refused in Thai with nothing written; the preview draws
//! the headers and saves nothing; a save is audited and shows in the
//! back office, the sign-in page and the Mini App, with the logo served
//! with its real type and `nosniff`; a reset returns to the TOML and
//! deletes the logo no setting names; a backup carries the settings and
//! the logo. The Fake Channel has no Menu images: that part says so.

mod branding_support;

use branding_support::*;
use harpseal::backup::{self, LoadOptions, MemoryBackup};
use harpseal_testing::Harness;

#[tokio::test]
async fn only_holders_of_the_setting_permission_reach_the_page_and_posts_need_csrf() {
    let h = Harness::new();
    let app = boot(&h).await;
    let anonymous = app.browser();
    assert_eq!(anonymous.get(PAGE).await.status, 303, "to the sign-in page");
    let viewer = signed_in(&h, &app, "Viewer").await;
    assert_eq!(viewer.get(PAGE).await.status, 403);
    let r = post(
        &viewer,
        SAVE,
        &[("op", b"save"), ("name", b"x"), ("color", b"#1b4f72")],
    )
    .await;
    assert_eq!(r.status, 403);

    let admin = signed_in(&h, &app, "Administrator").await;
    let page = admin.get(PAGE).await;
    assert_eq!(page.status, 200);
    assert!(page.body.contains("แบรนด์และรูปเมนู"), "listed for the admin");
    assert!(
        page.body.contains("ยังไม่มีช่องทางที่ใช้รูปเมนู"),
        "the Fake Channel draws no Menu image"
    );
    let parts: [(&str, &[u8]); 3] = [("op", b"save"), ("name", b"x"), ("color", b"#1b4f72")];
    let forged = post_raw(&admin, SAVE, &parts).await;
    assert_eq!(forged.status, 403, "no CSRF token");
    assert!(settings(&h).await.is_empty());
}

#[tokio::test]
async fn bad_values_are_refused_in_thai_and_nothing_is_written() {
    let h = Harness::new();
    let app = boot(&h).await;
    let admin = signed_in(&h, &app, "Administrator").await;
    admin.get(PAGE).await;
    let r = post(
        &admin,
        SAVE,
        &[
            ("op", b"save"),
            ("name", b"   "),
            ("color", b"#ffd400"),
            ("logo", b"<svg onload=alert(1)/>"),
        ],
    )
    .await;
    assert_eq!(r.status, 422);
    assert!(r.body.contains("กรุณาใส่ชื่อสถาบัน"), "{}", r.body);
    assert!(r.body.contains("ตัวอักษรสีขาวบนสีนี้อ่านยาก"));
    assert!(r.body.contains("โลโก้ต้องเป็นไฟล์ PNG หรือ JPEG"));
    let r = post(
        &admin,
        SAVE,
        &[("op", b"save"), ("name", b"Seal Bay"), ("color", b"blue")],
    )
    .await;
    assert_eq!(r.status, 422);
    assert!(r.body.contains("สีต้องอยู่ในรูปแบบ #RRGGBB"));
    let big = png(4000, 300);
    let r = post(
        &admin,
        SAVE,
        &[
            ("op", b"save"),
            ("name", b"Seal Bay"),
            ("color", b"#1b4f72"),
            ("logo", &big),
        ],
    )
    .await;
    assert!(r.body.contains("โลโก้ขนาด 4000 x 300 px"), "{}", r.body);
    assert!(settings(&h).await.is_empty());
    assert!(audits(&h, "branding.update").await.is_empty());
    assert!(h.files().keys().is_empty());
}

#[tokio::test]
async fn the_preview_draws_both_headers_and_saves_nothing_then_a_save_uses_its_logo() {
    let h = Harness::new();
    let app = boot(&h).await;
    let admin = signed_in(&h, &app, "Administrator").await;
    admin.get(PAGE).await;
    let logo = png(200, 80);
    let r = post(
        &admin,
        SAVE,
        &[
            ("op", b"preview"),
            ("name", "มหาวิทยาลัยหาดแมวน้ำ".as_bytes()),
            ("color", b"#1B4F72"),
            ("logo", &logo),
        ],
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(r.body.contains("ตัวอย่าง (ยังไม่ได้บันทึก)"));
    assert!(r.body.contains("--brand:#1b4f72"));
    assert!(r.body.contains("แถบบนของหลังบ้าน") && r.body.contains("แถบบนของมินิแอป"));
    assert!(r.body.contains("src=\"data:image/png;base64,"));
    assert!(settings(&h).await.is_empty() && h.files().keys().is_empty());
    let home = admin.get("/admin").await.body;
    assert!(home.contains("Seal Bay University (branding)") && !home.contains("หาดแมวน้ำ"));

    // The preview carries the logo on: saving needs no second upload.
    let mark = "name=\"logo_draft\" value=\"";
    let at = r.body.find(mark).unwrap() + mark.len();
    let draft = &r.body[at..at + r.body[at..].find('"').unwrap()];
    let r = post(
        &admin,
        SAVE,
        &[
            ("op", b"save"),
            ("name", "มหาวิทยาลัยหาดแมวน้ำ".as_bytes()),
            ("color", b"#1b4f72"),
            ("logo_draft", draft.as_bytes()),
        ],
    )
    .await;
    assert_eq!(r.status, 303, "{}", r.body);
    let keys: Vec<String> = settings(&h).await.into_iter().map(|(k, _)| k).collect();
    assert_eq!(
        keys,
        [
            "kernel.branding.logo_file_key",
            "kernel.branding.name",
            "kernel.branding.primary_color"
        ]
    );
    assert_eq!(h.files().keys().len(), 1);
}

#[tokio::test]
async fn a_save_is_audited_and_used_in_every_chrome_and_a_reset_returns_to_the_toml() {
    let h = Harness::new();
    let app = boot(&h).await;
    let admin = signed_in(&h, &app, "Administrator").await;
    admin.get(PAGE).await;
    let logo = png(200, 80);
    let r = post(
        &admin,
        SAVE,
        &[
            ("op", b"save"),
            ("name", "มหาวิทยาลัยหาดแมวน้ำ".as_bytes()),
            ("color", b"#1b4f72"),
            ("logo", &logo),
        ],
    )
    .await;
    assert_eq!(r.status, 303);
    let (before, after) = audits(&h, "branding.update").await.remove(0);
    assert!(before.contains("\"name\":null"), "{before}");
    assert!(
        after.contains("หาดแมวน้ำ") && after.contains("#1b4f72"),
        "{after}"
    );
    assert!(after.contains("branding/") && after.contains(".png"));

    let home = admin.get("/admin").await.body;
    assert!(home.contains("--brand:#1b4f72"), "back office");
    assert!(home.contains("มหาวิทยาลัยหาดแมวน้ำ"));
    let src = {
        let at = home.find("src=\"/branding/logo?v=").unwrap() + 5;
        home[at..at + home[at..].find('"').unwrap()].to_string()
    };
    let notice = app.get("/notice").await.body;
    assert!(notice.contains("class=\"mini-bar\""), "Mini App chrome");
    assert!(notice.contains("มหาวิทยาลัยหาดแมวน้ำ") && notice.contains(&src));
    assert!(notice.contains("--brand:#1b4f72"));
    let login = app.browser().get("/admin/login").await.body;
    assert!(login.contains(&src), "sign-in page, no session");

    let served = app.get(&src).await;
    assert_eq!(served.status, 200);
    assert_eq!(served.header("content-type"), Some("image/png"));
    assert_eq!(served.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(served.bytes, logo);

    admin.get(PAGE).await;
    let r = post(&admin, SAVE, &[("op", b"reset")]).await;
    assert_eq!(r.status, 303);
    assert!(settings(&h).await.is_empty());
    assert_eq!(audits(&h, "branding.reset").await.len(), 1);
    assert!(h.files().keys().is_empty(), "no setting names the logo");
    let home = admin.get("/admin").await.body;
    assert!(home.contains("--brand:#0b5cad") && home.contains("Seal Bay University (branding)"));
    assert_eq!(app.get(&src).await.status, 404);
}

#[tokio::test]
async fn a_backup_carries_the_settings_and_the_logo() {
    let h = Harness::new();
    let app = boot(&h).await;
    let admin = signed_in(&h, &app, "Administrator").await;
    admin.get(PAGE).await;
    let logo = png(120, 120);
    let r = post(
        &admin,
        SAVE,
        &[
            ("op", b"save"),
            ("name", "มหาวิทยาลัยหาดแมวน้ำ".as_bytes()),
            ("color", b"#1b4f72"),
            ("logo", &logo),
        ],
    )
    .await;
    assert_eq!(r.status, 303);
    let mut sink = MemoryBackup::new();
    backup::dump(app.deployment(), &mut sink).await.unwrap();

    let fresh = Harness::new();
    let d = fresh.builder().config(config()).build().unwrap();
    backup::load(&d, &sink, &LoadOptions::new()).await.unwrap();
    assert_eq!(settings(&fresh).await, settings(&h).await);
    assert_eq!(fresh.files().keys(), h.files().keys());
    let restored = fresh.boot(d).await;
    let home = restored.get("/notice").await.body;
    assert!(home.contains("มหาวิทยาลัยหาดแมวน้ำ") && home.contains("--brand:#1b4f72"));
    let at = home.find("/branding/logo?v=").unwrap();
    let src = &home[at..at + home[at..].find('"').unwrap()];
    assert_eq!(restored.get(src).await.bytes, logo);
}
