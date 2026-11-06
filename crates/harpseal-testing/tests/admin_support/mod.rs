//! Setup for the admin pages tests (#27): a Deployment whose TOML has an
//! Administrator holding every Kernel Permission, a Viewer holding every
//! `.view` one, an Officer and a Reader (Students only), a `[texts]`
//! override, and the chat-flow roster (s1..s4).

#![allow(dead_code)]

use axum::body::Body;
use harpseal::{DeploymentConfig, Statement};
use harpseal_testing::{Browser, Harness, TestApp, TestResponse};
use http::Request;

pub const PASSWORD: &str = "kelp forest at dawn";

/// Every Kernel Permission for the Administrator.
pub const CONFIG: &str = r#"
name = "Seal Bay University (admin)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Administrator = [
    "kernel.student.view", "kernel.student.edit", "kernel.link.manage",
    "kernel.roster.view", "kernel.roster.edit", "kernel.roster.import",
    "kernel.usage.view", "kernel.security.view", "kernel.audit.view",
    "kernel.staff.manage", "kernel.setting.edit",
]
Viewer = [
    "kernel.student.view", "kernel.roster.view", "kernel.usage.view",
    "kernel.security.view", "kernel.audit.view",
]
Officer = ["kernel.student.view", "kernel.student.edit", "kernel.link.manage"]
Reader = ["kernel.student.view"]

[texts.th]
"kernel.menu.pick" = "เลือกบริการจากเมนูด้านล่างได้เลย"
"#;

/// Boots the Deployment and loads the chat-flow roster.
pub async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    let app = h.boot(h.builder().config(config).build().unwrap()).await;
    for sql in super::support::ROSTER {
        h.db().execute(Statement::new(*sql)).await.unwrap();
    }
    (h, app)
}

/// The email of the test Staff member holding `role`.
pub fn email_of(role: &str) -> String {
    format!("{}@sealbay.example", role.to_lowercase())
}

/// A signed-in browser for a new Staff member holding `role`; returns it
/// with `staff.id`.
pub async fn signed_in<'a>(h: &Harness, app: &'a TestApp, role: &str) -> (Browser<'a>, String) {
    let email = email_of(role);
    let id = h.create_staff(&email, PASSWORD, &[role]).await;
    let b = app.browser();
    let r = b.login(&email, PASSWORD).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(b.get("/admin").await.status, 200);
    (b, id)
}

/// `GET path` as HTMX does it.
pub async fn htmx_get(b: &Browser<'_>, path: &str) -> TestResponse {
    let req = Request::get(path)
        .header("hx-request", "true")
        .body(Body::empty())
        .unwrap();
    b.send(req).await
}

/// Runs one statement on the harness database.
pub async fn run(h: &Harness, sql: &str) {
    h.db()
        .execute(Statement::new(sql.to_string()))
        .await
        .unwrap();
}

/// How many audit rows have `action`.
pub async fn audits(h: &Harness, action: &str) -> i64 {
    let rows = h
        .db()
        .query(Statement::new("SELECT count(*) AS n FROM audit_log WHERE action = ?").bind(action))
        .await
        .unwrap();
    rows.get(0, "n").and_then(|v| v.as_i64()).unwrap_or(0)
}

/// Column `col` of the first row of `sql` with `param`, as text.
pub async fn text(h: &Harness, sql: &'static str, param: &str, col: &str) -> Option<String> {
    let rows = h.db().query(Statement::new(sql).bind(param)).await.unwrap();
    rows.get(0, col)
        .and_then(|v| v.as_text())
        .map(str::to_string)
}

/// The pending flash cookie (`ok:<key>` / `err:<key>`).
pub fn flash(b: &Browser<'_>) -> String {
    b.cookie("hs_flash").unwrap_or_default()
}

/// The value of the first hidden input `name` in `body`.
pub fn hidden(body: &str, name: &str) -> String {
    let key = format!("name=\"{name}\" value=\"");
    let at = body.find(&key).unwrap_or_else(|| panic!("no {name} field")) + key.len();
    let end = body[at..].find('"').unwrap();
    body[at..at + end].to_string()
}

/// The `version` of the Staff page of `id` (a fresh GET).
pub async fn staff_version(b: &Browser<'_>, id: &str) -> String {
    hidden(&b.get(&format!("/admin/staff/{id}")).await.body, "version")
}

/// The temporary password on a "shown once" page.
pub fn temp_password(body: &str) -> String {
    let key = "id=\"temp-password\">";
    let at = body.find(key).expect("a temporary password") + key.len();
    let end = body[at..].find('<').unwrap();
    body[at..at + end].to_string()
}

/// Rows a list page shows (`<tr>` in its `<tbody>`).
pub fn rows(body: &str) -> usize {
    body.split("<tbody>").nth(1).map_or(0, |t| {
        t.split("</tbody>")
            .next()
            .unwrap_or("")
            .matches("<tr")
            .count()
    })
}
