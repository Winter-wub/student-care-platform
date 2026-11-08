//! A Module setting with no default (#103): `ModuleRegistrar::
//! setting_without_default` reads as `SettingNotSet` until the first save,
//! `/admin/settings` shows "not set yet" (not the failed-check flag), a
//! backup keeps it unset or set. `Settings::save` writes only as Staff
//! holding the Setting's Permission (Module pages and Jobs hold a
//! `Settings`, so they can never write as the system); a seed or the CLI
//! uses `Deployment::save_setting_as_system`: same check, same audit row.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use harpseal::backup::{self, LoadOptions, MemoryBackup};
use harpseal::{
    Actor, AppState, CatalogEntry, DeploymentConfig, Error, Job, JobContext, JobSpec, Module,
    ModuleRegistrar, Permission, RequestCtx, SettingKind, Statement, Timestamp,
};
use harpseal_testing::{Browser, Harness, TEST_CONFIG, TestApp, urlencode};

const PASSWORD: &str = "kelp forest at dawn";
const PAGE: &str = "/admin/settings";
const ACCOUNT: &str = "gift.account";
const EDIT: &str = "gift.account.edit";
const DEFAULTED: &str = "gift.title";

const TEXTS: &[CatalogEntry] = &[
    CatalogEntry::new("gift.label", "th", "บัญชีรับบริจาค"),
    CatalogEntry::new("gift.help", "th", "แสดงในหน้าบริจาค"),
];

/// `gift.account` has no default; `gift.title` has one.
struct Gift;

impl Module for Gift {
    fn name(&self) -> &'static str {
        "gift"
    }

    fn register(&self, r: &mut ModuleRegistrar) {
        r.permission(Permission::new(EDIT, "Edit the gift account"))
            .messages(TEXTS)
            .setting_without_default(
                ACCOUNT,
                SettingKind::AccountNumber,
                EDIT,
                "gift.label",
                "gift.help",
            )
            .setting(
                DEFAULTED,
                SettingKind::Text,
                "บริจาค",
                EDIT,
                "gift.label",
                "gift.help",
            )
            .public("/gift/try", get(try_from_a_page))
            .job(JobSpec::new("gift.try"), try_from_a_job);
    }
}

/// What a Module page can do with its `Settings`: save as the system, and
/// as Staff it does not hold (no session at all here).
async fn try_from_a_page(State(s): State<AppState>) -> StatusCode {
    let ctx = RequestCtx::default();
    let attempts = [
        s.settings()
            .save(ACCOUNT, "1-1-1", &Actor::System, &ctx)
            .await,
        s.settings()
            .save(ACCOUNT, "2-2-2", &Actor::Student("s1".into()), &ctx)
            .await,
    ];
    if attempts
        .iter()
        .all(|r| matches!(r, Err(Error::Forbidden { .. })))
    {
        StatusCode::FORBIDDEN
    } else {
        StatusCode::OK
    }
}

/// The same from a Job, as `Actor::Job`; the outcome goes to `progress`.
async fn try_from_a_job(cx: JobContext, job: Job) -> harpseal::Result<()> {
    let r = cx
        .settings()
        .save(
            ACCOUNT,
            "3-3-3",
            &Actor::Job(job.id.clone()),
            &RequestCtx::default(),
        )
        .await;
    let forbidden = matches!(r, Err(Error::Forbidden { .. }));
    cx.progress(if forbidden { "forbidden" } else { "wrote" })
        .await
}

fn config() -> DeploymentConfig {
    let toml = TEST_CONFIG.replace(
        "Viewer = [\"kernel.student.view\"]",
        "Viewer = [\"kernel.student.view\"]\nTreasurer = [\"gift.account.edit\"]\n\
         Editor = [\"kernel.setting.edit\"]",
    );
    DeploymentConfig::from_toml(&toml).unwrap()
}

fn deployment(h: &Harness) -> harpseal::Deployment {
    h.builder().config(config()).module(Gift).build().unwrap()
}

async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let app = h.boot(deployment(&h)).await;
    (h, app)
}

async fn signed_in<'a>(h: &Harness, app: &'a TestApp, role: &str) -> Browser<'a> {
    let email = format!("{}@sealbay.example", role.to_lowercase());
    h.create_staff(&email, PASSWORD, &[role]).await;
    let b = app.browser();
    assert_eq!(b.login(&email, PASSWORD).await.status, 303);
    b
}

fn token(body: &str, key: &str) -> String {
    let at = body
        .find(&format!("name=\"key\" value=\"{key}\""))
        .unwrap_or_else(|| panic!("no form for {key}"));
    let k = "name=\"version\" value=\"";
    let from = at + body[at..].find(k).unwrap() + k.len();
    body[from..from + body[from..].find('"').unwrap()].to_string()
}

