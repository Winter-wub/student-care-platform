//! A switch among the Module settings (#163): `SettingKind::Boolean` is a
//! checkbox on `/admin/settings`, saved and audited like the other kinds
//! and read as a `bool`. Stored as `1`/`0`, so a 0/1 Number switch turned
//! into a Boolean reads on without a migration.

use harpseal::{
    BuildError, CatalogEntry, DeploymentConfig, Module, ModuleRegistrar, Permission, SettingKind,
    SettingValue, Statement,
};
use harpseal_testing::{Browser, Harness, TEST_CONFIG, TestApp, urlencode};

const PASSWORD: &str = "kelp forest at dawn";
const PAGE: &str = "/admin/settings";
const LATE: &str = "dues.late_fee_on";
const OLD: &str = "dues.old_switch";
const EDIT: &str = "dues.config.edit";

const TEXTS: &[CatalogEntry] = &[
    CatalogEntry::new("dues.late_label", "th", "คิดค่าปรับชำระช้า"),
    CatalogEntry::new("dues.late_help", "th", "เปิดเมื่อเริ่มเก็บค่าปรับ"),
];

/// `dues.late_fee_on` (off by default) and `dues.old_switch`, once a
/// 0/1 Number, both Booleans now; `default` is the first one's default.
struct Dues(&'static str);

impl Module for Dues {
    fn name(&self) -> &'static str {
        "dues"
    }

    fn register(&self, r: &mut ModuleRegistrar) {
        let (label, help) = ("dues.late_label", "dues.late_help");
        r.permission(Permission::new(EDIT, "Edit the dues settings"))
            .messages(TEXTS)
            .setting(LATE, SettingKind::Boolean, self.0, EDIT, label, help)
            .setting(OLD, SettingKind::Boolean, "0", EDIT, label, help);
    }
}

fn config() -> DeploymentConfig {
    let toml = TEST_CONFIG.replace(
        "Viewer = [\"kernel.student.view\"]",
        "Viewer = [\"kernel.student.view\"]\nClerk = [\"dues.config.edit\"]\n\
         Editor = [\"kernel.setting.edit\"]",
    );
    DeploymentConfig::from_toml(&toml).unwrap()
}

async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let d = h
        .builder()
        .config(config())
        .module(Dues("0"))
        .build()
        .unwrap();
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

/// Posts the form of `key` as the browser does: `value=1` when ticked,
/// no `value` at all when not.
async fn save(b: &Browser<'_>, key: &str, on: bool) -> harpseal_testing::TestResponse {
    let page = b.get(PAGE).await;
    assert_eq!(page.status, 200, "{}", page.body);
    let version = urlencode(&token(&page.body, key));
    let value = if on { "&value=1" } else { "" };
    b.post(PAGE, &format!("key={key}&version={version}{value}"))
        .await
}

/// (before, after) of every `setting.update` audit row, in order.
async fn audits(h: &Harness) -> Vec<(String, String)> {
    let rows = h
        .db()
        .query(Statement::new(
            "SELECT before, after FROM audit_log WHERE action = 'setting.update' \
             ORDER BY rowid",
        ))
        .await
        .unwrap();
    let value = |i: usize, c: &str| {
        let json = rows.get(i, c).and_then(|v| v.as_text()).unwrap();
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        v["value"].as_str().unwrap_or("null").to_string()
    };
    (0..rows.len())
        .map(|i| (value(i, "before"), value(i, "after")))
        .collect()
}

