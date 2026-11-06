//! Setup for the usage and Quota tests (#26): a Deployment with the Fake
//! Channel under `[quotas.fake]` (the example Deployment's LINE cap, 300)
//! and a second Fake Channel `fake2` with no Quota; Staff Roles with and
//! without `kernel.usage.view` and `kernel.staff.manage`.

#![allow(dead_code)]

use harpseal::{DeploymentConfig, Job, Statement};
use harpseal_testing::{Browser, Harness, START_MS, TestApp};

pub const PASSWORD: &str = "kelp forest at dawn";
/// The example Deployment's `[quotas.line] metered_per_month`.
pub const CAP: i64 = 300;
/// The local day at [`START_MS`] (07:00 in +07:00).
pub const TODAY: &str = "2026-11-05";
/// 00:30 on 1 December 2026 in +07:00 (17:30 on 30 November UTC).
pub const DEC_1_0030: i64 = START_MS + 25 * 86_400_000 + 17 * 3_600_000 + 30 * 60_000;

pub const CONFIG: &str = r#"
name = "Seal Bay University (usage)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Administrator = [
    "kernel.student.view", "kernel.usage.view", "kernel.audit.view",
    "kernel.staff.manage", "kernel.setting.edit",
]
Officer = ["kernel.student.view", "kernel.usage.view"]
Viewer = ["kernel.student.view"]

[quotas.fake]
metered_per_month = 300
"#;

/// Boots the Deployment (Fake `fake` with a Quota, `fake2` without).
pub async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    let d = h
        .builder()
        .config(config)
        .channel(h.fake_channel().with_kind("fake2"))
        .build()
        .unwrap();
    let app = h.boot(d).await;
    (h, app)
}

/// The email of the test Staff member `name`.
pub fn email_of(name: &str) -> String {
    format!("{}@sealbay.example", name.to_lowercase())
}

/// Creates Staff `name` holding `roles`; returns `staff.id`.
pub async fn staff(h: &Harness, name: &str, roles: &[&str]) -> String {
    h.create_staff(&email_of(name), PASSWORD, roles).await
}

/// A signed-in browser for a new Staff member `name` holding `role`.
pub async fn signed_in<'a>(h: &Harness, app: &'a TestApp, name: &str, role: &str) -> Browser<'a> {
    staff(h, name, &[role]).await;
    let b = app.browser();
    let r = b.login(&email_of(name), PASSWORD).await;
    assert_eq!(r.status, 303, "{}", r.body);
    b
}

/// Runs one statement on the harness database.
pub async fn run(h: &Harness, sql: &str) {
    h.db()
        .execute(Statement::new(sql.to_string()))
        .await
        .unwrap();
}

/// Sets `(channel, day)` in `usage_daily`.
pub async fn seed(h: &Harness, channel: &str, day: &str, metered: i64, free: i64, failed: i64) {
    h.db()
        .execute(
            Statement::new(
                "INSERT INTO usage_daily (channel, day, metered, free, failed) VALUES (?, ?, ?, ?, ?) \
                 ON CONFLICT (channel, day) DO UPDATE SET metered = excluded.metered, \
                 free = excluded.free, failed = excluded.failed",
            )
            .bind(channel)
            .bind(day)
            .bind(metered)
            .bind(free)
            .bind(failed),
        )
        .await
        .unwrap();
}

/// Sets today's metered pushes of `fake` to `percent` % of [`CAP`].
pub async fn use_percent(h: &Harness, percent: i64) {
    seed(h, "fake", TODAY, CAP * percent / 100, 0, 0).await;
}

/// Runs the watch Job once, outside any tick.
pub async fn watch(app: &TestApp) {
    app.deployment()
        .run_job(Job {
            id: "watch-test".into(),
            kind: "kernel.usage.watch".into(),
            payload: "{}".into(),
            attempt: 1,
        })
        .await
        .unwrap();
}

/// The banner slot's contents in `body`.
pub fn banner(body: &str) -> &str {
    let at = body.find("<div id=\"usage-banner\">").expect("a banner slot");
    let rest = &body[at..];
    let end = rest.find("<main>").expect("main after the banner");
    &rest[..end]
}

/// Integer `col` of the first row of `sql`.
pub async fn int(h: &Harness, sql: &str) -> i64 {
    let rows = h.db().query(Statement::new(sql.to_string())).await.unwrap();
    rows.rows
        .first()
        .and_then(|r| r.first())
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
}
