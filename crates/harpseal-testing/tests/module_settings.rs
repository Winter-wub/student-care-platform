//! Module settings at the Deployment seam (#75): a probe Module declares
//! one setting of each kind; `/admin/settings` edits each only with its
//! own Permission, audits every change, refuses a stale form, and a Job
//! and a backup see the latest value.

use harpseal::backup::{self, LoadOptions, MemoryBackup};
use harpseal::{
    CatalogEntry, DeploymentConfig, Job, JobContext, JobSpec, Module, ModuleRegistrar, Permission,
    SettingKind, SettingValue, Statement, Timestamp,
};
use harpseal_testing::{Browser, Harness, TEST_CONFIG, TestApp, urlencode};

const PASSWORD: &str = "kelp forest at dawn";
const PAGE: &str = "/admin/settings";
const ACCOUNT: &str = "dues.account";
const DEFAULT_ACCOUNT: &str = "ธนาคารทะเลใส\nเลขที่ 000-0-00000-0";
const EDIT: &str = "dues.account.edit";

const TEXTS: &[CatalogEntry] = &[
    CatalogEntry::new("dues.account", "th", "บัญชีรับชำระ"),
    CatalogEntry::new("dues.account_help", "th", "แสดงในหน้าค่าธรรมเนียม"),
    CatalogEntry::new("dues.other", "th", "ค่าอื่น"),
];

/// Declares `dues.account` (its own Permission) and one setting of every
/// other kind (`dues.config.edit`), and a Job that reports the account.
struct Dues;

impl Module for Dues {
    fn name(&self) -> &'static str {
        "dues"
    }

    fn register(&self, r: &mut ModuleRegistrar) {
        let other = |r: &mut ModuleRegistrar, key, kind, default| {
            r.setting(
                key,
                kind,
                default,
                "dues.config.edit",
                "dues.other",
                "dues.other",
            );
        };
        r.permission(Permission::new(EDIT, "Edit the bank account"))
            .permission(Permission::new(
                "dues.config.edit",
                "Edit the other settings",
            ))
            .messages(TEXTS)
            .setting(
                ACCOUNT,
                SettingKind::MultilineText,
                DEFAULT_ACCOUNT,
                EDIT,
                "dues.account",
                "dues.account_help",
            )
            .job(JobSpec::new("dues.report"), report);
        other(r, "dues.title", SettingKind::Text, "ค่าธรรมเนียม");
        other(r, "dues.late_fee", SettingKind::Number, "200");
        other(r, "dues.due", SettingKind::Date, "2027-01-31");
        other(
            r,
            "dues.pay_url",
            SettingKind::Url,
            "https://pay.seal-bay.invalid",
        );
    }
}

async fn report(cx: JobContext, _job: Job) -> harpseal::Result<()> {
    let account = cx.settings().text(ACCOUNT).await?;
    cx.progress(&account).await
}

fn config() -> DeploymentConfig {
    let toml = TEST_CONFIG.replace(
        "Viewer = [\"kernel.student.view\"]",
        "Viewer = [\"kernel.student.view\"]\nTreasurer = [\"dues.account.edit\"]\n\
         Editor = [\"kernel.setting.edit\"]\nClerk = [\"dues.config.edit\"]\n\
         Bursar = [\"dues.account.edit\", \"kernel.setting.edit\"]",
    );
    DeploymentConfig::from_toml(&toml).unwrap()
}

async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let d = h.builder().config(config()).module(Dues).build().unwrap();
    let app = h.boot(d).await;
    (h, app)
}

async fn signed_in<'a>(h: &Harness, app: &'a TestApp, role: &str) -> Browser<'a> {
    let email = format!("{}@sealbay.example", role.to_lowercase());
    h.create_staff(&email, PASSWORD, &[role]).await;
    let b = app.browser();
    assert_eq!(b.login(&email, PASSWORD).await.status, 303);
    b
}

/// The compare-and-set token of `key`'s form on the page.
fn token(body: &str, key: &str) -> String {
    let at = body
        .find(&format!("name=\"key\" value=\"{key}\""))
        .unwrap_or_else(|| panic!("no form for {key}"));
    let k = "name=\"version\" value=\"";
    let from = at + body[at..].find(k).unwrap() + k.len();
    body[from..from + body[from..].find('"').unwrap()].to_string()
}

fn form(key: &str, value: &str, version: &str) -> String {
    format!(
        "key={key}&value={}&version={}",
        urlencode(value),
        urlencode(version)
    )
}

/// Opens the page, then saves `value` with the token it showed.
async fn save(b: &Browser<'_>, key: &str, value: &str) -> harpseal_testing::TestResponse {
    let page = b.get(PAGE).await;
    assert_eq!(page.status, 200, "{}", page.body);
    b.post(PAGE, &form(key, value, &token(&page.body, key)))
        .await
}

