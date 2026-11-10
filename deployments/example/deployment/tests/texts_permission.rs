//! The example's Student Affairs Officer edits texts (#165): its Role lists
//! `kernel.texts.edit` and not `kernel.setting.edit`, so it opens the
//! Texts page and saves a text, and is refused the privacy notice version
//! and branding. The Roles page reads the new Permission in Thai.

use example_deployment::{Secrets, builder};
use harpseal::{Actor, RequestCtx};
use harpseal_testing::{Browser, Harness, TestApp, urlencode};

const PASSWORD: &str = "kelp forest at dawn";
const CONSENT_FORM: &str = "action=\"/admin/texts/consent\"";

async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let secrets = Secrets::from_lookup(|k| match k {
        "LINE_LIFF_ID" => Some("1234567890-AbCdEfGh".into()),
        "TELEGRAM_WEB_APP_URL" => Some("https://app.seal-bay.invalid/tg".into()),
        _ => None,
    });
    let d = builder(h.services(), secrets)
        .unwrap()
        .channel(h.fake_channel())
        .build()
        .expect("the example Deployment builds");
    let app = h.boot(d).await;
    (h, app)
}

/// A signed-in browser for a new Staff member holding `role`, and its id.
async fn signed_in<'a>(h: &Harness, app: &'a TestApp, role: &str) -> (Browser<'a>, String) {
    let email = format!("{}@seal-bay.invalid", role.to_lowercase());
    let id = h.create_staff(&email, PASSWORD, &[role]).await;
    let b = app.browser();
    assert_eq!(b.login(&email, PASSWORD).await.status, 303);
    assert_eq!(b.get("/admin").await.status, 200);
    (b, id)
}

#[tokio::test]
async fn the_officer_edits_texts_but_not_the_other_settings() {
    let (h, app) = boot().await;
    let (b, id) = signed_in(&h, &app, "Officer").await;
    let list = b.get("/admin/texts").await;
    assert_eq!(list.status, 200);
    assert!(
        !list.body.contains(CONSENT_FORM),
        "no privacy notice version"
    );
    let form = format!(
        "locale=th&value={}&version=-&op=save",
        urlencode("ติดต่อกองกิจการนักศึกษา อาคาร 1 ชั้น 2")
    );
    let r = b.post("/admin/texts/contact.reply", &form).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(
        b.cookie("hs_flash").as_deref(),
        Some("ok:kernel.admin.saved")
    );

    let r = b.post("/admin/texts/consent", "version=2&expected=-").await;
    assert_eq!(r.status, 403);
    assert_eq!(b.get("/admin/branding").await.status, 403);

    // A Module's own code writes a text with the same Permission.
    let (d, ctx, actor) = (app.deployment(), RequestCtx::cli(), Actor::Staff(id));
    let saved = harpseal::texts::set_override(d, "contact.reply", "en", "Room 2", &actor, &ctx);
    assert!(saved.await.unwrap());

    // The Administrator keeps both.
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    assert!(admin.get("/admin/texts").await.body.contains(CONSENT_FORM));
    assert_eq!(admin.get("/admin/branding").await.status, 200);
}

#[tokio::test]
async fn the_roles_page_reads_the_texts_permission_in_thai() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let page = admin.get("/admin/roles").await.body;
    assert!(page.contains("แก้ไขข้อความที่ระบบใช้ในแชทและหน้าต่าง ๆ"), "{page}");
    assert!(page.contains("value=\"kernel.texts.edit\""));
    let (viewer, _) = signed_in(&h, &app, "Viewer").await;
    assert_eq!(viewer.get("/admin/texts").await.status, 403);
}
