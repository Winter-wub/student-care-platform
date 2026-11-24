//! A Number setting with a declared range (#172): `/admin/settings`
//! refuses a number outside it with a Thai error naming the range, keeps
//! the typed value and stores nothing; the bounds save; the input carries
//! `min`/`max`/`step` and the help line names the range. A Number setting
//! without a range takes any whole number, as before.

use harpseal::{
    CatalogEntry, DeploymentConfig, Module, ModuleRegistrar, NumberRange, Permission, SettingKind,
    Statement,
};
use harpseal_testing::{Browser, Harness, TEST_CONFIG, TestApp, urlencode};

const PASSWORD: &str = "kelp forest at dawn";
const PAGE: &str = "/admin/settings";
const EDIT: &str = "dues.config.edit";
/// 1 to 30, default 5.
const DAYS: &str = "dues.days";
/// 0 to 60 in steps of 15, default 0.
const MINUTE: &str = "dues.minute";
/// No range: any whole number.
const FEE: &str = "dues.fee";

const TEXTS: &[CatalogEntry] = &[
    CatalogEntry::new("dues.label", "th", "จำนวนวัน"),
    CatalogEntry::new("dues.help", "th", "วันก่อนครบกำหนด"),
];

struct Dues;

impl Module for Dues {
    fn name(&self) -> &'static str {
        "dues"
    }

    fn register(&self, r: &mut ModuleRegistrar) {
        let (label, help) = ("dues.label", "dues.help");
        let quarter = NumberRange::new(0, 60).step(15);
        r.permission(Permission::new(EDIT, "Edit the dues settings"))
            .messages(TEXTS)
            .number_setting(DAYS, "5", NumberRange::new(1, 30), EDIT, label, help)
            .number_setting(MINUTE, "0", quarter, EDIT, label, help)
            .setting(FEE, SettingKind::Number, "200", EDIT, label, help);
    }
}

async fn boot() -> (Harness, TestApp) {
    let toml = TEST_CONFIG.replace(
        "Viewer = [\"kernel.student.view\"]",
        "Viewer = [\"kernel.student.view\"]\nClerk = [\"dues.config.edit\"]",
    );
    let h = Harness::new();
    let d = h
        .builder()
        .config(DeploymentConfig::from_toml(&toml).unwrap())
        .module(Dues)
        .build()
        .unwrap();
    let app = h.boot(d).await;
    (h, app)
}

async fn clerk<'a>(h: &Harness, app: &'a TestApp) -> Browser<'a> {
    h.create_staff("clerk@sealbay.example", PASSWORD, &["Clerk"])
        .await;
    let b = app.browser();
    assert_eq!(b.login("clerk@sealbay.example", PASSWORD).await.status, 303);
    b
}

/// The card of `key` on the page.
fn card<'a>(body: &'a str, key: &str) -> &'a str {
    let id = format!("id=\"setting-{}\"", key.replace('.', "-"));
    let at = body.find(&id).unwrap_or_else(|| panic!("no card {key}"));
    &body[at..at + body[at..].find("</section>").unwrap()]
}

fn token(body: &str, key: &str) -> String {
    let c = card(body, key);
    let k = "name=\"version\" value=\"";
    let from = c.find(k).unwrap() + k.len();
    c[from..from + c[from..].find('"').unwrap()].to_string()
}

/// Opens the page, then saves `value` with the token it showed.
async fn save(b: &Browser<'_>, key: &str, value: &str) -> harpseal_testing::TestResponse {
    let page = b.get(PAGE).await;
    assert_eq!(page.status, 200, "{}", page.body);
    let body = format!(
        "key={key}&value={}&version={}",
        urlencode(value),
        urlencode(&token(&page.body, key))
    );
    b.post(PAGE, &body).await
}

async fn stored(h: &Harness, key: &str) -> Option<String> {
    let q = Statement::new("SELECT value FROM setting WHERE key = ? AND locale = ''").bind(key);
    let rows = h.db().query(q).await.unwrap();
    rows.get(0, "value")
        .and_then(|v| v.as_text())
        .map(str::to_string)
}

async fn audits(h: &Harness) -> i64 {
    let q = "SELECT COUNT(*) AS n FROM audit_log WHERE action = 'setting.update'";
    let rows = h.db().query(Statement::new(q)).await.unwrap();
    rows.get(0, "n").and_then(|v| v.as_i64()).unwrap_or(-1)
}