async fn count(h: &Harness, sql: &str) -> i64 {
    let rows = h.db().query(Statement::new(sql.to_string())).await.unwrap();
    rows.get(0, "n").and_then(|v| v.as_i64()).unwrap_or(-1)
}

const AUDITS: &str = "SELECT COUNT(*) AS n FROM audit_log WHERE action = 'setting.update'";

#[tokio::test]
async fn the_settings_permission_edits_and_each_change_is_audited() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Treasurer").await;
    // Listed once in the navigation and once as a tile, even for Staff
    // holding both Permissions that show it.
    for staff in [&b, &signed_in(&h, &app, "Bursar").await] {
        let home = staff.get("/admin").await.body;
        assert_eq!(
            home.matches("href=\"/admin/settings\"").count(),
            2,
            "{home}"
        );
    }
    let page = b.get(PAGE).await;
    assert!(page.body.contains("บัญชีรับชำระ") && page.body.contains("ธนาคารทะเลใส"));
    assert!(
        !page.body.contains("dues.late_fee"),
        "only what Treasurer may see"
    );
    let new = "ธนาคารอ่าวคราม สาขาแมวน้ำ\r\nเลขที่ 111-2-33333-4";
    let r = save(&b, ACCOUNT, new).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(
        b.cookie("hs_flash").unwrap_or_default(),
        "ok:kernel.admin.saved"
    );
    let stored = "ธนาคารอ่าวคราม สาขาแมวน้ำ\nเลขที่ 111-2-33333-4";
    let read = app.deployment().settings().get(ACCOUNT).await.unwrap();
    assert_eq!(read, SettingValue::Text(stored.into()));

    let rows = h
        .db()
        .query(Statement::new(
            "SELECT target_kind, target_id, before, after, actor_kind FROM audit_log \
             WHERE action = 'setting.update'",
        ))
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    let col = |c: &str| {
        rows.get(0, c)
            .and_then(|v| v.as_text())
            .unwrap()
            .to_string()
    };
    assert_eq!(
        (col("target_kind"), col("target_id")),
        ("setting".into(), "dues.account:".into())
    );
    assert_eq!(col("actor_kind"), "staff");
    let json = |c: &str| serde_json::from_str::<serde_json::Value>(&col(c)).unwrap();
    assert_eq!(json("before")["value"], DEFAULT_ACCOUNT);
    assert_eq!(json("after")["value"], stored);

    // A second change: before is the first change.
    assert_eq!(save(&b, ACCOUNT, "บัญชีใหม่").await.status, 303);
    assert_eq!(count(&h, AUDITS).await, 2);
    // Saving the same value writes nothing.
    assert_eq!(save(&b, ACCOUNT, "บัญชีใหม่").await.status, 303);
    assert_eq!(count(&h, AUDITS).await, 2);
}

#[tokio::test]
async fn kernel_setting_edit_alone_sees_no_button_and_gets_403() {
    let (h, app) = boot().await;
    let editor = signed_in(&h, &app, "Editor").await;
    let page = editor.get(PAGE).await;
    assert_eq!(page.status, 200);
    assert!(page.body.contains("ธนาคารทะเลใส"), "shown read-only");
    assert!(
        !page.body.contains("name=\"key\""),
        "no form, no Save button"
    );
    let r = editor.post(PAGE, &form(ACCOUNT, "บัญชีโจร", "-")).await;
    assert_eq!(r.status, 403, "{}", r.body);
    // Clerk holds another setting's Permission, not this one's.
    let clerk = signed_in(&h, &app, "Clerk").await;
    let page = clerk.get(PAGE).await.body;
    assert!(page.contains("name=\"key\" value=\"dues.late_fee\""));
    assert!(!page.contains("value=\"dues.account\""));
    assert_eq!(
        clerk.post(PAGE, &form(ACCOUNT, "บัญชีโจร", "-")).await.status,
        403
    );
    // Neither Permission: the whole page is a 403, and no link to it.
    let viewer = signed_in(&h, &app, "Viewer").await;
    assert_eq!(viewer.get(PAGE).await.status, 403);
    assert!(!viewer.get("/admin").await.body.contains("/admin/settings"));
    assert_eq!(
        viewer.post(PAGE, &form(ACCOUNT, "x", "-")).await.status,
        403
    );
    assert_eq!(count(&h, AUDITS).await, 0);
    let read = app.deployment().settings().text(ACCOUNT).await.unwrap();
    assert_eq!(read, DEFAULT_ACCOUNT);
    // The Texts page cannot write it either: it only writes a Locale's row.
    let texts = "/admin/texts/dues.account";
    let r = editor
        .post(texts, "locale=&value=x&version=-&op=save")
        .await;
    assert_eq!(r.status, 404);
    assert_eq!(
        app.deployment().settings().text(ACCOUNT).await.unwrap(),
        DEFAULT_ACCOUNT
    );
}