#[tokio::test]
async fn a_switch_renders_saves_audits_and_reads_as_bool() {
    let (h, app) = boot().await;
    let settings = app.deployment().settings();
    assert!(!settings.bool(LATE).await.unwrap(), "the default: off");
    let b = signed_in(&h, &app, "Clerk").await;
    let page = b.get(PAGE).await.body;
    let c = card(&page, LATE);
    assert!(
        c.contains("type=\"checkbox\"") && c.contains("role=\"switch\""),
        "{c}"
    );
    assert!(!c.contains("checked"), "off: not ticked");
    assert!(c.contains("เปิดหรือปิด"), "the kind, in plain Thai");

    let r = save(&b, LATE, true).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(
        b.cookie("hs_flash").unwrap_or_default(),
        "ok:kernel.admin.saved"
    );
    assert!(settings.bool(LATE).await.unwrap());
    assert_eq!(settings.get(LATE).await.unwrap(), SettingValue::Bool(true));
    assert_eq!(settings.text(LATE).await.unwrap(), "1");
    assert!(card(&b.get(PAGE).await.body, LATE).contains("checked"));

    // Unticked: the browser sends no value, which is off.
    assert_eq!(save(&b, LATE, false).await.status, 303);
    assert!(!settings.bool(LATE).await.unwrap());
    // Off again writes nothing.
    assert_eq!(save(&b, LATE, false).await.status, 303);
    let want = [("0", "1"), ("1", "0")].map(|(a, b)| (a.to_string(), b.to_string()));
    assert_eq!(audits(&h).await, want);

    // Anything but on or off, posted by hand, is refused.
    let page = b.get(PAGE).await.body;
    let form = format!(
        "key={LATE}&version={}&value=yes",
        urlencode(&token(&page, LATE))
    );
    let r = b.post(PAGE, &form).await;
    assert_eq!(r.status, 422, "{}", r.body);
    assert!(r.body.contains("ต้องเป็นเปิดหรือปิด"));
    assert_eq!(audits(&h).await.len(), 2);
    // Another kind's reader refuses it, and `bool` refuses another kind.
    assert!(settings.number(LATE).await.is_err());
}

#[tokio::test]
async fn read_only_staff_see_on_or_off_and_cannot_save() {
    let (h, app) = boot().await;
    let editor = signed_in(&h, &app, "Editor").await;
    let page = editor.get(PAGE).await.body;
    let c = card(&page, LATE);
    // "ปิดอยู่" (off), not "เปิดอยู่" (on), and no checkbox.
    assert!(c.contains("ปิดอยู่") && !c.contains("เปิดอยู่"), "{c}");
    assert!(!c.contains("type=\"checkbox\""), "{c}");
    let r = editor
        .post(PAGE, &format!("key={LATE}&version=-&value=1"))
        .await;
    assert_eq!(r.status, 403);
    assert!(!app.deployment().settings().bool(LATE).await.unwrap());
}

#[tokio::test]
async fn a_stored_value_other_than_one_or_zero_reads_as_not_set() {
    for (i, stored) in ["true", "2"].into_iter().enumerate() {
        let (h, app) = boot().await;
        h.db()
            .execute(Statement::new(format!(
                "INSERT INTO setting (key, locale, value, updated_at) \
                 VALUES ('{OLD}', '', '{stored}', {i})"
            )))
            .await
            .unwrap();
        let read = app.deployment().settings().bool(OLD).await;
        assert!(
            matches!(&read, Err(harpseal::Error::SettingNotSet(k)) if k == OLD),
            "{stored}: {read:?}"
        );
        let page = signed_in(&h, &app, "Clerk").await.get(PAGE).await.body;
        assert!(
            card(&page, OLD).contains("ไม่ผ่านการตรวจแล้ว"),
            "{stored}: flagged"
        );
    }
}

#[tokio::test]
async fn the_switch_is_named_by_its_setting() {
    let (h, app) = boot().await;
    let page = signed_in(&h, &app, "Clerk").await.get(PAGE).await.body;
    let c = card(&page, LATE);
    let card_id = "setting-dues-late_fee_on";
    let names = format!("aria-labelledby=\"{card_id}-name {card_id}-switch\"");
    assert!(c.contains(&names), "{c}");
    assert!(
        c.contains(&format!("<span id=\"{card_id}-name\">คิดค่าปรับชำระช้า</span>")),
        "{c}"
    );
}

#[tokio::test]
async fn a_number_switch_stored_as_one_reads_on_without_a_migration() {
    let (h, app) = boot().await;
    // Saved while `dues.old_switch` was a 0/1 Number.
    h.db()
        .execute(Statement::new(format!(
            "INSERT INTO setting (key, locale, value, updated_at) VALUES ('{OLD}', '', '1', 1)"
        )))
        .await
        .unwrap();
    assert!(app.deployment().settings().bool(OLD).await.unwrap());
    let page = signed_in(&h, &app, "Clerk").await.get(PAGE).await.body;
    let c = card(&page, OLD);
    assert!(
        c.contains("checked") && !c.contains("ไม่ผ่านการตรวจแล้ว"),
        "{c}"
    );
}

#[test]
fn a_switch_default_must_be_one_or_zero() {
    for bad in ["true", "on", ""] {
        let h = Harness::new();
        let r = h.builder().config(config()).module(Dues(bad)).build();
        assert!(
            matches!(&r, Err(BuildError::InvalidSetting { key, .. }) if key == LATE),
            "{bad}: {:?}",
            r.err()
        );
    }
}
