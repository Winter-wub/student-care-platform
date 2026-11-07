//! The unlink Permission (#80): `kernel.link.revoke` is Staff unlink,
//! `kernel.link.manage` is Manual Link only. The Student detail page shows
//! each action only to Staff holding its Permission, and the routes refuse
//! (403) without it. A Role that held `kernel.link.manage` before the split
//! keeps both, in the database (migration 0012) and in `[roles]`.

mod students_support;
mod support;

use harpseal::DeploymentConfig;
use harpseal_testing::{Harness, TestApp};
use students_support::{audit_json, flash, run, signed_in};
use support::{Chat, ROSTER, live_link};

/// `UnlinkOnly` lists the new Permission alone; `Legacy` lists only the old
/// one, as a Deployment written before the split does; `Both` lists both.
/// `ManualOnly` cannot be written in TOML (the old id alone means both), so
/// the tests create it in the database.
const CONFIG: &str = r#"
name = "Seal Bay University (link permissions)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
UnlinkOnly = ["kernel.student.view", "kernel.link.revoke"]
Legacy = ["kernel.student.view", "kernel.link.manage"]
Both = ["kernel.student.view", "kernel.link.manage", "kernel.link.revoke"]
"#;

async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    let app = h.boot(h.builder().config(config).build().unwrap()).await;
    for sql in ROSTER {
        run(&h, sql).await;
    }
    (h, app)
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

/// Student s1 linked from `u1`, and the revoke path of that Link.
async fn linked(h: &Harness, app: &TestApp) -> String {
    let mut c = Chat::new(h, app, "u1");
    c.link("68010001").await;
    format!(
        "/admin/students/s1/links/{}/revoke",
        live_link(h, "u1").await
    )
}

const MANUAL_FORM: &str = "action=\"/admin/students/s1/links\"";

#[tokio::test]
async fn unlink_only_can_revoke_but_not_manual_link() {
    let (h, app) = boot().await;
    let path = linked(&h, &app).await;
    let (b, staff_id) = signed_in(&h, &app, "UnlinkOnly").await;

    let detail = b.get("/admin/students/s1").await.body;
    assert!(
        detail.contains(&format!("action=\"{path}\"")),
        "unlink shown"
    );
    // Student s3 has no Link; the Manual Link form is not offered either.
    assert!(!detail.contains(MANUAL_FORM));
    assert!(!b.get("/admin/students/s3").await.body.contains("/links\""));

    h.fake_channel().take_sent();
    let manual = b
        .post("/admin/students/s3/links", "channel=fake&external_id=u1")
        .await;
    assert_eq!(manual.status, 403, "{}", manual.body);
    assert_eq!(
        app.count("SELECT count(*) FROM link").await,
        1,
        "no Link made"
    );
    assert_eq!(
        app.count(
            "SELECT count(*) FROM audit_log WHERE action = 'link.create' AND actor_kind = 'staff'"
        )
        .await,
        0
    );

    assert_eq!(b.post(&path, "").await.status, 200, "asks first");
    let r = b.post(&path, "confirm=yes").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "ok:kernel.students.revoked");
    // The audit row is the one the single Permission wrote before the split.
    let (before, after) = audit_json(&h, "link.revoke").await;
    assert_eq!(before, serde_json::json!({ "status": "active" }));
    assert_eq!(after, serde_json::json!({ "status": "revoked" }));
    let actor = app
        .query("SELECT actor_kind, actor_id FROM audit_log WHERE action = 'link.revoke'")
        .await;
    assert_eq!(actor.get(0, "actor_kind").unwrap().as_text(), Some("staff"));
    assert_eq!(
        actor.get(0, "actor_id").unwrap().as_text(),
        Some(staff_id.as_str())
    );
}