#[tokio::test]
async fn the_input_shows_the_range() {
    let (h, app) = boot().await;
    let page = clerk(&h, &app).await.get(PAGE).await.body;
    let days = card(&page, DAYS);
    assert!(
        days.contains("min=\"1\"") && days.contains("max=\"30\"") && days.contains("step=\"1\""),
        "{days}"
    );
    assert!(days.contains("จำนวนเต็ม ตั้งแต่ 1 ถึง 30"), "{days}");
    assert!(days.contains("aria-describedby=\"setting-dues-days-kind\""));
    let minute = card(&page, MINUTE);
    assert!(minute.contains("step=\"15\"") && minute.contains("max=\"60\""));
    assert!(minute.contains("ตั้งแต่ 0 ถึง 60 เพิ่มทีละ 15"), "{minute}");
    // No range: the plain whole-number input of before.
    let fee = card(&page, FEE);
    assert!(!fee.contains("min=") && !fee.contains("max="), "{fee}");
    assert!(fee.contains("step=\"1\"") && fee.contains("จำนวนเต็ม"));
    assert!(!fee.contains("ตั้งแต่"));
}

#[tokio::test]
async fn a_number_outside_the_range_is_refused_and_kept_in_the_form() {
    let (h, app) = boot().await;
    let b = clerk(&h, &app).await;
    for (key, bad, why) in [
        (DAYS, "45", "ต้องเป็นจำนวนเต็มตั้งแต่ 1 ถึง 30"),
        (DAYS, "0", "ต้องเป็นจำนวนเต็มตั้งแต่ 1 ถึง 30"),
        (DAYS, "31", "ต้องเป็นจำนวนเต็มตั้งแต่ 1 ถึง 30"),
        (DAYS, "-1", "ต้องเป็นจำนวนเต็มตั้งแต่ 1 ถึง 30"),
        (MINUTE, "75", "ต้องเป็นจำนวนเต็มตั้งแต่ 0 ถึง 60"),
        (MINUTE, "20", "เพิ่มทีละ 15 จาก 0"),
        // Not a number at all: the kind's own error, as before.
        (DAYS, "1.5", "ไม่มีจุลภาคหรือทศนิยม"),
    ] {
        let r = save(&b, key, bad).await;
        assert_eq!(r.status, 422, "{key} {bad}");
        let c = card(&r.body, key);
        assert!(c.contains(why), "{key} {bad}: {c}");
        assert!(c.contains(&format!("value=\"{bad}\"")), "kept: {key} {bad}");
        assert!(c.contains("aria-invalid=\"true\""), "{key} {bad}");
        assert_eq!(stored(&h, key).await, None, "nothing stored: {key} {bad}");
    }
    assert_eq!(audits(&h).await, 0);
    let settings = app.deployment().settings();
    assert_eq!(settings.number(DAYS).await.unwrap(), 5, "still the default");
    // Saving from code goes through the same check.
    let staff = h
        .create_staff("other@sealbay.example", PASSWORD, &["Clerk"])
        .await;
    let ctx = harpseal::RequestCtx::default();
    let actor = harpseal::Actor::Staff(staff);
    let r = settings.save(DAYS, "45", &actor, &ctx).await;
    assert!(matches!(r, Err(harpseal::Error::Invalid(_))), "{r:?}");
    assert_eq!(stored(&h, DAYS).await, None);
}

#[tokio::test]
async fn the_bounds_and_the_steps_save() {
    let (h, app) = boot().await;
    let b = clerk(&h, &app).await;
    let settings = app.deployment().settings();
    for (key, ok) in [
        (DAYS, 1),
        (DAYS, 30),
        (DAYS, 12),
        (MINUTE, 60),
        (MINUTE, 45),
    ] {
        let r = save(&b, key, &format!(" {ok} ")).await;
        assert_eq!(r.status, 303, "{key} {ok}: {}", r.body);
        assert_eq!(settings.number(key).await.unwrap(), ok, "{key}");
        assert_eq!(stored(&h, key).await, Some(ok.to_string()));
    }
    assert_eq!(audits(&h).await, 5);
}

#[tokio::test]
async fn a_number_without_a_range_takes_any_whole_number() {
    let (h, app) = boot().await;
    let b = clerk(&h, &app).await;
    let settings = app.deployment().settings();
    for n in [-1500, 0, 45, 99_999] {
        assert_eq!(save(&b, FEE, &n.to_string()).await.status, 303, "{n}");
        assert_eq!(settings.number(FEE).await.unwrap(), n);
    }
}

#[tokio::test]
async fn a_saved_value_outside_the_range_reads_as_not_set_and_is_flagged() {
    let (h, app) = boot().await;
    // A restored row, or one saved before the range was declared.
    h.db()
        .execute(Statement::new(
            "INSERT INTO setting (key, locale, value, updated_at) \
             VALUES ('dues.days', '', '45', 1)",
        ))
        .await
        .unwrap();
    let read = app.deployment().settings().number(DAYS).await;
    assert!(
        matches!(&read, Err(harpseal::Error::SettingNotSet(k)) if k == DAYS),
        "{read:?}"
    );
    let page = clerk(&h, &app).await.get(PAGE).await.body;
    assert!(card(&page, DAYS).contains("ไม่ผ่านการตรวจแล้ว"));
}
