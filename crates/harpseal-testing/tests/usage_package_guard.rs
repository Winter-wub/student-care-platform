//! The LINE package form's guards (#115): the Permission and the CSRF
//! token, a stale form, an unreadable saved value, the audit row of every
//! change, and the backup round trip.

mod usage_support;

use harpseal::backup::{self, LoadOptions, MemoryBackup};
use harpseal::{DeploymentConfig, Statement};
use harpseal_testing::Harness;
use usage_support::package::*;
use usage_support::*;

#[tokio::test]
async fn only_holders_of_the_setting_permission_may_save_and_the_csrf_token_is_checked() {
    let (h, app) = boot_line(true).await;
    let officer = signed_in(&h, &app, "officer", "Officer").await;
    let page = officer.get("/admin/usage").await.body;
    assert!(!page.contains("line-package-choice"), "no form");
    assert!(
        page.contains("ยังไม่ได้เลือกแพ็กเกจ"),
        "but the package is shown"
    );
    let r = officer.post(PATH, "package=pro&custom=&expected=-").await;
    assert_eq!(r.status, 403, "{}", r.body);

    // kernel.setting.edit alone (no usage page) is enough to save.
    let editor = signed_in(&h, &app, "editor", "Editor").await;
    assert_eq!(editor.get("/admin").await.status, 200);
    let r = editor.post(PATH, "package=pro&custom=&expected=-").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&editor), "ok:kernel.usage.package.saved");
    assert_eq!(stored(&h).await.as_deref(), Some("pro"));

    // No token, a wrong token, and no session.
    let (h, app) = boot_line(true).await;
    let b = admin(&h, &app).await;
    b.get("/admin/usage").await;
    let r = b.post_raw(PATH, "package=pro&custom=&expected=-").await;
    assert_eq!(r.status, 403, "{}", r.body);
    let r = b
        .post_raw(PATH, "package=pro&custom=&expected=-&_csrf=wrong")
        .await;
    assert_eq!(r.status, 403, "{}", r.body);
    let r = app.browser().post_raw(PATH, "package=pro&expected=-").await;
    assert!(matches!(r.status.as_u16(), 303 | 401 | 403), "{}", r.status);
    assert_eq!(stored(&h).await, None);
}

#[tokio::test]
async fn a_stale_form_is_refused_and_a_deployment_without_line_has_no_form() {
    let (h, app) = boot_line(true).await;
    let b = admin(&h, &app).await;
    let page = b.get("/admin/usage").await.body; // token "-"
    assert!(page.contains("name=\"expected\" value=\"-\""));
    choose(&b, "basic", "").await;
    let r = b.post(PATH, "package=pro&custom=&expected=-").await;
    assert_eq!(r.status, 303);
    assert_eq!(flash(&b), "err:kernel.admin.conflict");
    assert_eq!(stored(&h).await.as_deref(), Some("basic"));

    // The Fake-only Deployment of the other usage tests has no LINE card.
    let (h2, app2) = usage_support::boot().await;
    let b = signed_in(&h2, &app2, "x", "Administrator").await;
    let body = b.get("/admin/usage").await.body;
    assert!(!body.contains("line-package"));
    let r = b.post(PATH, "package=pro&custom=&expected=-").await;
    assert_eq!(
        flash(&b),
        "err:kernel.usage.package.err_no_line",
        "{}",
        r.status
    );
}

#[tokio::test]
async fn an_unreadable_saved_value_falls_back_to_the_toml_and_is_flagged() {
    let (h, app) = boot_line(true).await;
    let b = admin(&h, &app).await;
    run(
        &h,
        &format!("INSERT INTO setting (key, locale, value, updated_at) VALUES ('{KEY}', '', 'platinum', 0)"),
    )
    .await;
    let body = b.get("/admin/usage").await.body;
    let c = card(&body, "line");
    assert!(c.contains("ใช้ไป 0 จาก 300"), "{c}");
    assert!(c.contains("ค่าที่บันทึกไว้อ่านไม่ได้"), "{c}");
    // Choosing again repairs it; the audit row's value before is the bad one.
    assert_eq!(choose(&b, "pro", "").await, "ok:kernel.usage.package.saved");
    let before = text(
        &h,
        "SELECT before FROM audit_log WHERE action = 'setting.update'",
    )
    .await;
    assert!(before.contains("platinum"), "{before}");
}

#[tokio::test]
async fn every_change_is_audited_with_the_value_before_and_after() {
    let (h, app) = boot_line(true).await;
    let id = staff(&h, "audited", &["Administrator"]).await;
    let b = app.browser();
    assert_eq!(b.login(&email_of("audited"), PASSWORD).await.status, 303);
    choose(&b, "basic", "").await;
    h.clock().advance_minutes(1); // audit rows are ordered by time
    choose(&b, "custom", "4200").await;
    let rows = h
        .db()
        .query(Statement::new(
            "SELECT actor_kind, actor_id, target_kind, target_id, before, after \
             FROM audit_log WHERE action = 'setting.update' ORDER BY at, id",
        ))
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    let col = |i: usize, c: &str| {
        rows.get(i, c)
            .and_then(|v| v.as_text())
            .unwrap()
            .to_string()
    };
    assert_eq!(col(0, "actor_kind"), "staff");
    assert_eq!(col(0, "actor_id"), id);
    assert_eq!(col(0, "target_kind"), "setting");
    assert_eq!(col(0, "target_id"), format!("{KEY}:"));
    let json = |s: String| serde_json::from_str::<serde_json::Value>(&s).unwrap();
    // The first save's value before is the TOML number as Custom.
    assert_eq!(json(col(0, "before"))["value"], "custom:300");
    assert_eq!(json(col(0, "after"))["value"], "basic");
    assert_eq!(json(col(1, "before"))["value"], "basic");
    assert_eq!(json(col(1, "after"))["value"], "custom:4200");
    // Refusals write nothing.
    choose(&b, "custom", "nope").await;
    choose(&b, "custom", "4200").await;
    assert_eq!(audits(&h, "setting.update").await, 2);
}

#[tokio::test]
async fn a_backup_carries_the_choice_and_the_restored_deployment_uses_it() {
    let (h, app) = boot_line(true).await;
    staff(&h, "keeper", &["Administrator"]).await;
    let b = app.browser();
    assert_eq!(b.login(&email_of("keeper"), PASSWORD).await.status, 303);
    seed(&h, "line", TODAY, 1000, 0, 0).await;
    choose(&b, "basic", "").await;

    let mut dump = MemoryBackup::new();
    backup::dump(app.deployment(), &mut dump).await.unwrap();
    assert!(dump.entries.contains_key("tables/setting.ndjson"));

    let fresh = Harness::new();
    let toml = format!("{CONFIG_LINE}{TOML_CAP}");
    let d = fresh
        .builder()
        .config(DeploymentConfig::from_toml(&toml).unwrap())
        .channel(fresh.fake_channel().with_kind("line"))
        .build()
        .unwrap();
    backup::load(&d, &dump, &LoadOptions::new()).await.unwrap();
    assert_eq!(stored(&fresh).await.as_deref(), Some("basic"));
    fresh.clock().advance_minutes(24 * 60);
    let app = fresh.boot(d).await;
    let b = app.browser();
    assert_eq!(b.login(&email_of("keeper"), PASSWORD).await.status, 303);
    let body = b.get("/admin/usage").await.body;
    let c = card(&body, "line");
    assert!(c.contains("แพ็กเกจ Basic: 15000 ข้อความต่อเดือน"), "{c}");
    assert!(c.contains("ใช้ไป 1000 จาก 15000"), "{c}");
}