async fn save_on_page(b: &Browser<'_>, key: &str, value: &str) -> harpseal_testing::TestResponse {
    let page = b.get(PAGE).await;
    let version = token(&page.body, key);
    let form = format!(
        "key={key}&value={}&version={}",
        urlencode(value),
        urlencode(&version)
    );
    b.post(PAGE, &form).await
}

async fn count(h: &Harness, sql: &str) -> i64 {
    let rows = h.db().query(Statement::new(sql.to_string())).await.unwrap();
    rows.get(0, "n").and_then(|v| v.as_i64()).unwrap_or(-1)
}

const AUDITS: &str = "SELECT COUNT(*) AS n FROM audit_log WHERE action = 'setting.update'";

fn not_set(r: &harpseal::Result<String>) -> bool {
    matches!(r, Err(Error::SettingNotSet(k)) if k == ACCOUNT)
}

#[tokio::test]
async fn unset_reads_not_set_until_saved_then_the_saved_value() {
    let (h, app) = boot().await;
    let s = app.deployment().settings();
    assert!(
        not_set(&s.text(ACCOUNT).await),
        "no default to fall back on"
    );
    assert_eq!(
        s.text(DEFAULTED).await.unwrap(),
        "บริจาค",
        "defaults as today"
    );

    let b = signed_in(&h, &app, "Treasurer").await;
    let page = b.get(PAGE).await.body;
    assert!(page.contains("id=\"setting-gift-account-unset\""), "{page}");
    assert!(!page.contains("flash err"), "not the failed-check flag");
    assert_eq!(save_on_page(&b, ACCOUNT, "123-4-56789-0").await.status, 303);
    assert_eq!(s.text(ACCOUNT).await.unwrap(), "123-4-56789-0");
    let page = b.get(PAGE).await.body;
    assert!(!page.contains("id=\"setting-gift-account-unset\""));

    // The first save's audit row has no value before.
    let first = "SELECT COUNT(*) AS n FROM audit_log WHERE action = 'setting.update' \
                 AND target_id = 'gift.account:' AND json_extract(before, '$.value') IS NULL \
                 AND json_extract(after, '$.value') = '123-4-56789-0'";
    assert_eq!(count(&h, first).await, 1);
}

#[tokio::test]
async fn read_only_staff_see_not_set_yet_with_no_form() {
    let (h, app) = boot().await;
    let page = signed_in(&h, &app, "Editor").await.get(PAGE).await;
    assert_eq!(page.status, 200);
    assert!(page.body.contains("ยังไม่ได้ตั้งค่า"), "{}", page.body);
    assert!(!page.body.contains("name=\"key\""), "no Save button");
    let broken = signed_in(&h, &app, "Treasurer").await;
    // A saved value failing its check is a different message.
    h.db()
        .execute(Statement::new(
            "INSERT INTO setting (key, locale, value, updated_at) \
             VALUES ('gift.account', '', 'abc', 1)",
        ))
        .await
        .unwrap();
    let body = broken.get(PAGE).await.body;
    assert!(body.contains("ไม่ผ่านการตรวจแล้ว"), "{body}");
    assert!(
        !body.contains("id=\"setting-gift-account-unset\""),
        "{body}"
    );
}

#[tokio::test]
async fn a_backup_keeps_unset_and_set() {
    let (h, app) = boot().await;
    let mut unset = MemoryBackup::new();
    backup::dump(app.deployment(), &mut unset).await.unwrap();
    let fresh = Harness::new();
    let d = deployment(&fresh);
    backup::load(&d, &unset, &LoadOptions::new()).await.unwrap();
    assert!(not_set(&d.settings().text(ACCOUNT).await), "stays unset");

    let b = signed_in(&h, &app, "Treasurer").await;
    assert_eq!(save_on_page(&b, ACCOUNT, "123-4-56789-0").await.status, 303);
    let mut set = MemoryBackup::new();
    backup::dump(app.deployment(), &mut set).await.unwrap();
    let again = Harness::new();
    let d = deployment(&again);
    backup::load(&d, &set, &LoadOptions::new()).await.unwrap();
    assert_eq!(d.settings().text(ACCOUNT).await.unwrap(), "123-4-56789-0");
}