#[tokio::test]
async fn every_kind_is_checked_on_the_server() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Clerk").await;
    for (key, bad, why) in [
        ("dues.title", "บรรทัด\nสอง", "อักขระที่ใช้ไม่ได้"),
        ("dues.title", "   ", "กรุณากรอกค่า"),
        ("dues.late_fee", "1,500", "จำนวนเต็ม"),
        ("dues.due", "2027-02-30", "ปปปป-ดด-วว"),
        ("dues.pay_url", "http://pay.seal-bay.invalid", "https://"),
        ("dues.pay_url", "javascript:alert(1)", "https://"),
    ] {
        let r = save(&b, key, bad).await;
        assert_eq!(r.status, 422, "{key} {bad}");
        assert!(r.body.contains(why), "{key} {bad}");
        assert!(r.body.contains("aria-invalid=\"true\""));
    }
    assert_eq!(count(&h, AUDITS).await, 0);
    assert_eq!(save(&b, "dues.late_fee", " 350 ").await.status, 303);
    assert_eq!(save(&b, "dues.due", "2028-02-29").await.status, 303);
    let settings = app.deployment().settings();
    assert_eq!(settings.number("dues.late_fee").await.unwrap(), 350);
    assert_eq!(
        settings.get("dues.due").await.unwrap(),
        SettingValue::Date("2028-02-29".into())
    );
    assert!(settings.number("dues.due").await.is_err());
    assert!(settings.get("dues.nope").await.is_err());
    // An unknown key is a 404.
    let r = b.post(PAGE, &form("dues.nope", "x", "-")).await;
    assert_eq!(r.status, 404);
}

#[tokio::test]
async fn a_stale_form_is_refused_by_the_token() {
    let (h, app) = boot().await;
    let first = signed_in(&h, &app, "Treasurer").await;
    let second = signed_in(&h, &app, "Bursar").await;
    let t1 = token(&first.get(PAGE).await.body, ACCOUNT);
    let t2 = token(&second.get(PAGE).await.body, ACCOUNT);
    assert_eq!(t1, t2);
    let r = first.post(PAGE, &form(ACCOUNT, "บัญชีของคนแรก", &t1)).await;
    assert_eq!(r.status, 303);
    let r = second.post(PAGE, &form(ACCOUNT, "บัญชีของคนที่สอง", &t2)).await;
    assert_eq!(r.status, 303);
    assert_eq!(
        second.cookie("hs_flash").unwrap_or_default(),
        "err:kernel.admin.conflict"
    );
    let read = app.deployment().settings().text(ACCOUNT).await.unwrap();
    assert_eq!(read, "บัญชีของคนแรก");
    assert_eq!(count(&h, AUDITS).await, 1);
}

#[tokio::test]
async fn a_job_reads_the_latest_value() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Treasurer").await;
    let run = || async {
        let d = app.deployment();
        d.enqueue("dues.report", serde_json::json!({}), None)
            .await
            .unwrap();
        let r = d.tick(Timestamp(h.clock().now_ms())).await.unwrap();
        assert_eq!(r.failed, 0, "{:?}", r.errors);
        let rows = h
            .db()
            .query(Statement::new(
                "SELECT progress FROM job ORDER BY created_at DESC, id DESC",
            ))
            .await
            .unwrap();
        rows.get(0, "progress")
            .and_then(|v| v.as_text())
            .unwrap()
            .to_string()
    };
    assert_eq!(run().await, DEFAULT_ACCOUNT);
    assert_eq!(save(&b, ACCOUNT, "บัญชีที่แก้แล้ว").await.status, 303);
    h.clock().advance_ms(1);
    assert_eq!(run().await, "บัญชีที่แก้แล้ว");
}

#[tokio::test]
async fn a_backup_restores_the_settings() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Treasurer").await;
    assert_eq!(save(&b, ACCOUNT, "บัญชีที่สำรองไว้").await.status, 303);
    let mut sink = MemoryBackup::new();
    backup::dump(app.deployment(), &mut sink).await.unwrap();
    assert!(sink.entries.contains_key("tables/setting.ndjson"));

    let fresh = Harness::new();
    let d = fresh
        .builder()
        .config(config())
        .module(Dues)
        .build()
        .unwrap();
    backup::load(&d, &sink, &LoadOptions::new()).await.unwrap();
    assert_eq!(d.settings().text(ACCOUNT).await.unwrap(), "บัญชีที่สำรองไว้");
    assert_eq!(d.settings().text("dues.title").await.unwrap(), "ค่าธรรมเนียม");
}
