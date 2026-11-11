//! The privacy notice texts stay with `kernel.setting.edit` (#170): the
//! example's Student Affairs Officer holds `kernel.texts.edit` alone, so it
//! finds the notice texts in search and the to-fix chips but reads them
//! only, with a Thai note; saving, resetting and `texts::set_override` are
//! refused 403 and write nothing. The Administrator holds both and saves.

use example_deployment::{Secrets, builder};
use harpseal::{Actor, Error, RequestCtx};
use harpseal_testing::{Browser, Harness, TestApp, urlencode};

const PASSWORD: &str = "kelp forest at dawn";
const NOTICE: &str = "/admin/texts/kernel.consent.notice";
const NOTE: &str = "data-notice-locked";
const NOTE_TH: &str = "ข้อความนี้เป็นส่วนหนึ่งของประกาศความเป็นส่วนตัวที่นักศึกษายอมรับไว้";

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

fn save(value: &str) -> String {
    format!("locale=th&value={}&version=-&op=save", urlencode(value))
}

async fn overrides(app: &TestApp) -> i64 {
    app.count("SELECT count(*) FROM setting WHERE key LIKE 'kernel.consent.%' OR key LIKE 'kernel.notice.%'")
        .await
}

#[tokio::test]
async fn the_officer_reads_the_privacy_notice_texts_and_cannot_save_them() {
    let (h, app) = boot().await;
    let (b, id) = signed_in(&h, &app, "Officer").await;

    // Search and the to-fix chips still list them.
    let found = b.get("/admin/texts?q=kernel.consent.notice").await;
    assert_eq!(found.status, 200);
    assert!(found.body.contains(NOTICE), "listed in search");
    let all = b.get("/admin/texts").await.body;
    assert!(all.contains("/admin/texts/kernel.notice.body"), "listed");

    // Read-only, with the note.
    let page = b.get(NOTICE).await;
    assert_eq!(page.status, 200);
    assert!(page.body.contains(NOTE) && page.body.contains(NOTE_TH));
    assert!(page.body.contains("readonly"));
    assert!(!page.body.contains("value=\"save\""), "no save button");

    // The page and the public write refuse, naming kernel.setting.edit.
    let r = b.post(NOTICE, &save("ถ้อยคำใหม่")).await;
    assert_eq!(r.status, 403);
    assert!(r.body.contains("<code>kernel.setting.edit</code>"));
    let reset = "locale=th&value=&version=-&op=reset";
    assert_eq!(b.post(NOTICE, reset).await.status, 403);
    let (d, ctx, actor) = (app.deployment(), RequestCtx::cli(), Actor::Staff(id));
    for key in ["kernel.consent.notice", "kernel.notice.title"] {
        match harpseal::texts::set_override(d, key, "th", "ถ้อยคำใหม่", &actor, &ctx).await
        {
            Err(Error::Forbidden { permission }) => assert_eq!(permission, "kernel.setting.edit"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
    assert_eq!(overrides(&app).await, 0);
    assert_eq!(
        harpseal::texts::permissions_for("kernel.consent.accept"),
        ["kernel.texts.edit", "kernel.setting.edit"]
    );
    assert_eq!(
        harpseal::texts::permissions_for("contact.reply"),
        ["kernel.texts.edit"]
    );

    // Any other text is still theirs.
    let r = b.post("/admin/texts/contact.reply", &save("อาคาร 1")).await;
    assert_eq!(r.status, 303, "{}", r.body);
    let audited = "SELECT count(*) FROM audit_log WHERE action = 'setting.update'";
    assert_eq!(
        app.count(audited).await,
        1,
        "only that one, audited as before"
    );
}

#[tokio::test]
async fn the_administrator_saves_the_privacy_notice_texts_as_before() {
    let (h, app) = boot().await;
    let (admin, id) = signed_in(&h, &app, "Administrator").await;
    let page = admin.get(NOTICE).await;
    assert_eq!(page.status, 200);
    assert!(!page.body.contains(NOTE));
    assert!(page.body.contains("value=\"save\""));
    let r = admin.post(NOTICE, &save("ประกาศฉบับปรับถ้อยคำ")).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(
        admin.cookie("hs_flash").as_deref(),
        Some("ok:kernel.admin.saved")
    );
    let (d, ctx, actor) = (app.deployment(), RequestCtx::cli(), Actor::Staff(id));
    let saved =
        harpseal::texts::set_override(d, "kernel.notice.title", "en", "Privacy", &actor, &ctx);
    assert!(saved.await.unwrap());
    assert_eq!(overrides(&app).await, 2);
    let audited = "SELECT count(*) FROM audit_log WHERE action = 'setting.update'";
    assert_eq!(app.count(audited).await, 2);
}