#[tokio::test]
async fn the_system_entry_point_checks_the_value_and_audits_and_save_checks_the_actor() {
    let (h, app) = boot().await;
    let d = app.deployment();
    let s = d.settings();
    let ctx = RequestCtx::cli().seeded();

    // A bad value is refused and writes nothing.
    let bad = d
        .save_setting_as_system(ACCOUNT, "\u{202E}0-54321", &ctx)
        .await;
    assert!(matches!(bad, Err(Error::Invalid(_))), "{bad:?}");
    let nope = d.save_setting_as_system("gift.nope", "1", &ctx).await;
    assert!(matches!(nope, Err(Error::Invalid(_))), "{nope:?}");
    assert_eq!(count(&h, AUDITS).await, 0);
    assert!(not_set(&s.text(ACCOUNT).await));

    // The system saves; the value is stored as the kind returns it.
    d.save_setting_as_system(ACCOUNT, " 123  4 56789-0 ", &ctx)
        .await
        .unwrap();
    assert_eq!(s.text(ACCOUNT).await.unwrap(), "123 4 56789-0");
    let seeded = "SELECT COUNT(*) AS n FROM audit_log WHERE action = 'setting.update' \
                  AND actor_kind = 'system' AND request_id = 'cli' \
                  AND json_extract(after, '$.seed') = 1 AND json_extract(before, '$.value') IS NULL";
    assert_eq!(count(&h, seeded).await, 1);
    // The same value again writes no second row.
    d.save_setting_as_system(ACCOUNT, "123 4 56789-0", &ctx)
        .await
        .unwrap();
    assert_eq!(count(&h, AUDITS).await, 1);
    // A setting with a default audits the default as the value before.
    d.save_setting_as_system(DEFAULTED, "ร่วมบริจาค", &ctx)
        .await
        .unwrap();
    let defaulted = "SELECT COUNT(*) AS n FROM audit_log WHERE target_id = 'gift.title:' \
                     AND json_extract(before, '$.value') = 'บริจาค'";
    assert_eq!(count(&h, defaulted).await, 1);

    // Staff need the setting's own Permission; a Student never saves.
    let viewer = h
        .create_staff("viewer@sealbay.example", PASSWORD, &["Viewer"])
        .await;
    let denied = s.save(ACCOUNT, "1-2", &Actor::Staff(viewer), &ctx).await;
    assert!(matches!(denied, Err(Error::Forbidden { .. })), "{denied:?}");
    let student = s
        .save(ACCOUNT, "1-2", &Actor::Student("s1".into()), &ctx)
        .await;
    assert!(
        matches!(student, Err(Error::Forbidden { .. })),
        "{student:?}"
    );
    let treasurer = h
        .create_staff("treasurer@sealbay.example", PASSWORD, &["Treasurer"])
        .await;
    s.save(ACCOUNT, "9-8-7", &Actor::Staff(treasurer), &ctx)
        .await
        .unwrap();
    assert_eq!(s.text(ACCOUNT).await.unwrap(), "9-8-7");
    assert_eq!(count(&h, AUDITS).await, 3);

    // `Settings::save` never writes as the system or a Job, whoever calls.
    for actor in [Actor::System, Actor::Job("j1".into())] {
        let r = s.save(ACCOUNT, "5-5-5", &actor, &ctx).await;
        assert!(
            matches!(r, Err(Error::Forbidden { .. })),
            "{actor:?}: {r:?}"
        );
    }
    // A deactivated Staff member who held the Permission is refused too.
    let gone = h
        .create_staff("gone@sealbay.example", PASSWORD, &["Treasurer"])
        .await;
    let off =
        Statement::new("UPDATE staff SET status = 'disabled' WHERE id = ?").bind(gone.as_str());
    h.db().execute(off).await.unwrap();
    let r = s.save(ACCOUNT, "5-5-5", &Actor::Staff(gone), &ctx).await;
    assert!(matches!(r, Err(Error::Forbidden { .. })), "{r:?}");
    assert_eq!(s.text(ACCOUNT).await.unwrap(), "9-8-7");
    assert_eq!(count(&h, AUDITS).await, 3, "nothing written");
}

#[tokio::test]
async fn module_pages_and_jobs_cannot_write_a_setting_as_the_system_or_a_job() {
    let (h, app) = boot().await;
    // A page (no session) saving as the system and as a Student: refused.
    let page = app.browser().get("/gift/try").await;
    assert_eq!(page.status, 403, "{}", page.body);
    // A Job saving as `Actor::Job`: refused, and it reports it.
    let d = app.deployment();
    d.enqueue("gift.try", serde_json::json!({}), None)
        .await
        .unwrap();
    let r = d.tick(Timestamp(h.clock().now_ms())).await.unwrap();
    assert_eq!(r.failed, 0, "{:?}", r.errors);
    let rows = h
        .db()
        .query(Statement::new(
            "SELECT progress FROM job WHERE kind = 'gift.try'",
        ))
        .await
        .unwrap();
    assert_eq!(
        rows.get(0, "progress").and_then(|v| v.as_text()),
        Some("forbidden")
    );
    assert!(
        not_set(&d.settings().text(ACCOUNT).await),
        "nothing written"
    );
    assert_eq!(count(&h, AUDITS).await, 0);
}
