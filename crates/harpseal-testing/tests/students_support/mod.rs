//! Setup for the Students pages tests (#22): a Deployment whose TOML
//! declares a `[roster.attributes]` schema and Roles for every Students
//! Permission, the chat-flow roster, and signed-in browsers.

#![allow(dead_code)]

use axum::body::Body;
use harpseal::{DeploymentConfig, Statement};
use harpseal_testing::{Browser, Harness, TestApp, TestResponse};
use http::Request;

pub const PASSWORD: &str = "kelp forest at dawn";

/// A fictional university with a roster schema: year (integer, listed),
/// programme (enum, listed), entry date (optional date).
pub const CONFIG: &str = r#"
name = "Seal Bay University (students)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Officer = ["kernel.student.view", "kernel.student.edit", "kernel.link.manage"]
Viewer = ["kernel.student.view"]

[roster.attributes.year]
column = "ชั้นปี"
type = "integer"
label = { th = "ชั้นปี", en = "Year" }
shown_in_list = true
order = 2

[roster.attributes.programme]
column = "หลักสูตร"
type = "enum"
values = ["MSC", "MSC-Intl"]
label = { th = "หลักสูตร", en = "Programme" }
shown_in_list = true
order = 1

[roster.attributes.entered]
column = "วันเข้าศึกษา"
type = "date"
optional = true
label = { th = "วันเข้าศึกษา", en = "Entry date" }
"#;

/// Boots the Deployment and loads `support::ROSTER` (s1..s4), with
/// attributes on s1.
pub async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    let app = h.boot(h.builder().config(config).build().unwrap()).await;
    for sql in super::support::ROSTER {
        h.db().execute(Statement::new(*sql)).await.unwrap();
    }
    run(
        &h,
        "UPDATE student SET attributes = '{\"year\":2,\"programme\":\"MSC\"}' WHERE id = 's1'",
    )
    .await;
    (h, app)
}

/// Runs one statement on the harness database.
pub async fn run(h: &Harness, sql: &str) {
    h.db()
        .execute(Statement::new(sql.to_string()))
        .await
        .unwrap();
}

/// A signed-in browser for a new Staff member holding `role`; returns it
/// with `staff.id`.
pub async fn signed_in<'a>(h: &Harness, app: &'a TestApp, role: &str) -> (Browser<'a>, String) {
    let email = format!("{}@sealbay.example", role.to_lowercase());
    let id = h.create_staff(&email, PASSWORD, &[role]).await;
    let b = app.browser();
    let r = b.login(&email, PASSWORD).await;
    assert_eq!(r.status, 303, "{}", r.body);
    // Load a page so the browser holds the CSRF token.
    assert_eq!(b.get("/admin").await.status, 200);
    (b, id)
}

/// `GET path` as HTMX does it (`HX-Request: true`).
pub async fn htmx_get(b: &Browser<'_>, path: &str) -> TestResponse {
    let req = Request::get(path)
        .header("hx-request", "true")
        .body(Body::empty())
        .unwrap();
    b.send(req).await
}

/// How many Students a list page shows (links to a detail page).
pub fn rows_shown(body: &str) -> usize {
    body.matches("class=\"name\" href=\"/admin/students/")
        .count()
}

/// Column `col` of the first row of `sql`, as text.
pub async fn one(app: &TestApp, sql: &'static str, col: &str) -> Option<String> {
    let rows = app.query(sql).await;
    rows.get(0, col)
        .and_then(|v| v.as_text())
        .map(str::to_string)
}

/// The JSON of `before` / `after` of the newest audit row with `action`.
pub async fn audit_json(h: &Harness, action: &str) -> (serde_json::Value, serde_json::Value) {
    let rows = h
        .db()
        .query(
            Statement::new(
                "SELECT before, after FROM audit_log WHERE action = ? ORDER BY rowid DESC",
            )
            .bind(action),
        )
        .await
        .unwrap();
    let parse = |c: &str| {
        rows.get(0, c)
            .and_then(|v| v.as_text())
            .map_or(serde_json::Value::Null, |t| {
                serde_json::from_str(t).unwrap()
            })
    };
    (parse("before"), parse("after"))
}

/// The pending flash cookie (`ok:<key>` / `err:<key>`).
pub fn flash(b: &Browser<'_>) -> String {
    b.cookie("hs_flash").unwrap_or_default()
}

/// The `version` token of the edit form of Student `id` (a fresh GET).
pub async fn version(b: &Browser<'_>, id: &str) -> String {
    let body = b.get(&format!("/admin/students/{id}/edit")).await.body;
    let key = "name=\"version\" value=\"";
    let at = body.find(key).expect("a version field") + key.len();
    body[at..at + 64].to_string()
}