#[tokio::test]
async fn manual_only_can_link_but_not_revoke() {
    let (h, app) = boot().await;
    db_role(
        &h,
        "ManualOnly",
        &["kernel.student.view", "kernel.link.manage"],
    )
    .await;
    let path = linked(&h, &app).await;
    let mut c = Chat::new(&h, &app, "u9");
    c.follow().await;
    let (b, staff_id) = signed_in(&h, &app, "ManualOnly").await;

    let detail = b.get("/admin/students/s1").await.body;
    assert!(!detail.contains(&path), "no unlink button");
    assert!(!detail.contains("/revoke"));
    assert!(
        b.get("/admin/students/s3")
            .await
            .body
            .contains("action=\"/admin/students/s3/links\"")
    );

    assert_eq!(
        b.post(&path, "").await.status,
        403,
        "not even the confirmation"
    );
    assert_eq!(b.post(&path, "confirm=yes").await.status, 403);
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status = 'active'")
            .await,
        1,
        "the Link stands"
    );
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'link.revoke'")
            .await,
        0
    );

    let r = b
        .post("/admin/students/s3/links", "channel=fake&external_id=u9")
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "ok:kernel.students.linked");
    let row = app
        .query("SELECT verified_by_staff_id FROM link WHERE method = 'manual'")
        .await;
    assert_eq!(
        row.get(0, "verified_by_staff_id").unwrap().as_text(),
        Some(staff_id.as_str())
    );
    assert_eq!(
        app.count(
            "SELECT count(*) FROM audit_log WHERE action = 'link.create' AND actor_kind = 'staff'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn both_permissions_show_both_actions() {
    let (h, app) = boot().await;
    let path = linked(&h, &app).await;
    let (b, _) = signed_in(&h, &app, "Both").await;
    let detail = b.get("/admin/students/s1").await.body;
    assert!(detail.contains(&format!("action=\"{path}\"")));
    assert!(
        b.get("/admin/students/s3")
            .await
            .body
            .contains("action=\"/admin/students/s3/links\"")
    );
    assert_eq!(b.post(&path, "confirm=yes").await.status, 303);
}

#[tokio::test]
async fn a_toml_role_with_only_the_old_permission_gets_both() {
    let (h, app) = boot().await;
    assert_eq!(
        perms_of(&app, "Legacy").await,
        [
            "kernel.link.manage",
            "kernel.link.revoke",
            "kernel.student.view"
        ]
    );
    assert_eq!(
        perms_of(&app, "UnlinkOnly").await,
        ["kernel.link.revoke", "kernel.student.view"],
        "a config listing only the new one gets only unlink"
    );
    assert_eq!(
        perms_of(&app, "Both").await,
        [
            "kernel.link.manage",
            "kernel.link.revoke",
            "kernel.student.view"
        ]
    );
    // A Legacy Staff member can do both, as before the split.
    let path = linked(&h, &app).await;
    let (b, _) = signed_in(&h, &app, "Legacy").await;
    assert!(
        b.get("/admin/students/s3")
            .await
            .body
            .contains(MANUAL_FORM.replace("s1", "s3").as_str())
    );
    assert_eq!(b.post(&path, "confirm=yes").await.status, 303);
}

#[tokio::test]
async fn migrate_tells_the_author_which_roles_relied_on_the_legacy_rule() {
    let (_h, app) = boot().await;
    let report = app.deployment().migrate().await.unwrap();
    assert_eq!(report.notices.len(), 1, "{:?}", report.notices);
    assert!(
        report.notices[0].contains("[roles] Legacy"),
        "{:?}",
        report.notices
    );
    assert!(report.notices[0].contains("kernel.link.revoke"));
}

#[tokio::test]
async fn the_migration_grants_revoke_to_every_database_role_that_managed_links() {
    let (h, app) = boot().await;
    // Roles as an Administrator made them before the split.
    db_role(
        &h,
        "Counsellor",
        &["kernel.student.view", "kernel.link.manage"],
    )
    .await;
    db_role(&h, "Auditor", &["kernel.audit.view"]).await;
    db_role(&h, "Greeter", &["kernel.link.revoke"]).await;
    // Put the database back to before 0012 ran, then upgrade.
    run(
        &h,
        "DELETE FROM migration WHERE name = '0012_link_revoke_permission'",
    )
    .await;
    let report = app.deployment().migrate().await.unwrap();
    assert_eq!(report.applied.len(), 1, "{report:?}");

    assert_eq!(
        perms_of(&app, "Counsellor").await,
        [
            "kernel.link.manage",
            "kernel.link.revoke",
            "kernel.student.view"
        ]
    );
    assert_eq!(
        perms_of(&app, "Auditor").await,
        ["kernel.audit.view"],
        "untouched"
    );
    assert_eq!(
        perms_of(&app, "Greeter").await,
        ["kernel.link.revoke"],
        "not widened"
    );

    // It ran once: taking revoke away afterwards is not undone.
    run(
        &h,
        "DELETE FROM role_permission WHERE role_id = 'r-Counsellor' \
         AND permission_id = 'kernel.link.revoke'",
    )
    .await;
    let again = app.deployment().migrate().await.unwrap();
    assert!(again.applied.is_empty());
    assert_eq!(
        perms_of(&app, "Counsellor").await,
        ["kernel.link.manage", "kernel.student.view"]
    );
}

#[test]
fn the_example_deployment_gives_its_roles_both_permissions() {
    let config = DeploymentConfig::from_toml(include_str!(
        "../../../deployments/example/deployment/deployment.toml"
    ))
    .unwrap();
    for role in ["Administrator", "Officer"] {
        let perms = &config.roles[role];
        for want in ["kernel.link.manage", "kernel.link.revoke"] {
            assert!(perms.iter().any(|p| p == want), "{role} lacks {want}");
        }
    }
    let viewer = &config.roles["Viewer"];
    assert!(!viewer.iter().any(|p| p.starts_with("kernel.link.")));
}
