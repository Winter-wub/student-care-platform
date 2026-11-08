//! Setup for the LINE package tests (#115): a Deployment with a `line`
//! Channel (with or without `[quotas.line]` 300), and the form a browser
//! posts to choose a package.

#![allow(dead_code)]

use harpseal::{DeploymentConfig, Statement};
use harpseal_testing::{Browser, Harness, TestApp, urlencode};

use super::{int, signed_in};

pub const KEY: &str = "kernel.usage.line_package";
pub const PATH: &str = "/admin/usage/package";

pub const CONFIG_LINE: &str = r#"
name = "Seal Bay University (package)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Administrator = [
    "kernel.student.view", "kernel.usage.view", "kernel.audit.view",
    "kernel.staff.manage", "kernel.setting.edit",
]
Officer = ["kernel.student.view", "kernel.usage.view"]
Editor = ["kernel.setting.edit"]
"#;

pub const TOML_CAP: &str = "\n[quotas.line]\nmetered_per_month = 300\n";

/// A Deployment with a `line` Channel and, with `quota`, `[quotas.line]` 300.
pub async fn boot_line(quota: bool) -> (Harness, TestApp) {
    let h = Harness::new();
    let toml = format!("{CONFIG_LINE}{}", if quota { TOML_CAP } else { "" });
    let d = h
        .builder()
        .config(DeploymentConfig::from_toml(&toml).unwrap())
        .channel(h.fake_channel().with_kind("line"))
        .build()
        .unwrap();
    let app = h.boot(d).await;
    (h, app)
}

pub fn card<'a>(body: &'a str, kind: &str) -> &'a str {
    let at = body.find(&format!("id=\"usage-{kind}\"")).expect("a card");
    let rest = &body[at..];
    &rest[..rest.find("</section>").unwrap()]
}

pub fn flash(b: &Browser<'_>) -> String {
    b.cookie("hs_flash").unwrap_or_default()
}

/// The form a browser posts: `expected` is the page's token.
pub async fn choose(b: &Browser<'_>, package: &str, custom: &str) -> String {
    let page = b.get("/admin/usage").await.body;
    let key = "name=\"expected\" value=\"";
    let at = page.find(key).expect("the package form") + key.len();
    let expected = &page[at..at + page[at..].find('"').unwrap()];
    let form = format!(
        "package={package}&custom={}&expected={expected}",
        urlencode(custom)
    );
    let r = b.post(PATH, &form).await;
    assert_eq!(r.status, 303, "{}", r.body);
    flash(b)
}

pub async fn admin<'a>(h: &Harness, app: &'a TestApp) -> Browser<'a> {
    signed_in(h, app, "admin", "Administrator").await
}

pub async fn stored(h: &Harness) -> Option<String> {
    let rows = h
        .db()
        .query(Statement::new("SELECT value FROM setting WHERE key = ? AND locale = ''").bind(KEY))
        .await
        .unwrap();
    rows.get(0, "value")
        .and_then(|v| v.as_text())
        .map(str::to_string)
}

pub async fn audits(h: &Harness, action: &str) -> i64 {
    int(
        h,
        &format!("SELECT count(*) FROM audit_log WHERE action = '{action}'"),
    )
    .await
}

pub async fn text(h: &Harness, sql: &str) -> String {
    let rows = h.db().query(Statement::new(sql.to_string())).await.unwrap();
    rows.rows[0][0].as_text().unwrap().to_string()
}
