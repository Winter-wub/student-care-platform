//! Texts have their own Permission (#165): `kernel.texts.edit` opens the
//! Texts page, its search and `texts::set_override`, and nothing else; the
//! privacy notice version, branding and the LINE package stay
//! `kernel.setting.edit`. A Role that held `kernel.setting.edit` before
//! the split keeps editing texts, in the database (migration 0019) and in
//! `[roles]`.

mod admin_support;
mod support;

use admin_support::{CONFIG, flash, htmx_get, run, signed_in};
use harpseal::{Actor, DeploymentConfig, Error, RequestCtx};
use harpseal_testing::{Harness, TestApp, urlencode};

const WELCOME: &str = "/admin/texts/kernel.flow.welcome";
const CONSENT_FORM: &str = "action=\"/admin/texts/consent\"";

/// The admin Deployment plus `Wording`, holding only the new Permission
/// (Student Affairs editing texts). Its Administrator lists only
/// `kernel.setting.edit`, as a Deployment written before the split does.
async fn boot() -> (Harness, TestApp) {
    let roles = CONFIG.replace("[roles]\n", "[roles]\nWording = [\"kernel.texts.edit\"]\n");
    let h = Harness::new();
    let config = DeploymentConfig::from_toml(&roles).unwrap();
    let app = h.boot(h.builder().config(config).build().unwrap()).await;
    for sql in support::ROSTER {
        run(&h, sql).await;
    }
    (h, app)
}

fn save(value: &str) -> String {
    format!("locale=th&value={}&version=-&op=save", urlencode(value))
}

/// Creates Role `name` in the database with `perms`, as an Administrator
/// would on the Roles page.
async fn db_role(h: &Harness, name: &str, perms: &[&str]) {
    run(
        h,
        &format!("INSERT INTO role (id, name, created_at) VALUES ('r-{name}', '{name}', 1)"),
    )
    .await;
    for p in perms {
        run(
            h,
            &format!(
                "INSERT INTO role_permission (role_id, permission_id) VALUES ('r-{name}', '{p}')"
            ),
        )
        .await;
    }
}

async fn perms_of(app: &TestApp, role: &str) -> Vec<String> {
    harpseal::role_permissions(app.deployment().services(), role)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_role_with_only_the_texts_permission_edits_texts_and_no_other_setting() {
    let (h, app) = boot().await;
    let (b, id) = signed_in(&h, &app, "Wording").await;
    let list = b.get("/admin/texts").await;
    assert_eq!(list.status, 200);
    assert!(list.body.contains("href=\"/admin/texts\""), "in the nav");
    assert!(
        !list.body.contains(CONSENT_FORM),
        "no privacy notice version"
    );
    let found = htmx_get(&b, "/admin/texts?q=welcome").await;
    assert_eq!(found.status, 200);
    assert!(found.body.contains("kernel.flow.welcome"));
    assert_eq!(b.get(WELCOME).await.status, 200);
    let r = b.post(WELCOME, &save("ยินดีต้อนรับ (ฝ่ายกิจการนักศึกษา)")).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "ok:kernel.admin.saved");

    // Every other setting is refused and writes nothing.
    let r = b.post("/admin/texts/consent", "version=2&expected=-").await;
    assert_eq!(r.status, 403);
    assert_eq!(b.get("/admin/branding").await.status, 403);
    let r = b
        .post("/admin/usage/package", "package=pro&custom=&expected=-")
        .await;
    assert_eq!(r.status, 403);
    assert_eq!(
        app.count("SELECT count(*) FROM setting WHERE locale = ''")
            .await,
        0
    );

    // The public write takes the same Permission, audited as before.
    let (d, ctx) = (app.deployment(), RequestCtx::cli());
    let actor = Actor::Staff(id);
    let saved = harpseal::texts::set_override(d, "kernel.menu.pick", "th", "เลือกเมนู", &actor, &ctx);
    assert!(saved.await.unwrap());
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'setting.update'")
            .await,
        2
    );
    assert_eq!(harpseal::texts::TEXTS_PERMISSION, "kernel.texts.edit");
}

