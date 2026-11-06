//! The public write functions (`harpseal::roster`, `harpseal::texts`,
//! `harpseal::modules::announcements`) check the acting Staff member's
//! Permission like the pages do: no Permission, no write, no audit row.

use harpseal::modules::announcements::{self, Announcements, NewAnnouncement};
use harpseal::roster::{self, Confirmed, Uploaded};
use harpseal::{Actor, DeploymentConfig, Error, RequestCtx};
use harpseal_testing::{Harness, TestApp};

const CONFIG: &str = r#"
name = "Seal Bay University (public writes)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Officer = ["kernel.roster.view", "kernel.roster.import", "announcements.view",
           "announcements.send", "kernel.setting.edit"]
Viewer = ["kernel.roster.view", "announcements.view"]

[roster]
student_code = "รหัสนักศึกษา"
display_name = "ชื่อ-สกุล"
email = "อีเมล"

[roster.attributes.year]
column = "ชั้นปี"
type = "integer"
"#;

const CSV: &str =
    "รหัสนักศึกษา,ชื่อ-สกุล,อีเมล,ชั้นปี\r\n68010001,นกน้อย ทะเลใส,s68010001@sealbay.example,1\r\n";
const PASSWORD: &str = "kelp forest at dawn";

async fn boot() -> (Harness, TestApp, Actor, Actor) {
    let h = Harness::new();
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    let d = h
        .builder()
        .config(config)
        .module(Announcements)
        .build()
        .unwrap();
    let app = h.boot(d).await;
    let officer = h
        .create_staff("officer@sealbay.example", PASSWORD, &["Officer"])
        .await;
    let viewer = h
        .create_staff("viewer@sealbay.example", PASSWORD, &["Viewer"])
        .await;
    (h, app, Actor::Staff(officer), Actor::Staff(viewer))
}

fn refused<T: std::fmt::Debug>(r: harpseal::Result<T>, permission: &str) {
    match r {
        Err(Error::Invalid(m)) => assert!(m.contains(permission), "{m}"),
        other => panic!("expected a refusal naming {permission}, got {other:?}"),
    }
}

#[tokio::test]
async fn roster_upload_and_confirm_need_kernel_roster_import() {
    let (_h, app, officer, viewer) = boot().await;
    let d = app.deployment();
    let ctx = RequestCtx::cli();
    let bytes = || CSV.as_bytes().to_vec();
    let perm = "kernel.roster.import";
    refused(
        roster::upload(d, "a.csv", bytes(), "upsert", &viewer, &ctx).await,
        perm,
    );
    refused(
        roster::upload(d, "a.csv", bytes(), "upsert", &Actor::System, &ctx).await,
        perm,
    );
    assert_eq!(app.count("SELECT count(*) FROM roster_import").await, 0);

    let up = roster::upload(d, "a.csv", bytes(), "upsert", &officer, &ctx).await;
    let Ok(Uploaded::Previewed(id)) = up else {
        let err = app.query("SELECT error FROM roster_import").await;
        panic!("previewed: {up:?} {err:?}")
    };
    refused(roster::confirm(d, &id, &viewer, &ctx).await, perm);
    assert_eq!(
        roster::confirm(d, &id, &officer, &ctx).await.unwrap(),
        Confirmed::Queued
    );
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action LIKE 'roster.import%'")
            .await,
        2,
        "only the Officer's upload and confirm"
    );
}

#[tokio::test]
async fn announcements_need_announcements_send() {
    let (_h, app, officer, viewer) = boot().await;
    let d = app.deployment();
    let ctx = RequestCtx::cli();
    let new = NewAnnouncement::new("ประกาศทดสอบ", "เนื้อหาทดสอบ");
    let perm = "announcements.send";
    refused(
        announcements::create_draft(d, &new, &viewer, &ctx).await,
        perm,
    );
    assert_eq!(app.count("SELECT count(*) FROM announcement").await, 0);

    let id = announcements::create_draft(d, &new, &officer, &ctx)
        .await
        .unwrap();
    refused(announcements::confirm(d, &id, 0, &viewer, &ctx).await, perm);
    assert!(
        !announcements::confirm(d, &id, 0, &officer, &ctx)
            .await
            .unwrap()
    );

    let mut bad = NewAnnouncement::new("", "เนื้อหา");
    bad.require_ack = true;
    let err = announcements::create_draft(d, &bad, &officer, &ctx)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("announcements.form.th_required"),
        "{err}"
    );
}

#[tokio::test]
async fn text_overrides_need_kernel_setting_edit() {
    let (_h, app, officer, viewer) = boot().await;
    let d = app.deployment();
    let ctx = RequestCtx::cli().seeded();
    let (key, value) = ("kernel.flow.welcome", "สวัสดี (ทดสอบ)");
    refused(
        harpseal::texts::set_override(d, key, "th", value, &viewer, &ctx).await,
        "kernel.setting.edit",
    );
    assert_eq!(
        app.count("SELECT count(*) FROM setting WHERE locale <> ''")
            .await,
        0
    );

    assert!(
        harpseal::texts::set_override(d, key, "th", value, &officer, &ctx)
            .await
            .unwrap()
    );
    assert!(
        !harpseal::texts::set_override(d, key, "th", value, &officer, &ctx)
            .await
            .unwrap(),
        "the same value again writes nothing"
    );
    let not_editable =
        harpseal::texts::set_override(d, "kernel.admin.save", "th", "x", &officer, &ctx);
    assert!(matches!(not_editable.await, Err(Error::Invalid(_))));
    let seeded = app
        .count(
            "SELECT count(*) FROM audit_log WHERE action = 'setting.update' \
             AND json_extract(after, '$.seed') = 1",
        )
        .await;
    assert_eq!(seeded, 1);
}