#[tokio::test]
async fn the_setting_permission_without_texts_sees_only_the_version_on_the_texts_page() {
    let (h, app) = boot().await;
    // Only an Administrator taking texts away on /admin/roles makes this.
    db_role(&h, "Settings", &["kernel.setting.edit"]).await;
    let (b, id) = signed_in(&h, &app, "Settings").await;
    let page = b.get("/admin/texts").await;
    assert_eq!(page.status, 200);
    assert!(page.body.contains(CONSENT_FORM), "the version card");
    assert!(page.body.contains("href=\"/admin/texts\""), "in the nav");
    assert!(!page.body.contains("id=\"texts-q\""), "no search");
    assert!(!page.body.contains(WELCOME), "no texts");
    assert_eq!(b.get(WELCOME).await.status, 403);
    assert_eq!(b.post(WELCOME, &save("x")).await.status, 403);
    // The version is still theirs: its review step answers.
    let review = b.post("/admin/texts/consent", "version=2&expected=-").await;
    assert_eq!(review.status, 200, "{}", review.body);
    // Staff holding neither get the guard's 403.
    let (viewer, _) = signed_in(&h, &app, "Viewer").await;
    let r = viewer.get("/admin/texts?q=welcome").await;
    assert_eq!(r.status, 403);
    assert!(r.body.contains("<code>kernel.texts.edit</code>"));
    assert_eq!(b.get("/admin/branding").await.status, 200);
    let (ctx, actor) = (RequestCtx::cli(), Actor::Staff(id));
    let d = app.deployment();
    match harpseal::texts::set_override(d, "kernel.flow.welcome", "th", "x", &actor, &ctx).await {
        Err(Error::Forbidden { permission }) => assert_eq!(permission, "kernel.texts.edit"),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_toml_role_listing_only_the_setting_permission_keeps_texts() {
    let (h, app) = boot().await;
    let perms = perms_of(&app, "Administrator").await;
    assert!(perms.iter().any(|p| p == "kernel.texts.edit"), "{perms:?}");
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let list = admin.get("/admin/texts").await;
    assert_eq!(list.status, 200);
    assert!(list.body.contains(CONSENT_FORM), "and the version card");
    let report = app.deployment().migrate().await.unwrap();
    let notice = report
        .notices
        .iter()
        .find(|n| n.contains("[roles] Administrator") && n.contains("kernel.texts.edit"));
    assert!(notice.is_some(), "{:?}", report.notices);
}

#[tokio::test]
async fn a_seeded_role_whose_setting_permission_was_taken_away_gets_no_texts() {
    let (h, app) = boot().await;
    // Before #165: the Administrator from `[roles]` was seeded with
    // kernel.setting.edit, which an Administrator then took away on
    // /admin/roles. Neither 0019 nor the texts pair existed yet.
    for sql in [
        "DELETE FROM role_seed WHERE role_name = 'Administrator' \
         AND permission_id = 'kernel.texts.edit'",
        "DELETE FROM role_permission WHERE role_id = \
         (SELECT id FROM role WHERE name = 'Administrator') \
         AND permission_id IN ('kernel.texts.edit', 'kernel.setting.edit')",
        "DELETE FROM migration WHERE name = '0019_texts_edit_permission'",
    ] {
        run(&h, sql).await;
    }
    app.deployment().migrate().await.unwrap();
    let perms = perms_of(&app, "Administrator").await;
    assert!(
        !perms
            .iter()
            .any(|p| p.starts_with("kernel.setting") || p == "kernel.texts.edit"),
        "{perms:?}"
    );
    let recorded = app
        .count(
            "SELECT count(*) FROM role_seed WHERE role_name = 'Administrator' \
             AND permission_id = 'kernel.texts.edit'",
        )
        .await;
    assert_eq!(recorded, 1, "seeded, so never granted later");
    app.deployment().migrate().await.unwrap();
    assert_eq!(perms_of(&app, "Administrator").await, perms);
}

#[tokio::test]
async fn existing_roles_with_the_setting_permission_lose_nothing_after_migrate() {
    let (h, app) = boot().await;
    // Roles as an Administrator made them before the split.
    db_role(&h, "Editor", &["kernel.audit.view", "kernel.setting.edit"]).await;
    db_role(&h, "Auditor", &["kernel.audit.view"]).await;
    db_role(&h, "Writer", &["kernel.texts.edit"]).await;
    // Put the database back to before 0019 ran, then upgrade.
    run(
        &h,
        "DELETE FROM migration WHERE name = '0019_texts_edit_permission'",
    )
    .await;
    let report = app.deployment().migrate().await.unwrap();
    assert_eq!(report.applied.len(), 1, "{report:?}");
    assert_eq!(
        perms_of(&app, "Editor").await,
        [
            "kernel.audit.view",
            "kernel.setting.edit",
            "kernel.texts.edit"
        ]
    );
    assert_eq!(perms_of(&app, "Auditor").await, ["kernel.audit.view"]);
    assert_eq!(perms_of(&app, "Writer").await, ["kernel.texts.edit"]);
    let (b, _) = signed_in(&h, &app, "Editor").await;
    assert_eq!(b.get("/admin/texts").await.status, 200);

    // It ran once: taking texts away afterwards is not undone.
    run(
        &h,
        "DELETE FROM role_permission WHERE role_id = 'r-Editor' \
         AND permission_id = 'kernel.texts.edit'",
    )
    .await;
    assert!(app.deployment().migrate().await.unwrap().applied.is_empty());
    assert_eq!(
        perms_of(&app, "Editor").await,
        ["kernel.audit.view", "kernel.setting.edit"]
    );
}
